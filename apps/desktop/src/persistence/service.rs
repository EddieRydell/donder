use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Manager};

use super::model::*;
use crate::dto::{AppSettings, AppSnapshot, WorkspaceLayoutState};

const FILE_NAME: &str = "desktop-state-v2.json";
const MAX_RECENT_PROJECTS: usize = 10;

pub(crate) fn sequence_viewport_key(
    path: &str,
    object_key: &str,
    owned_path: &[crate::dto::GuiOwnedStep],
) -> Result<String, String> {
    let address =
        serde_json::to_string(&(object_key, owned_path)).map_err(|error| error.to_string())?;
    Ok(format!("{path}::{address}"))
}

fn remap_object_views<T>(views: &mut BTreeMap<String, T>, source: &str, destination: &str) {
    *views = std::mem::take(views)
        .into_iter()
        .map(|(key, state)| {
            let remapped = key.split_once("::").map_or(key.clone(), |(path, object)| {
                format!(
                    "{}::{object}",
                    remap_workspace_path(path, source, destination)
                )
            });
            (remapped, state)
        })
        .collect();
}

#[derive(Debug, Default)]
pub struct PersistenceService {
    inner: Mutex<PersistenceInner>,
}

impl PersistenceService {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(PersistenceInner::default()),
        }
    }

    pub fn load(&self, app: &AppHandle) -> Result<Option<String>, String> {
        let path = persistence_path(app)?;
        let mut inner = self.inner();
        inner.path = Some(path.clone());
        inner.write_allowed = true;
        if !path.exists() {
            inner.save_now()?;
            return Ok(None);
        }
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let store = match decode_store(&text) {
            Ok(store) => store,
            Err(error) => {
                eprintln!(
                    "Resetting invalid desktop state at {}: {error}",
                    path.display()
                );
                inner.store = PersistedStore::default();
                inner.last_saved_text = None;
                inner.save_now()?;
                return Ok(None);
            }
        };
        let last_project = store.last_project.clone();
        inner.store = store;
        Ok(last_project)
    }

    pub fn settings(&self) -> AppSettings {
        self.inner().store.settings.clone()
    }

    pub fn workspace_layout(&self) -> WorkspaceLayoutState {
        self.inner().store.workspace_layout.clone()
    }

    pub fn record_settings(&self, settings: AppSettings) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        inner.store.settings = settings;
        inner.save_now()
    }

    pub fn record_workspace_layout(&self, state: WorkspaceLayoutState) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        inner.store.workspace_layout = state;
        inner.save_now()
    }

    pub fn record_snapshot(&self, snapshot: &AppSnapshot) -> Result<(), String> {
        let Some(project_root) = snapshot.project_root.clone() else {
            return Ok(());
        };
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        let session = inner
            .store
            .projects
            .remove(&project_root)
            .unwrap_or_else(PersistedProjectSession::new);
        let session = session.with_snapshot(snapshot);
        inner.store.projects.insert(project_root.clone(), session);
        inner.store.last_project = Some(project_root);
        trim_recent_projects(&mut inner.store);
        inner.save_now()
    }

    pub fn record_editor_state(
        &self,
        project_root: &str,
        update: PersistedEditorViewStateUpdate,
    ) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        let session = inner
            .store
            .projects
            .entry(project_root.to_string())
            .or_insert_with(PersistedProjectSession::new);
        session.editor_states.insert(update.path, update.state);
        inner.save_now()
    }

    pub fn record_sequence_viewport(
        &self,
        project_root: &str,
        update: PersistedSequenceViewportStateUpdate,
    ) -> Result<(), String> {
        if !update.state.px_per_second.is_finite()
            || update.state.px_per_second <= 0.0
            || !update.state.audio_strip_height_px.is_finite()
            || update.state.audio_strip_height_px <= 0.0
            || !update.state.scroll_x_seconds.is_finite()
            || update.state.scroll_x_seconds < 0.0
            || !update.state.scroll_y.is_finite()
            || update.state.scroll_y < 0.0
            || update
                .state
                .row_heights
                .values()
                .any(|height| !height.is_finite() || *height <= 0.0)
        {
            return Err(
                "Sequence viewport geometry must be finite with positive row heights and zoom."
                    .into(),
            );
        }
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        let session = inner
            .store
            .projects
            .entry(project_root.to_string())
            .or_insert_with(PersistedProjectSession::new);
        session.sequence_viewports.insert(
            sequence_viewport_key(&update.path, &update.object_key, &update.owned_path)?,
            update.state,
        );
        inner.save_now()
    }

    pub fn record_graph_view(
        &self,
        project_root: &str,
        update: PersistedGraphViewStateUpdate,
    ) -> Result<(), String> {
        if update.state.viewport.as_ref().is_some_and(|view| {
            !view.x.is_finite() || !view.y.is_finite() || !view.zoom.is_finite() || view.zoom <= 0.0
        }) || update.state.node_sizes.values().any(|size| {
            !size.width.is_finite()
                || !size.height.is_finite()
                || size.width <= 0.0
                || size.height <= 0.0
        }) {
            return Err("Graph view geometry must be finite with positive sizes and zoom.".into());
        }
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        let session = inner
            .store
            .projects
            .entry(project_root.to_string())
            .or_insert_with(PersistedProjectSession::new);
        session.graph_views.insert(
            sequence_viewport_key(&update.path, &update.object_key, &update.owned_path)?,
            update.state,
        );
        inner.save_now()
    }

    pub fn record_spatial_view(
        &self,
        project_root: &str,
        update: PersistedSpatialViewStateUpdate,
    ) -> Result<(), String> {
        if update.state.guides.len() > 1000
            || update.state.guides.iter().any(|guide| {
                !guide.position_meters.is_finite() || guide.position_meters.abs() > 2000.0
            })
        {
            return Err(
                "Use at most 1,000 guides with positions between -2,000 and 2,000 meters.".into(),
            );
        }
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        let session = inner
            .store
            .projects
            .entry(project_root.to_string())
            .or_insert_with(PersistedProjectSession::new);
        session.spatial_views.insert(
            sequence_viewport_key(&update.path, &update.object_key, &update.owned_path)?,
            update.state,
        );
        inner.save_now()
    }

    pub fn remap_project_paths(
        &self,
        project_root: &str,
        source: &str,
        destination: &str,
    ) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        let Some(session) = inner.store.projects.get_mut(project_root) else {
            return Ok(());
        };
        for path in &mut session.tabs {
            *path = remap_workspace_path(path, source, destination);
        }
        if let Some(path) = &mut session.active_file {
            *path = remap_workspace_path(path, source, destination);
        }
        session.editor_states = std::mem::take(&mut session.editor_states)
            .into_iter()
            .map(|(path, state)| (remap_workspace_path(&path, source, destination), state))
            .collect();
        remap_object_views(&mut session.sequence_viewports, source, destination);
        remap_object_views(&mut session.graph_views, source, destination);
        remap_object_views(&mut session.spatial_views, source, destination);
        session.workspace_explorer.expanded_paths = session
            .workspace_explorer
            .expanded_paths
            .iter()
            .map(|path| remap_workspace_path(path, source, destination))
            .collect();
        session.workspace_explorer.recent_files = session
            .workspace_explorer
            .recent_files
            .iter()
            .map(|path| remap_workspace_path(path, source, destination))
            .collect();
        inner.save_now()
    }

    pub fn restore_for_project(
        &self,
        project_root: &str,
        valid_paths: &std::collections::BTreeSet<String>,
    ) -> Option<ProjectSessionRestore> {
        let inner = self.inner();
        let session = inner.store.projects.get(project_root)?.clone();
        let stale_tabs = session
            .tabs
            .iter()
            .filter(|path| !valid_paths.contains(*path))
            .cloned()
            .collect::<Vec<_>>();
        Some(ProjectSessionRestore {
            session,
            stale_tabs,
        })
    }

    pub fn restore_view_state(&self, project_root: &str) -> ProjectRestoreState {
        let inner = self.inner();
        let Some(session) = inner.store.projects.get(project_root) else {
            return ProjectRestoreState {
                editor_states: BTreeMap::new(),
                sequence_viewports: BTreeMap::new(),
                spatial_views: BTreeMap::new(),
                graph_views: BTreeMap::new(),
            };
        };
        ProjectRestoreState {
            editor_states: session.editor_states.clone(),
            sequence_viewports: session.sequence_viewports.clone(),
            spatial_views: session.spatial_views.clone(),
            graph_views: session.graph_views.clone(),
        }
    }

    pub fn record_main_window(&self, geometry: PersistedWindowState) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        inner.store.main_window = Some(geometry);
        inner.save_now()
    }

    pub fn main_window(&self) -> Option<PersistedWindowState> {
        self.inner().store.main_window.clone()
    }

    pub fn record_preview_window(&self, state: PersistedPreviewWindowState) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        inner.store.preview_window = state;
        inner.save_now()
    }

    pub fn preview_window(&self) -> PersistedPreviewWindowState {
        self.inner().store.preview_window.clone()
    }

    pub fn device_tokens(&self) -> std::collections::BTreeMap<String, String> {
        self.inner().store.device_tokens.clone()
    }

    pub fn record_device_token(&self, device: &str, token: String) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Err("Desktop settings are not loaded; the claim token cannot be saved.".into());
        }
        inner.store.device_tokens.insert(device.to_string(), token);
        inner.save_now()
    }

    fn inner(&self) -> MutexGuard<'_, PersistenceInner> {
        match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PersistenceInner {
    path: Option<PathBuf>,
    store: PersistedStore,
    write_allowed: bool,
    last_saved_text: Option<String>,
}

impl PersistenceInner {
    fn save_now(&mut self) -> Result<(), String> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        let parent = path
            .parent()
            .ok_or_else(|| "Persistence path has no parent directory.".to_string())?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let text = serde_json::to_string_pretty(&self.store).map_err(|error| error.to_string())?;
        if self.last_saved_text.as_ref() == Some(&text) {
            return Ok(());
        }
        let path = camino::Utf8Path::from_path(&path)
            .ok_or_else(|| "Persistence path is not valid UTF-8.".to_string())?;
        donder_project_io::atomic_write(path, text.as_bytes())
            .map_err(|error| error.to_string())?;
        self.last_saved_text = Some(text);
        Ok(())
    }
}

fn decode_store(text: &str) -> Result<PersistedStore, String> {
    let store: PersistedStore = serde_json::from_str(text).map_err(|error| error.to_string())?;
    store.validate()?;
    Ok(store)
}

fn persistence_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join(FILE_NAME))
        .map_err(|error| error.to_string())
}

fn trim_recent_projects(store: &mut PersistedStore) {
    if store.projects.len() <= MAX_RECENT_PROJECTS {
        return;
    }
    let protected = store.last_project.clone();
    let remove = store
        .projects
        .keys()
        .filter(|project| Some((*project).as_str()) != protected.as_deref())
        .take(store.projects.len().saturating_sub(MAX_RECENT_PROJECTS))
        .cloned()
        .collect::<Vec<_>>();
    for project in remove {
        store.projects.remove(&project);
    }
}

fn remap_workspace_path(path: &str, source: &str, destination: &str) -> String {
    if path == source {
        return destination.to_string();
    }
    path.strip_prefix(source)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .map(|suffix| format!("{destination}/{suffix}"))
        .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_sequence_row_heights_reach_disk_and_reopen_without_index_reassignment() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        let service = PersistenceService::new();
        {
            let mut inner = service.inner();
            inner.path = Some(path.clone());
            inner.write_allowed = true;
        }
        let settings = AppSettings::default();
        let height = settings.sequence_initial_lane_height_px;
        let mut state = PersistedSequenceViewportState {
            px_per_second: settings.sequence_initial_px_per_second,
            audio_strip_height_px: height,
            row_heights: BTreeMap::from([
                ("1001:row:0".into(), height),
                ("1001:row:1".into(), height * 2.0),
                ("1:row:0".into(), height * 2.0),
                ("1:row:1".into(), height),
            ]),
            scroll_x_seconds: 0.0,
            scroll_y: 0.0,
            active_mark_collection_key: None,
            visible_mark_collection_keys: Vec::new(),
        };
        let update = |state| PersistedSequenceViewportStateUpdate {
            path: "show.sequence.donder".into(),
            object_key: "show".into(),
            owned_path: Vec::new(),
            state,
        };
        service
            .record_sequence_viewport("project", update(state.clone()))
            .unwrap();
        // Resizing one row preserves the other kind, including an empty hidden row.
        state.row_heights.insert("1001:row:0".into(), height * 2.0);
        service
            .record_sequence_viewport("project", update(state.clone()))
            .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        let reopened = decode_store(&saved).unwrap();
        let key = sequence_viewport_key("show.sequence.donder", "show", &[]).unwrap();
        assert_eq!(
            reopened.projects["project"].sequence_viewports[&key].row_heights,
            state.row_heights
        );
        state.row_heights.insert("1001:row:1".into(), f32::NAN);
        assert!(
            service
                .record_sequence_viewport("project", update(state))
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    }

    #[test]
    fn owned_sequence_views_remain_distinct_after_document_move() {
        use crate::dto::GuiOwnedStep;
        let first =
            sequence_viewport_key("show.donder", "show", &[GuiOwnedStep::Sequence { id: 4 }])
                .unwrap();
        let second =
            sequence_viewport_key("show.donder", "show", &[GuiOwnedStep::Sequence { id: 8 }])
                .unwrap();
        assert_eq!(
            first,
            r#"show.donder::["show",[{"type":"sequence","id":4}]]"#
        );
        assert_ne!(first, second);
        let mut views = BTreeMap::from([(first, 10), (second, 20)]);
        remap_object_views(&mut views, "show.donder", "nested/show.donder");
        assert_eq!(
            views[&sequence_viewport_key(
                "nested/show.donder",
                "show",
                &[GuiOwnedStep::Sequence { id: 4 }]
            )
            .unwrap()],
            10
        );
        assert_eq!(
            views[&sequence_viewport_key(
                "nested/show.donder",
                "show",
                &[GuiOwnedStep::Sequence { id: 8 }]
            )
            .unwrap()],
            20
        );
    }

    #[test]
    fn spatial_guides_persist_per_owned_object_and_remap_with_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        let service = PersistenceService::new();
        {
            let mut inner = service.inner();
            inner.path = Some(path.clone());
            inner.write_allowed = true;
        }
        let update = |id, value| PersistedSpatialViewStateUpdate {
            path: "show.donder".into(),
            object_key: "show".into(),
            owned_path: vec![crate::dto::GuiOwnedStep::Fixture { id }],
            state: PersistedSpatialViewState {
                guides: vec![SpatialGuide {
                    axis: SpatialGuideAxis::X,
                    position_meters: value,
                }],
            },
        };
        service
            .record_spatial_view("project", update(1, 0.0254))
            .unwrap();
        service
            .record_spatial_view("project", update(2, -2.0))
            .unwrap();
        assert!(
            service
                .record_spatial_view("project", update(1, f64::NAN))
                .is_err()
        );
        service
            .remap_project_paths("project", "show.donder", "folder/show.donder")
            .unwrap();
        let stored = decode_store(&fs::read_to_string(path).unwrap()).unwrap();
        let views = &stored.projects["project"].spatial_views;
        assert_eq!(views.len(), 2);
        for (id, value) in [(1, 0.0254), (2, -2.0)] {
            let key = sequence_viewport_key(
                "folder/show.donder",
                "show",
                &[crate::dto::GuiOwnedStep::Fixture { id }],
            )
            .unwrap();
            assert_eq!(views[&key].guides[0].position_meters, value);
        }
    }

    #[test]
    fn current_desktop_state_roundtrips_and_other_versions_are_rejected() {
        let mut original = serde_json::to_value(PersistedStore::default()).unwrap();
        original["settings"]["autosaveProjectEdits"] = serde_json::json!(false);
        original["workspaceLayout"]["sidebarWidthPx"] = serde_json::json!(345.0);
        original["lastProject"] = serde_json::json!("C:/project");
        let loaded = decode_store(&original.to_string()).unwrap();
        assert_eq!(serde_json::to_value(loaded).unwrap(), original);
        for version in [VERSION - 1, VERSION + 1] {
            original["version"] = serde_json::json!(version);
            assert!(decode_store(&original.to_string()).is_err());
        }
    }

    #[test]
    fn rapid_persisted_changes_reach_disk_without_a_throttle() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        let service = PersistenceService::new();
        {
            let mut inner = service.inner();
            inner.path = Some(path.clone());
            inner.write_allowed = true;
        }
        service
            .record_workspace_layout(WorkspaceLayoutState::default())
            .unwrap();
        for cursor in [1, 41] {
            service
                .record_editor_state(
                    "C:/project",
                    PersistedEditorViewStateUpdate {
                        path: "project.donder".into(),
                        state: PersistedEditorViewState {
                            cursor_anchor: cursor,
                            cursor_head: cursor,
                            scroll_top: cursor as f32,
                        },
                    },
                )
                .unwrap();
        }
        let stored = decode_store(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            stored.projects["C:/project"].editor_states["project.donder"].cursor_head,
            41
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

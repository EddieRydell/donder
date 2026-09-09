use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Manager};

use super::model::*;
use crate::dto::{AppSettings, AppSnapshot, WorkspaceLayoutState};

const FILE_NAME: &str = "desktop-state-v2.json";
const MAX_RECENT_PROJECTS: usize = 10;

pub(crate) fn sequence_viewport_key(path: &str, object_key: &str) -> String {
    format!("{path}::{object_key}")
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
        if !path.exists() {
            inner.write_allowed = true;
            return Ok(None);
        }
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let store = decode_store(&text)?;
        let last_project = store.last_project.clone();
        inner.store = store;
        inner.write_allowed = true;
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
            sequence_viewport_key(&update.path, &update.object_key),
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
        session.sequence_viewports = std::mem::take(&mut session.sequence_viewports)
            .into_iter()
            .map(|(key, state)| {
                let remapped = key.split_once("::").map_or(key.clone(), |(path, object)| {
                    sequence_viewport_key(&remap_workspace_path(path, source, destination), object)
                });
                (remapped, state)
            })
            .collect();
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
            };
        };
        ProjectRestoreState {
            editor_states: session.editor_states.clone(),
            sequence_viewports: session.sequence_viewports.clone(),
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
        dawn_package::atomic_write(path, text.as_bytes()).map_err(|error| error.to_string())?;
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
                        path: "project.dawn".into(),
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
            stored.projects["C:/project"].editor_states["project.dawn"].cursor_head,
            41
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

use fs_err as fs;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};

use super::{
    DesktopState, LoadedProject, absolute_project_path, absolute_root_path, descriptor_for_path,
    generated_source_texts, lock_unpoisoned, path_matches_or_is_child, remap_workspace_path,
    valid_child_name,
};
use crate::persistence::ProjectRestoreState;
use crate::project::{new_project_files, write_new_project_files};
use donder_editor::dto::{AppSnapshot, EditorViewMode, ViewStateUpdate};
use donder_editor::dto::{
    WorkspacePathChangeImpact, WorkspacePathChangePlan, WorkspacePathChangeRequest,
};

impl DesktopState {
    pub(crate) fn open_project_path(&self, path: &str) -> AppSnapshot {
        let candidate = Utf8Path::new(path);
        let root = if candidate.is_dir() {
            candidate
        } else if candidate.file_name() == Some(donder_project_io::PROJECT_ROOT_FILE) {
            candidate.parent().unwrap_or(candidate)
        } else {
            return self.snapshot_with_error(
                "Open a project folder or its root project.data.donder document.",
            );
        };
        self.load_working_copy(root)
    }

    /// Keep one piece of view state: the workbench layout and explorer in the
    /// snapshot, an editor's state with the open project.
    pub(crate) fn save_view_state(&self, update: ViewStateUpdate) -> AppSnapshot {
        let snapshot = self.snapshot();
        let result = match (update, snapshot.project_root.as_deref()) {
            (ViewStateUpdate::WorkspaceLayout(state), _) => {
                return self.save_workspace_layout_state(state);
            }
            (ViewStateUpdate::WorkspaceExplorer(state), _) => {
                return self.update_snapshot(|snapshot| {
                    snapshot.workspace_explorer = state;
                });
            }
            (_, None) => return snapshot,
            (ViewStateUpdate::Editor(update), Some(root)) => self
                .persistence
                .record_editor_state(root, update)
                .map_err(|error| format!("Editor state was not saved: {error}")),
            (ViewStateUpdate::SequenceViewport(update), Some(root)) => self
                .persistence
                .record_sequence_viewport(root, update)
                .map_err(|error| format!("Sequence view state was not saved: {error}")),
            (ViewStateUpdate::Graph(update), Some(root)) => self
                .persistence
                .record_graph_view(root, update)
                .map_err(|error| format!("Graph view state was not saved: {error}")),
            (ViewStateUpdate::Spatial(update), Some(root)) => self
                .persistence
                .record_spatial_view(root, update)
                .map_err(|error| format!("Spatial view state was not saved: {error}")),
        };
        match result {
            Ok(()) => snapshot,
            Err(message) => self.set_persistence_error(message),
        }
    }

    pub(crate) fn restored_view_state(&self) -> ProjectRestoreState {
        let snapshot = self.snapshot();
        let Some(project_root) = snapshot.project_root.as_deref() else {
            return ProjectRestoreState {
                editor_states: Default::default(),
                sequence_viewports: Default::default(),
                spatial_views: Default::default(),
                graph_views: Default::default(),
            };
        };
        self.persistence.restore_view_state(project_root)
    }

    pub(crate) fn open_file_path(&self, path: &str) -> AppSnapshot {
        let Some(root) = self.project_root_path() else {
            return self.snapshot();
        };
        let relative = Utf8PathBuf::from(path);
        let project = self.project_session();
        let source_document = project
            .as_ref()
            .and_then(|project| donder_editor::document_for_editor_path(project, &relative));
        let known = lock_unpoisoned(&self.workspace)
            .documents
            .contains_key(&relative);
        if !known {
            let absolute = match (&project, &source_document) {
                (Some(project), Some(document)) => Some(project.source.absolute_path(document)),
                _ => absolute_root_path(&root, &relative),
            };
            let Some(absolute) = absolute else {
                return self.snapshot_with_error("Invalid source file path");
            };
            let text = match fs::read_to_string(&absolute) {
                Ok(text) => text,
                Err(error) => {
                    return self.snapshot_with_error(&error.to_string());
                }
            };
            let document = super::workspace_state::WorkingDocument::new(&relative, text);
            lock_unpoisoned(&self.workspace)
                .documents
                .insert(relative.clone(), document);
        }
        let descriptor = self
            .project_session()
            .and_then(|project| descriptor_for_path(&project, &relative));
        {
            let mut workspace = lock_unpoisoned(&self.workspace);
            let path = workspace.documents[&relative].buffer.path.clone();
            let document_path = Utf8PathBuf::from(&path);
            if !workspace.tabs.contains(&document_path) {
                workspace.tabs.push(document_path);
            }
            workspace.view.active_file = Some(path);
        }
        self.update_snapshot(|snapshot| {
            snapshot.active_document_descriptor = descriptor;
        })
    }

    /// The open project and how `request` changes it, if the project is still
    /// the one the request was made against.
    fn path_change(
        &self,
        request: &WorkspacePathChangeRequest,
    ) -> Result<
        (
            Arc<donder_project_io::ProjectSession>,
            donder_project_io::PathChangePlan,
        ),
        String,
    > {
        if request.project_revision != self.snapshot().project_revision {
            return Err("The project changed; plan the path operation again.".to_string());
        }
        let project = self
            .project_session()
            .ok_or_else(|| "No project is open.".to_string())?;
        let plan = donder_project_io::plan_path_change(
            &project,
            Utf8Path::new(&request.source),
            Utf8Path::new(&request.destination),
        )?;
        Ok((project, plan))
    }

    pub(crate) fn plan_workspace_path_change(
        &self,
        request: WorkspacePathChangeRequest,
    ) -> Result<WorkspacePathChangePlan, String> {
        let snapshot = self.snapshot();
        let (_, plan) = self.path_change(&request)?;
        let open_files = snapshot
            .tabs
            .iter()
            .filter(|buffer| path_matches_or_is_child(&buffer.path, &request.source))
            .map(|buffer| buffer.path.clone())
            .collect();
        let restore = snapshot
            .project_root
            .as_deref()
            .map(|root| self.persistence.restore_view_state(root));
        // Object views are keyed `path::object`; the impact lists their files.
        let persisted_state = restore
            .into_iter()
            .flat_map(|state| {
                state
                    .editor_states
                    .into_keys()
                    .chain(state.sequence_viewports.into_keys())
                    .chain(state.spatial_views.into_keys())
                    .chain(state.graph_views.into_keys())
            })
            .map(|key| {
                key.split_once("::")
                    .map_or(key.clone(), |(path, _)| path.to_string())
            })
            .filter(|path| path_matches_or_is_child(path, &request.source))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(WorkspacePathChangePlan {
            request,
            structural: plan.structural,
            impact: WorkspacePathChangeImpact {
                documents: plan.impact.documents,
                imports: plan.impact.imports,
                assets: plan.impact.assets,
                open_files,
                persisted_state,
            },
        })
    }

    pub(crate) fn apply_workspace_path_change(
        &self,
        request: WorkspacePathChangeRequest,
    ) -> Result<AppSnapshot, String> {
        let _authoring = self.settled_authoring();
        let (project, plan) = self.path_change(&request)?;
        if plan.structural
            && lock_unpoisoned(&self.workspace)
                .documents
                .values()
                .any(|document| document.buffer.dirty)
        {
            return Err(
                "Structural path changes require every open text buffer to be saved.".to_string(),
            );
        }
        self.working_copy.invalidate_pending();
        self.render_refresh.invalidate_pending();
        let candidate = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            donder_project_io::apply_path_change(&project, &plan)?
        };
        let root = candidate.source.project_root().to_string();
        let candidate = Arc::new(candidate);
        lock_unpoisoned(&self.gui_history).clear();
        let entries = super::workspace_entries(&candidate);
        let remap = |path: &str| remap_workspace_path(path, &request.source, &request.destination);
        let mut refresh_errors = Vec::new();
        {
            let mut workspace = lock_unpoisoned(&self.workspace);
            workspace.documents = std::mem::take(&mut workspace.documents)
                .into_iter()
                .map(|(path, mut document)| {
                    let path = Utf8PathBuf::from(remap(path.as_str()));
                    document.buffer.path = path.to_string();
                    document.buffer.name = path.file_name().unwrap_or(path.as_str()).to_string();
                    (path, document)
                })
                .collect();
            for (path, document) in &mut workspace.documents {
                if document.buffer.dirty {
                    continue;
                }
                let refreshed = super::working_copy::read_disk(&Utf8Path::new(&root).join(path))
                    .and_then(|bytes| {
                        bytes.ok_or_else(|| format!("{path} is missing after the rename"))
                    })
                    .and_then(|bytes| {
                        let text =
                            String::from_utf8(bytes.clone()).map_err(|error| error.to_string())?;
                        Ok((bytes, text))
                    });
                match refreshed {
                    Ok((bytes, text)) => {
                        document.observed = Some(bytes);
                        document.edit(text);
                    }
                    Err(error) => refresh_errors.push(format!("{path}: {error}")),
                }
            }
            workspace.tabs = workspace
                .tabs
                .iter()
                .map(|path| Utf8PathBuf::from(remap(path.as_str())))
                .collect();
            workspace.view.active_file = workspace.view.active_file.as_deref().map(remap);
            // A move keeps every id; the target follows its objects to their new addresses.
            workspace.render_target = workspace.render_target.as_ref().and_then(|target| {
                let sequence = target.resolve(&project)?;
                super::workspace_state::RenderTarget::new(&candidate, sequence)
            });
            workspace.view.project_revision += 1;
            workspace.view.state_revision += 1;
            if refresh_errors.is_empty() {
                workspace.typed_revision = Some(workspace.view.project_revision);
                workspace.project = LoadedProject::Ready(Arc::clone(&candidate));
            } else {
                workspace.typed_revision = None;
                workspace.project = LoadedProject::Invalid;
                workspace.view.project_health = donder_editor::dto::ProjectHealth::Invalid;
                workspace.view.settings.editor_view_mode = EditorViewMode::Text;
            }
        }
        let persistence_error = self
            .persistence
            .remap_project_paths(&root, &request.source, &request.destination)
            .err();
        let snapshot = self.update_snapshot(|snapshot| {
            snapshot.active_document_descriptor = snapshot
                .active_file
                .as_deref()
                .filter(|_| refresh_errors.is_empty())
                .and_then(|path| super::descriptor_for_path(&candidate, Utf8Path::new(path)));
            snapshot.workspace_explorer.expanded_paths = snapshot
                .workspace_explorer
                .expanded_paths
                .iter()
                .map(|path| remap(path))
                .collect();
            snapshot.project_entries = entries;
            snapshot.status = if plan.structural {
                "Structural path change applied; GUI undo and redo history were cleared."
                    .to_string()
            } else {
                "Path change applied.".to_string()
            };
            if !refresh_errors.is_empty() {
                snapshot.status.push_str(&format!(
                    " Source refresh failed: {}",
                    refresh_errors.join("; ")
                ));
            }
            if let Some(error) = &persistence_error {
                snapshot
                    .status
                    .push_str(&format!(" View state was not saved: {error}"));
            }
        });
        if refresh_errors.is_empty() {
            self.schedule_render_refresh(candidate);
        } else {
            self.invalidate_prepared_project();
        }
        Ok(snapshot)
    }

    pub(crate) fn set_active_file_path(&self, path: &str) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        self.open_file_path(path)
    }

    /// Move the tab for `path` to `index` among the tabs and make it active.
    pub(crate) fn move_tab(&self, path: &str, index: u32) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        {
            let mut workspace = lock_unpoisoned(&self.workspace);
            if let Some(from) = workspace.tabs.iter().position(|item| item == path) {
                let tab = workspace.tabs.remove(from);
                let index = (index as usize).min(workspace.tabs.len());
                workspace.tabs.insert(index, tab);
            }
        }
        self.open_file_path(path)
    }

    pub(super) fn close_file_path(&self, path: &str) -> AppSnapshot {
        let path = Utf8Path::new(path);
        let next = {
            let mut workspace = lock_unpoisoned(&self.workspace);
            let Some(index) = workspace.tabs.iter().position(|item| item == path) else {
                return workspace.snapshot();
            };
            workspace.tabs.remove(index);
            if workspace.view.active_file.as_deref().map(Utf8Path::new) == Some(path) {
                workspace.view.active_file = workspace
                    .tabs
                    .get(index)
                    .or_else(|| workspace.tabs.last())
                    .map(ToString::to_string);
            }
            workspace.view.active_file.clone()
        };
        if let Some(next) = next {
            self.open_file_path(&next)
        } else {
            self.update_snapshot(|snapshot| snapshot.active_document_descriptor = None)
        }
    }

    pub(crate) fn set_editor_view_mode(&self, mode: EditorViewMode) -> AppSnapshot {
        if matches!(mode, EditorViewMode::Gui) {
            if let Err(error) = self.reconcile_external_files() {
                return self.snapshot_with_error(&error);
            }
            let _authoring = self.settled_authoring();
            if self.project_session().is_none() {
                self.focus_first_diagnostic();
                return self.update_snapshot(|snapshot| {
                    super::show_problems(&mut snapshot.settings, &mut snapshot.workspace_layout);
                    snapshot.status = "Fix the project errors before entering GUI".into();
                });
            }
            let mut settings = self.snapshot().settings;
            settings.editor_view_mode = mode;
            return self.update_app_settings_locked(settings);
        }
        let mut settings = self.snapshot().settings;
        settings.editor_view_mode = mode;
        self.update_app_settings(settings)
    }

    pub(crate) fn create_file(&self, parent: &str, name: &str) -> AppSnapshot {
        self.create_fs_entry(parent, name, donder_editor::dto::WorkspaceEntryKind::File)
    }

    pub(crate) fn create_directory(&self, parent: &str, name: &str) -> AppSnapshot {
        self.create_fs_entry(
            parent,
            name,
            donder_editor::dto::WorkspaceEntryKind::Directory,
        )
    }

    pub(super) fn create_new_project_locked(
        &self,
        parent_path: &str,
        directory_name: &str,
        initial_color: &str,
    ) -> AppSnapshot {
        let root = match project_destination(parent_path, directory_name) {
            Ok(root) => root,
            Err(error) => return self.snapshot_with_error(&error),
        };
        let files = match new_project_files(directory_name, initial_color) {
            Ok(files) => files,
            Err(error) => {
                return self.snapshot_with_error(&error);
            }
        };
        let result = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            write_new_project_files(&root, &files)
        };
        if let Err(error) = result {
            return self.snapshot_with_error(&error);
        }
        self.open_created_project_locked(&root)
    }

    /// A new project from the template, then the profile in
    /// `profile_path` imported into it. What was left out is written to
    /// [`IMPORT_REPORT_FILE`] in the project.
    pub(super) fn import_project_locked(
        &self,
        parent_path: &str,
        directory_name: &str,
        initial_color: &str,
        profile_path: &str,
    ) -> AppSnapshot {
        let root = match project_destination(parent_path, directory_name) {
            Ok(root) => root,
            Err(error) => {
                return self.snapshot_with_error(&error);
            }
        };
        let files = match new_project_files(directory_name, initial_color) {
            Ok(files) => files,
            Err(error) => {
                return self.snapshot_with_error(&error);
            }
        };
        let imported = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            write_new_project_files(&root, &files).and_then(|()| {
                let imported = import_profile(&root, Utf8Path::new(profile_path));
                if imported.is_err() {
                    let _ = fs_err::remove_dir_all(&root);
                }
                imported
            })
        };
        let report = match imported {
            Ok(report) => report,
            Err(error) => {
                return self.snapshot_with_error(&error);
            }
        };
        let opened = self.open_created_project_locked(&root);
        if opened.project_root.as_deref() != Some(root.as_str()) {
            return opened;
        }
        let left_out = report.skipped.len()
            + report.unrouted.len()
            + report.skipped_controllers.len()
            + report.dropped_members.len();
        self.update_snapshot(|snapshot| {
            snapshot.status = if left_out == 0 {
                format!("Imported {} fixtures", report.fixtures)
            } else {
                format!(
                    "Imported {} fixtures; {left_out} models, controllers and group members left out or not patched are listed in {IMPORT_REPORT_FILE}",
                    report.fixtures
                )
            };
        })
    }

    fn open_created_project_locked(&self, root: &Utf8Path) -> AppSnapshot {
        let opened = self.load_working_copy(root);
        if opened.project_root.as_deref() != Some(root.as_str()) {
            return opened;
        }
        if self.project_session().is_none() {
            return self.snapshot();
        }
        self.open_file_path(donder_project_io::PROJECT_ROOT_FILE);
        // The workspace transition already holds the authoring lock, and the
        // freshly loaded typed project has passed analysis.
        let mut settings = self.snapshot().settings;
        settings.editor_view_mode = EditorViewMode::Gui;
        self.update_app_settings_locked(settings)
    }

    /// What deleting `path` also changes in the documents that stay, a
    /// sentence each, for the person to confirm; why it cannot be deleted.
    pub(crate) fn plan_delete_path(&self, path: &str) -> Result<Vec<String>, String> {
        let _authoring = self.settled_authoring();
        let project = self.project_session().ok_or("No project is loaded.")?;
        let (before, edited) = self.deleted_documents(&project, path)?;
        Ok(match edited {
            Some(mut edited) => {
                donder_editor::repair_changes(&before, &edited.project.take_repairs())
            }
            None => Vec::new(),
        })
    }

    /// Delete `path` from disk. The objects its documents declare go from the
    /// project, and what the documents that stay held that referred to them
    /// gives way.
    pub(crate) fn delete_path(&self, path: &str) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        if lock_unpoisoned(&self.workspace)
            .documents
            .values()
            .any(|document| {
                document.buffer.dirty && path_matches_or_is_child(&document.buffer.path, path)
            })
        {
            return self.snapshot_with_error("Save or discard edits before deleting this path");
        }
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let Some(absolute_path) = absolute_project_path(&project, Utf8Path::new(path)) else {
            return self.snapshot_with_error("Path is outside the loaded project");
        };
        let edited = match self.deleted_documents(&project, path) {
            Ok((_, edited)) => edited,
            Err(message) => return self.snapshot_with_error(&message),
        };
        let texts = match &edited {
            Some(edited) => {
                let paths = edited
                    .source
                    .documents
                    .keys()
                    .map(|document| document.path().to_string())
                    .collect();
                match generated_source_texts(
                    edited,
                    &paths,
                    &mut lock_unpoisoned(&self.document_texts),
                ) {
                    Ok(texts) => Some(texts),
                    Err(message) => return self.snapshot_with_error(&message),
                }
            }
            None => None,
        };
        let result = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            if absolute_path.is_dir() {
                fs::remove_dir_all(&absolute_path)
            } else {
                fs::remove_file(&absolute_path)
            }
        };
        if let Err(error) = result {
            return self.snapshot_with_error(&error.to_string());
        }
        if let (Some(mut edited), Some(texts)) = (edited, texts) {
            edited.project.take_repairs();
            // Undo cannot bring the deleted files back.
            lock_unpoisoned(&self.gui_history).clear();
            if let Err(message) = self.accept_gui_sources(Arc::new(edited), texts, "Deleted") {
                return self.snapshot_with_error(&message);
            }
        }
        self.after_workspace_changed(Some(path), None)
    }

    /// The project before and after the documents at or below `path` are
    /// removed; none after when no project document is there.
    fn deleted_documents(
        &self,
        project: &Arc<donder_project_io::ProjectSession>,
        path: &str,
    ) -> Result<
        (
            Arc<donder_project_io::ProjectSession>,
            Option<donder_project_io::ProjectSession>,
        ),
        String,
    > {
        if project
            .source
            .entrypoint
            .as_ref()
            .is_some_and(|entrypoint| path_matches_or_is_child(entrypoint.path().as_str(), path))
        {
            return Err("The project entrypoint cannot be deleted.".into());
        }
        let documents = project
            .source
            .documents
            .keys()
            .filter(|document| path_matches_or_is_child(document.path().as_str(), path))
            .cloned()
            .collect::<Vec<_>>();
        if documents.is_empty() {
            return Ok((Arc::clone(project), None));
        }
        let mut edited = (**project).clone();
        donder_project_io::remove_documents(&mut edited, &documents).map_err(
            |error| match error {
                donder_model::EditError::Invalid(error) => {
                    donder_project_io::validation_message(project, &error)
                }
                donder_model::EditError::Rejected(rejection) => rejection.to_string(),
            },
        )?;
        Ok((Arc::clone(project), Some(edited)))
    }
}

fn project_destination(parent_path: &str, directory_name: &str) -> Result<Utf8PathBuf, String> {
    if !valid_child_name(directory_name) {
        return Err("Project folder name must be a single path segment".into());
    }
    let parent = Utf8Path::new(parent_path);
    if !parent.is_dir() {
        return Err("Parent location is not a directory".into());
    }
    let root = parent.join(directory_name);
    if root.exists() {
        return Err("Project folder already exists".into());
    }
    Ok(root)
}

/// Where an import lists what it left out.
const IMPORT_REPORT_FILE: &str = "import-report.txt";

/// Import the profile at `profile` into the new project at `root`.
fn import_profile(
    root: &Utf8Path,
    profile: &Utf8Path,
) -> Result<donder_import::ImportReport, String> {
    let mut session =
        donder_project_io::check_project(root, &Default::default(), &mut Default::default())
            .into_session()
            .map_err(|diagnostics| {
                let lines: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
                format!("The new project does not load: {}", lines.join("; "))
            })?;
    let report = donder_import::import_profile(
        &mut session,
        profile,
        &donder_import::ImportOptions::default(),
    )
    .map_err(|error| error.to_string())?;
    donder_project_io::save_project(&session).map_err(|error| error.to_string())?;
    fs_err::write(root.join(IMPORT_REPORT_FILE), report.to_string())
        .map_err(|error| error.to_string())?;
    Ok(report)
}

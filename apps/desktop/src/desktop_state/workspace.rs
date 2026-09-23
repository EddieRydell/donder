use std::fs;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};

use super::{
    DesktopState, FsEntryKind, LoadedProject, absolute_project_path, absolute_root_path,
    descriptor_for_path, lock_unpoisoned, path_matches_or_is_child, valid_child_name,
};
use crate::dto::{AppSnapshot, EditorViewMode, NewSequenceRequest};
use crate::dto::{
    WorkspaceExplorerState, WorkspacePathChangeImpact, WorkspacePathChangePlan,
    WorkspacePathChangeRequest, WorkspacePathOwnership,
};
use crate::persistence::{
    PersistedEditorViewStateUpdate, PersistedSequenceViewportStateUpdate, ProjectRestoreState,
};
use crate::project::{new_project_files, write_new_project_files};

impl DesktopState {
    pub fn open_project_path(&self, path: &str) -> AppSnapshot {
        let candidate = Utf8Path::new(path);
        let root = if candidate.is_dir() {
            candidate
        } else if candidate.file_name() == Some(donder_package::MANIFEST_FILE) {
            candidate.parent().unwrap_or(candidate)
        } else {
            return self.snapshot_with_error(
                "project.open",
                path,
                "Open a project by selecting its donder-package.json manifest.",
            );
        };
        self.load_working_copy(root)
    }

    pub fn save_editor_view_state(&self, update: PersistedEditorViewStateUpdate) -> AppSnapshot {
        let snapshot = self.snapshot();
        let Some(project_root) = snapshot.project_root.clone() else {
            return snapshot;
        };
        match self.persistence.record_editor_state(&project_root, update) {
            Ok(()) => snapshot,
            Err(error) => {
                self.set_persistence_error(format!("Editor state was not saved: {error}"))
            }
        }
    }

    pub fn save_sequence_viewport_state(
        &self,
        update: PersistedSequenceViewportStateUpdate,
    ) -> AppSnapshot {
        let snapshot = self.snapshot();
        let Some(project_root) = snapshot.project_root.clone() else {
            return snapshot;
        };
        match self
            .persistence
            .record_sequence_viewport(&project_root, update)
        {
            Ok(()) => snapshot,
            Err(error) => {
                self.set_persistence_error(format!("Sequence view state was not saved: {error}"))
            }
        }
    }

    pub fn restored_view_state(&self) -> ProjectRestoreState {
        let snapshot = self.snapshot();
        let Some(project_root) = snapshot.project_root.as_deref() else {
            return ProjectRestoreState {
                editor_states: Default::default(),
                sequence_viewports: Default::default(),
            };
        };
        self.persistence.restore_view_state(project_root)
    }

    pub fn open_file_path(&self, path: &str) -> AppSnapshot {
        let Some(root) = self.project_root_path() else {
            return self.snapshot();
        };
        let relative = Utf8PathBuf::from(path);
        let project = self.project_session();
        let source_document = project.as_ref().and_then(|project| {
            crate::source_documents::document_for_editor_path(project, &relative)
        });
        let read_only = match (&project, &source_document) {
            (Some(project), Some(document)) => !project.source.is_project_owned(document),
            _ => false,
        };
        let known = lock_unpoisoned(&self.workspace)
            .documents
            .contains_key(&relative);
        if !known {
            let absolute = match (&project, &source_document) {
                (Some(project), Some(document)) => project.source.absolute_path(document),
                _ => absolute_root_path(&root, &relative),
            };
            let Some(absolute) = absolute else {
                return self.snapshot_with_error("file.open", path, "Invalid source file path");
            };
            let text = match fs::read_to_string(&absolute) {
                Ok(text) => text,
                Err(error) => {
                    return self.snapshot_with_error("file.open", path, &error.to_string());
                }
            };
            let mut document = super::workspace_state::WorkingDocument::new(&relative, text);
            document.buffer.read_only = read_only;
            lock_unpoisoned(&self.workspace)
                .documents
                .insert(relative.clone(), document);
        }
        let descriptor = self
            .project_session()
            .and_then(|project| descriptor_for_path(&project, &relative));
        let path = {
            let mut workspace = lock_unpoisoned(&self.workspace);
            let path = workspace.documents[&relative].buffer.path.clone();
            let document_path = Utf8PathBuf::from(&path);
            if !workspace.tabs.contains(&document_path) {
                workspace.tabs.push(document_path);
            }
            workspace.view.active_file = Some(path.clone());
            path
        };
        self.update_snapshot(|snapshot| {
            snapshot.active_document_descriptor = descriptor;
            snapshot
                .workspace_explorer
                .recent_files
                .retain(|item| Utf8Path::new(item) != Utf8Path::new(&path));
            snapshot.workspace_explorer.recent_files.insert(0, path);
            snapshot.workspace_explorer.recent_files.truncate(20);
        })
    }

    pub fn save_workspace_explorer_state(&self, state: WorkspaceExplorerState) -> AppSnapshot {
        self.update_snapshot(|snapshot| {
            snapshot.workspace_explorer = state;
        })
    }

    pub fn plan_workspace_path_change(
        &self,
        request: WorkspacePathChangeRequest,
    ) -> Result<WorkspacePathChangePlan, String> {
        let snapshot = self.snapshot();
        if request.project_revision != snapshot.project_revision {
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
        let open_files = snapshot
            .tabs
            .iter()
            .filter(|buffer| path_matches_or_is_child(&buffer.path, &request.source))
            .map(|buffer| buffer.path.clone())
            .collect();
        let recent_files = snapshot
            .workspace_explorer
            .recent_files
            .iter()
            .filter(|path| path_matches_or_is_child(path, &request.source))
            .cloned()
            .collect();
        let restore = snapshot
            .project_root
            .as_deref()
            .map(|root| self.persistence.restore_view_state(root));
        let persisted_state = restore
            .into_iter()
            .flat_map(|state| {
                state
                    .editor_states
                    .into_keys()
                    .chain(state.sequence_viewports.into_keys())
            })
            .filter(|path| path_matches_or_is_child(path, &request.source))
            .collect();
        Ok(WorkspacePathChangePlan {
            request,
            structural: plan.structural,
            ownership: match plan.ownership {
                donder_project_io::PathChangeOwnership::Project => WorkspacePathOwnership::Project,
                donder_project_io::PathChangeOwnership::PathDependency {
                    module_id,
                    module_root,
                } => WorkspacePathOwnership::PathDependency {
                    module_id: module_id.to_string(),
                    module_root,
                },
            },
            impact: WorkspacePathChangeImpact {
                documents: plan.impact.documents,
                imports: plan.impact.imports,
                manifests: plan.impact.manifests,
                assets: plan.impact.assets,
                modules: plan.impact.modules,
                open_files,
                recent_files,
                persisted_state,
            },
        })
    }

    pub fn apply_workspace_path_change(
        &self,
        request: WorkspacePathChangeRequest,
    ) -> Result<AppSnapshot, String> {
        let _authoring = self.settled_authoring();
        let description = self.plan_workspace_path_change(request.clone())?;
        if description.structural
            && lock_unpoisoned(&self.workspace)
                .documents
                .values()
                .any(|document| document.buffer.dirty)
        {
            return Err(
                "Structural path changes require every open text buffer to be saved.".to_string(),
            );
        }
        let project = self
            .project_session()
            .ok_or_else(|| "No project is open.".to_string())?;
        let plan = donder_project_io::plan_path_change(
            &project,
            Utf8Path::new(&request.source),
            Utf8Path::new(&request.destination),
        )?;
        self.working_copy.invalidate_pending();
        self.render_refresh.invalidate_pending();
        let candidate = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            donder_project_io::apply_path_change(&project, &plan)?
        };
        let root = candidate.source.project_root().to_string();
        let candidate = std::sync::Arc::new(candidate);
        lock_unpoisoned(&self.gui_history).clear();
        let entries = super::workspace_entries(&candidate);
        let package = super::package_status(Utf8Path::new(&root), Some(&candidate));
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
            workspace.render_target = workspace.render_target.as_ref().map(|(setup, sequence)| {
                (
                    donder_language::setup::SetupId(plan.remap_identity(&setup.0)),
                    donder_language::sequence::SequenceId(plan.remap_identity(&sequence.0)),
                )
            });
            workspace.view.project_revision += 1;
            workspace.view.state_revision += 1;
            if refresh_errors.is_empty() {
                workspace.typed_revision = Some(workspace.view.project_revision);
                workspace.project = LoadedProject::Ready(std::sync::Arc::clone(&candidate));
            } else {
                workspace.typed_revision = None;
                workspace.project = LoadedProject::Invalid;
                workspace.view.project_health = crate::dto::ProjectHealth::Invalid;
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
            snapshot.workspace_explorer.recent_files = snapshot
                .workspace_explorer
                .recent_files
                .iter()
                .map(|path| remap(path))
                .collect();
            snapshot.project_entries = entries;
            snapshot.package = package;
            snapshot.status = if description.structural {
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

    pub fn set_active_file_path(&self, path: &str) -> AppSnapshot {
        let _authoring = self.settled_authoring();
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

    pub fn set_editor_view_mode(&self, mode: EditorViewMode) -> AppSnapshot {
        if matches!(mode, EditorViewMode::Gui) {
            if let Err(error) = self.reconcile_external_files() {
                return self.snapshot_with_error("project.reconcile", "", &error);
            }
            let _authoring = self.settled_authoring();
            if self.project_session().is_none() {
                self.focus_first_diagnostic();
                return self.update_snapshot(|snapshot| {
                    snapshot.settings.editor_view_mode = EditorViewMode::Text;
                    snapshot.workspace_layout.active_sidebar_view =
                        crate::dto::SidebarView::Problems;
                    snapshot.workspace_layout.sidebar_collapsed = false;
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

    pub fn create_file(&self, parent: &str, name: &str) -> AppSnapshot {
        self.create_fs_entry(parent, name, FsEntryKind::File)
    }

    pub fn create_directory(&self, parent: &str, name: &str) -> AppSnapshot {
        self.create_fs_entry(parent, name, FsEntryKind::Directory)
    }

    pub(super) fn create_new_project_locked(
        &self,
        parent_path: &str,
        directory_name: &str,
    ) -> AppSnapshot {
        let root = match project_destination(parent_path, directory_name) {
            Ok(root) => root,
            Err(error) => return self.snapshot_with_error("project.create", parent_path, &error),
        };
        let files = match new_project_files(directory_name) {
            Ok(files) => files,
            Err(error) => {
                return self.snapshot_with_error("project.create", root.as_str(), &error);
            }
        };
        let result = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            write_new_project_files(&root, &files)
        };
        if let Err(error) = result {
            return self.snapshot_with_error("project.create", root.as_str(), &error);
        }
        self.open_created_project_locked(&root)
    }

    pub(super) fn copy_project_locked(
        &self,
        parent_path: &str,
        directory_name: &str,
        discard: bool,
    ) -> AppSnapshot {
        let root = match project_destination(parent_path, directory_name) {
            Ok(root) => root,
            Err(error) => return self.snapshot_with_error("project.copy", parent_path, &error),
        };
        let project = if discard {
            let Some(original_root) = self.project_root_path() else {
                return self.snapshot_with_error("project.copy", parent_path, "No project is open");
            };
            match donder_project_io::load_package(&original_root) {
                Ok(loaded) => std::sync::Arc::new(loaded.session),
                Err(error) => {
                    return self.snapshot_with_error(
                        "project.copy",
                        original_root.as_str(),
                        &format!("The saved project cannot be copied: {error:?}"),
                    );
                }
            }
        } else {
            let Some(project) = self.project_session() else {
                return self.snapshot_with_error(
                    "project.copy",
                    parent_path,
                    "Fix project errors before creating an editable copy",
                );
            };
            project
        };
        let result = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            donder_project_io::export_editable_project(&project, &root)
        };
        if let Err(error) = result {
            return self.snapshot_with_error("project.copy", root.as_str(), &error);
        }
        self.open_created_project_locked(&root)
    }

    fn open_created_project_locked(&self, root: &Utf8Path) -> AppSnapshot {
        let opened = self.load_working_copy(root);
        if opened.project_root.as_deref() != Some(root.as_str()) {
            return opened;
        }
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let entrypoint = project
            .source
            .entrypoint
            .as_ref()
            .map(|document| document.path().as_str().to_string())
            .unwrap_or_else(|| project.project.root.id.0.document().to_string());
        self.open_file_path(&entrypoint);
        // The workspace transition already holds the authoring lock, and the
        // freshly loaded typed project has passed analysis.
        let mut settings = self.snapshot().settings;
        settings.editor_view_mode = EditorViewMode::Gui;
        self.update_app_settings_locked(settings)
    }

    pub fn create_sequence(&self, request: NewSequenceRequest) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let sequence_path = Utf8PathBuf::from(&request.file_path);
        let Ok(duration) = Duration::try_from_secs_f32(request.duration_seconds) else {
            return self.snapshot_with_error(
                "sequence.create",
                &request.file_path,
                "Sequence duration is outside the supported range",
            );
        };
        let mut edited = (*project).clone();
        let result = donder_project_io::insert_sequence(
            &mut edited,
            sequence_path.clone(),
            request.object_key.clone(),
            donder_language::values::DonderDuration(duration),
            request.frame_rate,
        );
        if let Err(error) = result {
            return self.snapshot_with_error(
                "sequence.create",
                &request.file_path,
                &error.to_string(),
            );
        }
        let mut paths = std::collections::BTreeSet::from([request.file_path.clone()]);
        if let Some(entrypoint) = &edited.source.entrypoint {
            paths.insert(entrypoint.path().to_string());
        }
        let texts = match super::generated_source_texts(&edited, &paths) {
            Ok(texts) => texts,
            Err(error) => {
                return self.snapshot_with_error("sequence.create", &request.file_path, &error);
            }
        };
        if let Err(error) =
            self.accept_gui_sources(std::sync::Arc::new(edited), texts, "Sequence created")
        {
            return self.snapshot_with_error("sequence.create", &request.file_path, &error);
        }
        lock_unpoisoned(&self.gui_history).clear();
        self.open_file_path(&request.file_path)
    }

    pub fn delete_path(&self, path: &str) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        if lock_unpoisoned(&self.workspace)
            .documents
            .values()
            .any(|document| {
                document.buffer.dirty && path_matches_or_is_child(&document.buffer.path, path)
            })
        {
            return self.snapshot_with_error(
                "file.delete",
                path,
                "Save or discard edits before deleting this path",
            );
        }
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let relative_path = Utf8PathBuf::from(path);
        if project.source.is_structural_workspace_path(&relative_path) {
            return self.snapshot_with_error(
                "file.delete",
                path,
                "Imported documents and the project entrypoint cannot be deleted from the workspace.",
            );
        }
        let Some(absolute_path) = absolute_project_path(&project, &relative_path) else {
            return self.snapshot_with_error(
                "file.delete",
                path,
                "Path is outside the loaded project",
            );
        };
        let result = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            if absolute_path.is_dir() {
                fs::remove_dir_all(&absolute_path)
            } else {
                fs::remove_file(&absolute_path)
            }
        };
        match result {
            Ok(()) => self.after_workspace_changed(Some(path), None),
            Err(error) => self.snapshot_with_error("file.delete", path, &error.to_string()),
        }
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

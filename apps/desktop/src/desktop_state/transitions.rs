use super::{DesktopState, LoadedProject, lock_unpoisoned};
use crate::dto::*;
use camino::Utf8Path;

impl DesktopState {
    pub fn request_transition(
        &self,
        request: TransitionRequest,
    ) -> Result<TransitionResult, String> {
        self.working_copy.finish_pending();
        self.reconcile_external_files()?;
        let _authoring = self.settled_authoring();
        let snapshot = self.snapshot();
        if snapshot.project_epoch != request.project_epoch
            || snapshot.project_revision != request.project_revision
        {
            return Err("The project changed; review the transition again".into());
        }
        if matches!(request.decision, Some(TransitionDecision::Cancel)) {
            return Ok(TransitionResult::Cancelled { snapshot });
        }
        let dirty_paths: Vec<_> = lock_unpoisoned(&self.workspace)
            .documents
            .values()
            .filter(|doc| doc.buffer.dirty)
            .map(|doc| doc.buffer.path.clone())
            .collect();
        if !dirty_paths.is_empty() {
            match request.decision {
                None if !snapshot.settings.autosave_project_edits => {
                    return Ok(TransitionResult::NeedsDecision {
                        snapshot,
                        dirty_paths,
                    });
                }
                Some(TransitionDecision::Discard) => {}
                _ => {
                    let saved = self.write_working_sources();
                    self.update_snapshot(|_| {});
                    saved?;
                }
            }
        }
        let close_application = matches!(request.transition, WorkspaceTransition::CloseApplication);
        let replaces_project = matches!(
            request.transition,
            WorkspaceTransition::OpenProject { .. }
                | WorkspaceTransition::CreateProject { .. }
                | WorkspaceTransition::CopyProject { .. }
                | WorkspaceTransition::ReloadProject
        );
        let previous_epoch = snapshot.project_epoch;
        let snapshot = match request.transition {
            WorkspaceTransition::CloseFile { path } => {
                if matches!(request.decision, Some(TransitionDecision::Discard)) {
                    self.discard_documents(&dirty_paths)?;
                }
                self.close_file_path(&path)
            }
            WorkspaceTransition::ReloadFile { path } => {
                let mut paths = if matches!(request.decision, Some(TransitionDecision::Discard)) {
                    dirty_paths
                } else {
                    Vec::new()
                };
                if !paths.contains(&path) {
                    paths.push(path.clone());
                }
                self.discard_documents(&paths)?;
                self.open_file_path(&path)
            }
            WorkspaceTransition::ReloadProject => {
                let root = self.project_root_path().ok_or("No project is open")?;
                self.load_working_copy(&root)
            }
            WorkspaceTransition::OpenProject { path } => self.open_project_path(&path),
            WorkspaceTransition::CreateProject {
                parent_path,
                directory_name,
            } => self.create_new_project_locked(&parent_path, &directory_name),
            WorkspaceTransition::CopyProject {
                parent_path,
                directory_name,
            } => self.copy_project_locked(
                &parent_path,
                &directory_name,
                matches!(request.decision, Some(TransitionDecision::Discard)),
            ),
            WorkspaceTransition::CloseApplication => {
                self.persistence.record_snapshot(&self.snapshot())?;
                let mut workspace = lock_unpoisoned(&self.workspace);
                workspace.close_authorization = Some((
                    workspace.view.project_epoch,
                    workspace.view.project_revision,
                ));
                drop(workspace);
                self.snapshot()
            }
        };
        if replaces_project && snapshot.project_epoch == previous_epoch {
            return Err(snapshot.status);
        }
        Ok(TransitionResult::Applied {
            snapshot,
            close_application,
        })
    }

    pub(crate) fn finish_close(
        &self,
        epoch: u32,
        revision: u32,
        close: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let _authoring = self.settled_authoring();
        {
            let mut workspace = lock_unpoisoned(&self.workspace);
            if workspace.close_authorization.take() != Some((epoch, revision))
                || workspace.view.project_epoch != epoch
                || workspace.view.project_revision != revision
            {
                return Err(
                    "The project changed before closing; close again to review the latest edits"
                        .into(),
                );
            }
        }
        self.persistence.record_snapshot(&self.snapshot())?;
        close()
    }

    fn discard_documents(&self, paths: &[String]) -> Result<(), String> {
        let root = self.project_root_path().ok_or("No project is open")?;
        let documents = paths
            .iter()
            .map(|path| {
                let absolute = super::absolute_root_path(&root, Utf8Path::new(path))
                    .ok_or("Invalid file path")?;
                let disk = super::working_copy::read_disk(&absolute)?;
                let text = disk
                    .as_ref()
                    .map(|bytes| {
                        String::from_utf8(bytes.clone()).map_err(|error| error.to_string())
                    })
                    .transpose()?;
                Ok((path, disk, text))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut workspace = lock_unpoisoned(&self.workspace);
        for (path, disk, text) in documents {
            if let Some(text) = text {
                if let Some(doc) = workspace.documents.get_mut(Utf8Path::new(path)) {
                    doc.observed = disk;
                    doc.buffer.external_state = BufferExternalState::Current;
                    doc.edit(text);
                    doc.buffer.dirty = false;
                    doc.buffer.saved_revision = doc.buffer.document_revision;
                    doc.buffer.save_state = DocumentSaveState::Saved;
                }
            } else {
                workspace.documents.remove(Utf8Path::new(path));
                workspace.tabs.retain(|item| item.as_str() != path);
                if workspace.view.active_file.as_deref() == Some(path.as_str()) {
                    workspace.view.active_file = workspace.tabs.first().map(ToString::to_string);
                }
            }
        }
        workspace.view.project_revision += 1;
        workspace.view.state_revision += 1;
        workspace.typed_revision = None;
        workspace.project = LoadedProject::Checking;
        workspace.view.project_health = ProjectHealth::Checking;
        workspace.view.active_document_descriptor = None;
        drop(workspace);
        lock_unpoisoned(&self.gui_history).clear();
        self.invalidate_prepared_project();
        self.schedule_working_copy(false);
        Ok(())
    }

    pub(super) fn start_filesystem_watcher(&self, root: &Utf8Path) {
        let weak = self.weak.clone();
        let epoch = self.snapshot().project_epoch;
        let watcher = crate::state_tasks::watch_project(root, move |event| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let state = DesktopState(inner);
            state.external_reconcile.schedule((epoch, event));
        });
        match watcher {
            Ok(watcher) => *lock_unpoisoned(&self.watcher) = Some(watcher),
            Err(error) => {
                self.snapshot_with_error("project.watch", root.as_str(), &error);
            }
        }
    }
}

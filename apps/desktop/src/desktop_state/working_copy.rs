use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use camino::{Utf8Path, Utf8PathBuf};
use donder_project_io::{ProjectCheckReport, ProjectSession, SourceTextWrite};

use super::workspace_state::WorkingDocument;
use super::{DesktopState, LoadedProject, absolute_root_path, lock_unpoisoned};
use crate::dto::*;
use crate::state_tasks::WorkingCopyPayload;

impl WorkingDocument {
    pub(super) fn new(path: &Utf8Path, text: String) -> Self {
        Self {
            observed: Some(text.as_bytes().to_vec()),
            buffer: EditorBuffer {
                path: path.to_string(),
                name: path.file_name().unwrap_or(path.as_str()).to_string(),
                syntax: donder_project_io::source_document_format(path).into(),
                text,
                dirty: false,
                read_only: false,
                document_revision: 0,
                saved_revision: 0,
                save_state: DocumentSaveState::Saved,
                external_state: BufferExternalState::Current,
            },
        }
    }

    pub(super) fn edit(&mut self, text: String) {
        if self.buffer.text == text {
            return;
        }
        self.buffer.document_revision += 1;
        self.buffer.text = text;
        self.buffer.dirty = self.observed.as_deref() != Some(self.buffer.text.as_bytes());
        if !self.buffer.dirty {
            self.buffer.saved_revision = self.buffer.document_revision;
        }
        self.buffer.save_state = if self.buffer.external_state != BufferExternalState::Current {
            DocumentSaveState::Conflict
        } else if self.buffer.dirty {
            DocumentSaveState::Dirty
        } else {
            DocumentSaveState::Saved
        };
    }
}

impl DesktopState {
    pub(super) fn settled_authoring(&self) -> std::sync::MutexGuard<'_, ()> {
        loop {
            self.working_copy.finish_pending();
            let guard = lock_unpoisoned(&self.authoring);
            if self.working_copy.is_idle() {
                return guard;
            }
            drop(guard);
        }
    }

    pub fn update_document(&self, update: DocumentUpdate) -> Result<AppSnapshot, String> {
        let _authoring = lock_unpoisoned(&self.authoring);
        {
            let mut workspace = lock_unpoisoned(&self.workspace);
            if workspace.view.project_epoch != update.project_epoch {
                return Err("The project changed before the text was received".into());
            }
            let document = workspace
                .documents
                .get_mut(Utf8Path::new(&update.path))
                .ok_or("Document is not open")?;
            if document.buffer.document_revision != update.expected_document_revision {
                return Err("The document changed before the text was received".into());
            }
            if document.buffer.read_only {
                return Err(
                    "Dependency sources are read-only. Create an independent copy to edit them."
                        .into(),
                );
            }
            if document.buffer.text == update.text {
                return Ok(workspace.snapshot());
            }
            document.edit(update.text);
            workspace.view.state_revision += 1;
            workspace.view.project_revision += 1;
            workspace.typed_revision = None;
            workspace.project = LoadedProject::Checking;
            workspace.view.project_health = ProjectHealth::Checking;
            workspace.view.active_document_descriptor = None;
        }
        lock_unpoisoned(&self.gui_history).clear();
        self.invalidate_prepared_project();
        self.schedule_working_copy(false);
        Ok(self.update_snapshot(|snapshot| snapshot.status = "Checking project".into()))
    }

    pub(super) fn invalidate_prepared_project(&self) {
        self.render_refresh.invalidate_pending();
        self.suspend_live_output();
        lock_unpoisoned(&self.sequence_render).unload();
        *lock_unpoisoned(&self.sequence_clip_raster) =
            crate::sequence_clip_raster::SequenceClipRasterService::new();
    }

    pub(super) fn schedule_working_copy(&self, save: bool) {
        let request = {
            let workspace = lock_unpoisoned(&self.workspace);
            let Some(root) = workspace.view.project_root.as_ref() else {
                return;
            };
            WorkingCopyPayload {
                root: Utf8PathBuf::from(root),
                epoch: workspace.view.project_epoch,
                revision: workspace.view.project_revision,
                sources: workspace
                    .documents
                    .iter()
                    .filter(|(_, doc)| !doc.buffer.read_only)
                    .map(|(path, doc)| (path.clone(), doc.buffer.text.clone()))
                    .collect(),
                typed: match &workspace.project {
                    LoadedProject::Ready(session) => Some(Arc::clone(session)),
                    _ => None,
                },
                save,
                autosave_generation: self.autosave_generation.load(Ordering::Acquire),
            }
        };
        self.working_copy.schedule(request);
    }

    pub(super) fn complete_working_copy(
        &self,
        request: WorkingCopyPayload,
        report: Option<ProjectCheckReport>,
    ) {
        let _authoring = lock_unpoisoned(&self.authoring);
        let current = self.snapshot();
        if current.project_epoch != request.epoch || current.project_revision != request.revision {
            return;
        }
        if let Some(report) = report {
            self.apply_analysis(report);
        }
        if request.save
            || (current.settings.autosave_project_edits
                && request.autosave_generation == self.autosave_generation.load(Ordering::Acquire))
        {
            let _ = self.write_working_sources();
        }
        self.update_snapshot(|_| {});
    }

    /// The caller owns the authoring gate. An accepted revision cannot change
    /// between disk preconditions, the write, and the saved acknowledgement.
    pub(super) fn write_working_sources(&self) -> Result<(), String> {
        let _filesystem = lock_unpoisoned(&self.filesystem);
        let (root, writes) = {
            let mut workspace = lock_unpoisoned(&self.workspace);
            let root = Utf8PathBuf::from(
                workspace
                    .view
                    .project_root
                    .as_ref()
                    .ok_or("No project is open")?,
            );
            let mut writes = BTreeMap::new();
            for (path, document) in &mut workspace.documents {
                if !document.buffer.dirty {
                    continue;
                }
                if document.buffer.read_only {
                    return Err(format!("Dependency source {path} is read-only"));
                }
                let actual = match read_disk(&root.join(path)) {
                    Ok(actual) => actual,
                    Err(message) => {
                        document.buffer.save_state = DocumentSaveState::Failed {
                            message: message.clone(),
                        };
                        return Err(message);
                    }
                };
                if actual != document.observed
                    || document.buffer.external_state != BufferExternalState::Current
                {
                    document.buffer.external_state = if actual.is_some() {
                        BufferExternalState::ChangedOnDisk
                    } else {
                        BufferExternalState::DeletedOnDisk
                    };
                    document.observed = actual;
                    document.buffer.save_state = DocumentSaveState::Conflict;
                    let message = format!("Resolve the external conflict in {path} before saving");
                    workspace.typed_revision = None;
                    workspace.project = LoadedProject::Invalid;
                    workspace.view.project_health = ProjectHealth::Invalid;
                    workspace.view.active_document_descriptor = None;
                    workspace.view.settings.editor_view_mode = EditorViewMode::Text;
                    workspace.view.workspace_layout.active_sidebar_view = SidebarView::Problems;
                    workspace.view.workspace_layout.sidebar_collapsed = false;
                    drop(workspace);
                    self.invalidate_prepared_project();
                    self.update_snapshot(|snapshot| snapshot.status.clone_from(&message));
                    return Err(message);
                }
                writes.insert(
                    path.clone(),
                    SourceTextWrite {
                        text: document.buffer.text.clone(),
                        expected: document.observed.clone(),
                    },
                );
            }
            for path in writes.keys() {
                if let Some(document) = workspace.documents.get_mut(path) {
                    document.buffer.save_state = DocumentSaveState::Saving;
                }
            }
            (root, writes)
        };
        let result = donder_project_io::write_source_texts(&root, &writes)
            .map_err(|error| error.to_string());
        let mut workspace = lock_unpoisoned(&self.workspace);
        for (path, write) in writes {
            let Some(document) = workspace.documents.get_mut(&path) else {
                continue;
            };
            match &result {
                Ok(_) => {
                    document.observed = Some(write.text.into_bytes());
                    document.buffer.saved_revision = document.buffer.document_revision;
                    document.buffer.dirty = false;
                    document.buffer.save_state = DocumentSaveState::Saved;
                    document.buffer.external_state = BufferExternalState::Current;
                }
                Err(message) => {
                    document.buffer.save_state = DocumentSaveState::Failed {
                        message: message.clone(),
                    }
                }
            }
        }
        if let Err(message) = &result {
            workspace.view.status = format!("Save failed: {message}");
        }
        result.map(|_| ())
    }

    pub fn save_all(&self) -> Result<AppSnapshot, String> {
        let _authoring = self.settled_authoring();
        let result = self.write_working_sources();
        let snapshot = self.update_snapshot(|snapshot| {
            snapshot.status = match &result {
                Ok(()) => "Saved all files".into(),
                Err(error) => format!("Save failed: {error}"),
            };
        });
        result.map(|_| snapshot)
    }

    pub(super) fn accept_gui_sources(
        &self,
        session: Arc<ProjectSession>,
        texts: BTreeMap<String, String>,
        status: &str,
    ) -> Result<AppSnapshot, String> {
        let mut new_documents = BTreeMap::new();
        {
            let workspace = lock_unpoisoned(&self.workspace);
            for path in texts.keys() {
                let path = Utf8PathBuf::from(path);
                if !workspace.documents.contains_key(&path) {
                    let mut document = WorkingDocument::new(&path, String::new());
                    document.observed = read_disk(&session.source.project_root().join(&path))?;
                    new_documents.insert(path, document);
                }
            }
        }
        {
            let mut workspace = lock_unpoisoned(&self.workspace);
            workspace.documents.extend(new_documents);
            for (path, text) in texts {
                let path = Utf8PathBuf::from(path);
                if let Some(document) = workspace.documents.get_mut(&path) {
                    document.edit(text);
                }
            }
            workspace.view.project_revision += 1;
            workspace.typed_revision = Some(workspace.view.project_revision);
            workspace.view.state_revision += 1;
            workspace.project = LoadedProject::Ready(Arc::clone(&session));
            workspace.view.project_health = ProjectHealth::Ready;
        }
        let entries = super::workspace_entries(&session);
        let descriptor = self
            .snapshot()
            .active_file
            .as_deref()
            .and_then(|path| super::descriptor_for_path(&session, Utf8Path::new(path)));
        self.invalidate_prepared_project();
        self.schedule_render_refresh(session);
        self.schedule_working_copy(false);
        Ok(self.update_snapshot(|snapshot| {
            snapshot.project_entries = entries;
            snapshot.active_document_descriptor = descriptor;
            snapshot.status = status.into();
        }))
    }

    pub fn reconcile_external_files(&self) -> Result<AppSnapshot, String> {
        let _authoring = lock_unpoisoned(&self.authoring);
        self.reconcile_external_files_locked()
    }

    pub(super) fn reconcile_external_files_locked(&self) -> Result<AppSnapshot, String> {
        let Some(root) = self.project_root_path() else {
            return Ok(self.snapshot());
        };
        let disk_sources =
            donder_project_io::project_source_texts(&root).map_err(|error| error.to_string())?;
        let mut changed = false;
        {
            let mut workspace = lock_unpoisoned(&self.workspace);
            let mut deleted_clean = Vec::new();
            // Validate every observation before changing any working document.
            let observations = workspace
                .documents
                .iter()
                .map(|(path, document)| {
                    let actual = read_disk(&root.join(path))?;
                    let text = if !document.buffer.dirty && actual != document.observed {
                        actual
                            .as_ref()
                            .map(|bytes| {
                                String::from_utf8(bytes.clone()).map_err(|error| error.to_string())
                            })
                            .transpose()?
                    } else {
                        None
                    };
                    Ok((path.clone(), actual, text))
                })
                .collect::<Result<Vec<_>, String>>()?;
            for (path, actual, text) in observations {
                let Some(document) = workspace.documents.get_mut(&path) else {
                    continue;
                };
                if actual == document.observed {
                    continue;
                }
                if actual.as_deref() == Some(document.buffer.text.as_bytes()) {
                    document.observed = actual;
                    document.buffer.dirty = false;
                    document.buffer.saved_revision = document.buffer.document_revision;
                    document.buffer.save_state = DocumentSaveState::Saved;
                    document.buffer.external_state = BufferExternalState::Current;
                } else if document.buffer.dirty {
                    document.buffer.external_state = if actual.is_some() {
                        BufferExternalState::ChangedOnDisk
                    } else {
                        BufferExternalState::DeletedOnDisk
                    };
                    document.observed = actual;
                    document.buffer.save_state = DocumentSaveState::Conflict;
                } else {
                    document.observed = actual.clone();
                    if let Some(text) = text {
                        document.buffer.external_state = BufferExternalState::Current;
                        document.edit(text);
                    } else {
                        deleted_clean.push(path.clone());
                    }
                }
                changed = true;
            }
            for path in deleted_clean {
                workspace.documents.remove(&path);
                workspace.tabs.retain(|tab| tab != &path);
                if workspace.view.active_file.as_deref() == Some(path.as_str()) {
                    workspace.view.active_file = workspace.tabs.first().map(ToString::to_string);
                }
            }
            for (path, text) in disk_sources {
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    workspace.documents.entry(path.clone())
                {
                    entry.insert(WorkingDocument::new(&path, text));
                    changed = true;
                }
            }
            if changed {
                workspace.view.state_revision += 1;
                workspace.view.project_revision += 1;
                workspace.typed_revision = None;
                workspace.project = LoadedProject::Checking;
                workspace.view.project_health = ProjectHealth::Checking;
                workspace.view.active_document_descriptor = None;
            }
        }
        if changed {
            lock_unpoisoned(&self.gui_history).clear();
            self.invalidate_prepared_project();
            self.schedule_working_copy(false);
            Ok(self.update_snapshot(|snapshot| {
                snapshot.status = "Project files changed; checking project".into()
            }))
        } else {
            Ok(self.snapshot())
        }
    }

    pub fn resolve_external_conflict(
        &self,
        epoch: u32,
        path: String,
        revision: u32,
        decision: ExternalConflictDecision,
    ) -> Result<AppSnapshot, String> {
        let _authoring = self.settled_authoring();
        let mut workspace = lock_unpoisoned(&self.workspace);
        if workspace.view.project_epoch != epoch {
            return Err("The project changed".into());
        }
        let root = Utf8PathBuf::from(
            workspace
                .view
                .project_root
                .as_ref()
                .ok_or("No project is open")?,
        );
        let absolute =
            absolute_root_path(&root, Utf8Path::new(&path)).ok_or("Invalid source path")?;
        let document = workspace
            .documents
            .get_mut(Utf8Path::new(&path))
            .ok_or("Document is not open")?;
        if document.buffer.document_revision != revision {
            return Err("The document changed".into());
        }
        let disk = read_disk(&absolute)?;
        match decision {
            ExternalConflictDecision::Reload => {
                let bytes =
                    disk.ok_or("The file was deleted. Keep your working copy to recreate it.")?;
                let text = String::from_utf8(bytes.clone()).map_err(|error| error.to_string())?;
                document.observed = Some(bytes);
                document.edit(text);
                document.buffer.dirty = false;
                document.buffer.saved_revision = document.buffer.document_revision;
                document.buffer.save_state = DocumentSaveState::Saved;
            }
            ExternalConflictDecision::KeepWorkingCopy => {
                document.observed = disk;
                document.buffer.dirty =
                    document.observed.as_deref() != Some(document.buffer.text.as_bytes());
                document.buffer.save_state = if document.buffer.dirty {
                    DocumentSaveState::Dirty
                } else {
                    DocumentSaveState::Saved
                };
            }
        }
        document.buffer.external_state = BufferExternalState::Current;
        workspace.view.state_revision += 1;
        workspace.view.project_revision += 1;
        workspace.typed_revision = None;
        workspace.project = LoadedProject::Checking;
        workspace.view.project_health = ProjectHealth::Checking;
        drop(workspace);
        self.invalidate_prepared_project();
        lock_unpoisoned(&self.gui_history).clear();
        self.schedule_working_copy(false);
        Ok(self.update_snapshot(|_| {}))
    }
}

pub(super) fn read_disk(path: &Utf8Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{path}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_foundation_tests::tests::starter_copy;
    const SEQUENCE: &str = "sequences/layer_test.sequence.donder";

    fn project() -> (tempfile::TempDir, Utf8PathBuf, DesktopState) {
        let (temporary, root) = starter_copy();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        *lock_unpoisoned(&state.watcher) = None;
        let mut settings = state.snapshot().settings;
        settings.autosave_project_edits = false;
        state.update_app_settings(settings);
        state.open_file_path(SEQUENCE);
        (temporary, root, state)
    }

    fn edit(state: &DesktopState, text: String) -> AppSnapshot {
        let snapshot = state.snapshot();
        let buffer = snapshot
            .tabs
            .iter()
            .find(|buffer| buffer.path == SEQUENCE)
            .unwrap();
        state
            .update_document(DocumentUpdate {
                project_epoch: snapshot.project_epoch,
                path: SEQUENCE.into(),
                expected_document_revision: buffer.document_revision,
                text,
            })
            .unwrap()
    }

    fn gui_request(state: &DesktopState) -> GuiDocumentRequest {
        GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: SEQUENCE.into(),
            view: DocumentViewId::Sequence,
            object_key: Some("layer_test".into()),
        }
    }

    fn transition(
        state: &DesktopState,
        action: WorkspaceTransition,
        decision: Option<TransitionDecision>,
    ) -> Result<TransitionResult, String> {
        let snapshot = state.snapshot();
        state.request_transition(TransitionRequest {
            transition: action,
            project_epoch: snapshot.project_epoch,
            project_revision: snapshot.project_revision,
            decision,
        })
    }

    #[test]
    fn unsaved_text_is_the_gui_baseline_and_gui_edits_share_autosave() {
        let (_temporary, root, state) = project();
        let original = std::fs::read_to_string(root.join(SEQUENCE)).unwrap();
        edit(
            &state,
            original.replace("frame_rate: 144", "frame_rate: 90"),
        );
        state.set_editor_view_mode(EditorViewMode::Gui);
        assert_eq!(state.snapshot().project_health, ProjectHealth::Ready);
        assert_eq!(
            std::fs::read_to_string(root.join(SEQUENCE)).unwrap(),
            original
        );
        let request = gui_request(&state);
        let GuiDocument::Sequence { document } = state.get_gui_document(request.clone()).document
        else {
            panic!("GUI unavailable");
        };
        assert_eq!(document.frame_rate, 90.0);
        let result = state.apply_gui_edit(
            request,
            GuiEditCommand::Sequence {
                edit: SequenceGuiEdit::SetDuration {
                    duration_seconds: 300.0,
                },
            },
        );
        assert!(!matches!(result.document, GuiDocument::Blocked { .. }));
        let projection = result.snapshot.gui_projection.as_ref().unwrap();
        assert_eq!(
            projection.project_revision,
            result.snapshot.project_revision
        );
        assert_eq!(
            projection.request.project_revision,
            result.snapshot.project_revision
        );
        let GuiDocument::Sequence { document } = &projection.document else {
            panic!("Edit snapshot omitted the sequence projection");
        };
        assert_eq!(document.duration_seconds, 300.0);
        state.working_copy.finish_pending();
        assert_eq!(
            state.snapshot().gui_projection.unwrap().project_revision,
            result.snapshot.project_revision
        );
        assert!(state.snapshot().active_buffer.unwrap().dirty);
        assert_eq!(
            std::fs::read_to_string(root.join(SEQUENCE)).unwrap(),
            original
        );
        let mut settings = state.snapshot().settings;
        settings.autosave_project_edits = true;
        state.update_app_settings(settings);
        state.working_copy.finish_pending();
        let buffer = state.snapshot().active_buffer.unwrap();
        assert!(!buffer.dirty);
        assert_eq!(buffer.document_revision, buffer.saved_revision);
        assert_eq!(
            std::fs::read_to_string(root.join(SEQUENCE)).unwrap(),
            buffer.text
        );
    }

    #[test]
    fn invalid_source_saves_raw_text_and_never_projects_a_previous_gui() {
        let (_temporary, root, state) = project();
        let previous = gui_request(&state);
        let mut settings = state.snapshot().settings;
        settings.autosave_project_edits = true;
        state.update_app_settings(settings);
        let invalid = "layer_test: [".to_string();
        edit(&state, invalid.clone());
        let snapshot = state.set_editor_view_mode(EditorViewMode::Gui);
        assert_eq!(snapshot.project_health, ProjectHealth::Invalid);
        assert!(matches!(
            snapshot.settings.editor_view_mode,
            EditorViewMode::Text
        ));
        assert!(matches!(
            snapshot.workspace_layout.active_sidebar_view,
            SidebarView::Problems
        ));
        assert!(state.project_session().is_none());
        assert!(matches!(
            state.get_gui_document(previous).document,
            GuiDocument::Blocked { .. }
        ));
        assert!(
            lock_unpoisoned(&state.sequence_render)
                .active_target()
                .is_none()
        );
        assert_eq!(
            std::fs::read_to_string(root.join(SEQUENCE)).unwrap(),
            invalid
        );
        assert!(
            !snapshot
                .tabs
                .iter()
                .find(|tab| tab.path == SEQUENCE)
                .unwrap()
                .dirty
        );
    }

    #[test]
    fn new_gui_documents_remain_unsaved_and_are_available_to_text_analysis() {
        let (_temporary, root, state) = project();
        let path = "sequences/unsaved.sequence.donder";
        state.create_sequence(NewSequenceRequest {
            file_path: path.into(),
            object_key: "new_sequence".into(),
            duration_seconds: 60.0,
            frame_rate: 60,
        });
        state.working_copy.finish_pending();
        let snapshot = state.snapshot();
        assert_eq!(snapshot.project_health, ProjectHealth::Ready);
        assert!(
            snapshot
                .project_entries
                .iter()
                .any(|entry| entry.path == path)
        );
        assert!(!root.join(path).exists());
        let buffer = snapshot.active_buffer.unwrap();
        assert_eq!(buffer.path, path);
        state
            .update_document(DocumentUpdate {
                project_epoch: snapshot.project_epoch,
                path: path.into(),
                expected_document_revision: buffer.document_revision,
                text: format!("{}\n# still unsaved\n", buffer.text),
            })
            .unwrap();
        state.working_copy.finish_pending();
        assert_eq!(
            state.snapshot().project_health,
            ProjectHealth::Ready,
            "{:?}",
            state.snapshot().diagnostics
        );
        assert!(!root.join(path).exists());
        state.save_all().unwrap();
        assert!(donder_project_io::check_package(&root).session.is_some());
    }

    #[test]
    fn source_and_epoch_guards_reject_old_commands_and_completion_results() {
        let (_temporary, root, state) = project();
        let initial = state.snapshot();
        let request = gui_request(&state);
        let old = WorkingCopyPayload {
            root: root.clone(),
            epoch: initial.project_epoch,
            revision: initial.project_revision,
            sources: donder_project_io::project_source_texts(&root).unwrap(),
            typed: None,
            save: true,
            autosave_generation: 0,
        };
        let report = crate::state_tasks::analyze_working_copy(&old);
        edit(
            &state,
            format!(
                "{}\n# newest\n",
                initial.active_buffer.as_ref().unwrap().text
            ),
        );
        state.working_copy.finish_pending();
        state.complete_working_copy(old, report);
        assert!(state.snapshot().active_buffer.as_ref().unwrap().dirty);
        assert_eq!(
            state.snapshot().project_revision,
            initial.project_revision + 1
        );
        let error = state.update_document(DocumentUpdate {
            project_epoch: initial.project_epoch,
            path: SEQUENCE.into(),
            expected_document_revision: 0,
            text: "stale".into(),
        });
        assert!(error.is_err());
        let before = state.snapshot().state_revision;
        assert!(matches!(
            state
                .apply_gui_edit(
                    request,
                    GuiEditCommand::Sequence {
                        edit: SequenceGuiEdit::SetDuration {
                            duration_seconds: 100.0
                        }
                    }
                )
                .document,
            GuiDocument::Blocked { .. }
        ));
        assert_eq!(state.snapshot().state_revision, before);
        state.open_project_path(root.as_str());
        assert!(
            state
                .update_document(DocumentUpdate {
                    project_epoch: initial.project_epoch,
                    path: SEQUENCE.into(),
                    expected_document_revision: 0,
                    text: "old project".into()
                })
                .is_err()
        );
    }

    #[test]
    fn navigation_and_save_all_preserve_latest_text() {
        let (_temporary, root, state) = project();
        let text = format!(
            "{}\n# latest before navigation\n",
            state.snapshot().active_buffer.unwrap().text
        );
        edit(&state, text.clone());
        state.open_file_path(donder_package::MANIFEST_FILE);
        state.set_active_file_path(SEQUENCE);
        assert_eq!(state.snapshot().active_buffer.unwrap().text, text);
        let snapshot = state.save_all().unwrap();
        assert_eq!(std::fs::read_to_string(root.join(SEQUENCE)).unwrap(), text);
        assert!(!snapshot.active_buffer.unwrap().dirty);
    }

    #[test]
    fn clean_external_changes_reload_and_dirty_changes_and_deletions_conflict() {
        let (_temporary, root, state) = project();
        let original = state.snapshot().active_buffer.unwrap().text;
        let external = format!("{original}\n# external\n");
        std::fs::write(root.join(SEQUENCE), &external).unwrap();
        state.reconcile_external_files().unwrap();
        state.working_copy.finish_pending();
        assert_eq!(state.snapshot().active_buffer.unwrap().text, external);
        let mine = format!("{original}\n# mine\n");
        edit(&state, mine.clone());
        state.working_copy.finish_pending();
        std::fs::write(root.join(SEQUENCE), &original).unwrap();
        // No watcher event: the save boundary must still detect the conflict.
        assert!(state.save_all().is_err());
        let snapshot = state.snapshot();
        assert_eq!(
            snapshot.active_buffer.as_ref().unwrap().external_state,
            BufferExternalState::ChangedOnDisk
        );
        assert_eq!(snapshot.active_buffer.unwrap().text, mine);
        std::fs::remove_file(root.join(SEQUENCE)).unwrap();
        state.reconcile_external_files().unwrap();
        assert_eq!(
            state.snapshot().active_buffer.unwrap().external_state,
            BufferExternalState::DeletedOnDisk
        );
        let snapshot = state.snapshot();
        state
            .resolve_external_conflict(
                snapshot.project_epoch,
                SEQUENCE.into(),
                snapshot.active_buffer.unwrap().document_revision,
                ExternalConflictDecision::KeepWorkingCopy,
            )
            .unwrap();
        state.save_all().unwrap();
        assert_eq!(std::fs::read_to_string(root.join(SEQUENCE)).unwrap(), mine);
    }

    #[test]
    fn every_destructive_transition_requires_a_decision_and_cancel_preserves_text() {
        let (_temporary, root, state) = project();
        let text = format!(
            "{}\n# must survive cancellation\n",
            state.snapshot().active_buffer.unwrap().text
        );
        edit(&state, text.clone());
        for action in [
            WorkspaceTransition::CloseFile {
                path: SEQUENCE.into(),
            },
            WorkspaceTransition::ReloadFile {
                path: SEQUENCE.into(),
            },
            WorkspaceTransition::ReloadProject,
            WorkspaceTransition::OpenProject {
                path: root.to_string(),
            },
            WorkspaceTransition::CloseApplication,
        ] {
            assert!(matches!(
                transition(&state, action.clone(), None).unwrap(),
                TransitionResult::NeedsDecision { .. }
            ));
            assert!(matches!(
                transition(&state, action, Some(TransitionDecision::Cancel)).unwrap(),
                TransitionResult::Cancelled { .. }
            ));
            assert_eq!(state.snapshot().active_buffer.unwrap().text, text);
        }
        assert!(matches!(
            transition(
                &state,
                WorkspaceTransition::CloseFile {
                    path: SEQUENCE.into()
                },
                Some(TransitionDecision::SaveAll)
            )
            .unwrap(),
            TransitionResult::Applied { .. }
        ));
        assert_eq!(std::fs::read_to_string(root.join(SEQUENCE)).unwrap(), text);
        state.open_file_path(SEQUENCE);
        edit(&state, format!("{text}\n# discard me\n"));
        transition(
            &state,
            WorkspaceTransition::ReloadFile {
                path: SEQUENCE.into(),
            },
            Some(TransitionDecision::Discard),
        )
        .unwrap();
        assert_eq!(state.snapshot().active_buffer.unwrap().text, text);
    }

    #[test]
    fn stale_render_completion_cannot_restore_output_after_invalid_text() {
        let (_temporary, _root, state) = project();
        let project = state.project_session().unwrap();
        let sequence_id = state.resolve_sequence_id(&gui_request(&state)).unwrap();
        let snapshot = state.snapshot();
        let request = crate::state_tasks::RenderRefreshPayload {
            project_epoch: snapshot.project_epoch,
            project_revision: snapshot.project_revision,
            setup_id: project.project.root.setup.clone(),
            sequence_id,
            project,
        };
        let result = crate::rendering::prepare_sequence_output(
            &request.project.project,
            &request.setup_id,
            &request.sequence_id,
        );
        assert!(result.is_ok());
        edit(&state, "invalid: [".into());
        state.working_copy.finish_pending();
        state.complete_render_refresh(request, result);
        assert_eq!(state.snapshot().project_health, ProjectHealth::Invalid);
        assert!(
            lock_unpoisoned(&state.sequence_render)
                .active_target()
                .is_none()
        );
    }

    #[test]
    fn close_requires_the_exact_reviewed_revision_and_preserves_later_edits() {
        let (_temporary, _root, state) = project();
        let initial = state.snapshot();
        assert!(
            state
                .finish_close(initial.project_epoch, initial.project_revision, || Ok(()))
                .is_err()
        );
        transition(&state, WorkspaceTransition::CloseApplication, None).unwrap();
        edit(
            &state,
            format!(
                "{}\n# after close request\n",
                initial.active_buffer.unwrap().text
            ),
        );
        assert!(
            state
                .finish_close(initial.project_epoch, initial.project_revision, || panic!(
                    "stale close must not destroy the window"
                ))
                .is_err()
        );
        assert!(state.snapshot().active_buffer.unwrap().dirty);
        let result = transition(
            &state,
            WorkspaceTransition::CloseApplication,
            Some(TransitionDecision::SaveAll),
        )
        .unwrap();
        let TransitionResult::Applied { snapshot, .. } = result else {
            panic!("close should be ready")
        };
        state
            .finish_close(snapshot.project_epoch, snapshot.project_revision, || Ok(()))
            .unwrap();
    }

    #[test]
    fn disabling_autosave_cancels_a_captured_save_and_failed_writes_stay_dirty() {
        let (_temporary, root, state) = project();
        let original = state.snapshot().active_buffer.unwrap().text;
        let text = format!("{original}\n# unsaved\n");
        edit(&state, text.clone());
        state.working_copy.finish_pending();
        let request = {
            let _authoring = lock_unpoisoned(&state.authoring);
            let mut settings = state.snapshot().settings;
            settings.autosave_project_edits = true;
            let snapshot = state.update_app_settings_locked(settings.clone());
            let sources = lock_unpoisoned(&state.workspace)
                .documents
                .iter()
                .map(|(path, document)| (path.clone(), document.buffer.text.clone()))
                .collect();
            let request = WorkingCopyPayload {
                root: root.clone(),
                epoch: snapshot.project_epoch,
                revision: snapshot.project_revision,
                sources,
                typed: state.project_session(),
                save: false,
                autosave_generation: state.autosave_generation.load(Ordering::Acquire),
            };
            settings.autosave_project_edits = false;
            state.update_app_settings_locked(settings);
            request
        };
        state.working_copy.finish_pending();
        state.complete_working_copy(request, None);
        assert_eq!(
            std::fs::read_to_string(root.join(SEQUENCE)).unwrap(),
            original
        );
        std::fs::remove_file(root.join(SEQUENCE)).unwrap();
        std::fs::create_dir(root.join(SEQUENCE)).unwrap();
        assert!(state.save_all().is_err());
        let failed = state.snapshot().active_buffer.unwrap();
        assert!(failed.dirty);
        assert!(matches!(
            failed.save_state,
            DocumentSaveState::Failed { .. }
        ));
        assert_ne!(failed.document_revision, failed.saved_revision);
        edit(&state, format!("{text}\n# newer\n"));
        assert!(state.snapshot().active_buffer.unwrap().dirty);
    }

    #[test]
    fn filesystem_notifications_reload_clean_source_without_polling_snapshots() {
        let (_temporary, root) = starter_copy();
        let (sender, receiver) = std::sync::mpsc::channel();
        let state = DesktopState::new(move |snapshot| {
            let _ = sender.send(snapshot);
        });
        state.open_project_path(root.as_str());
        state.open_file_path(SEQUENCE);
        let original = state.snapshot().active_buffer.unwrap().text;
        let updated = format!("{original}\n# written outside Donder\n");
        std::fs::write(root.join(SEQUENCE), &updated).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let snapshot = receiver
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("watcher should publish a complete snapshot");
            if snapshot.project_health == ProjectHealth::Ready
                && snapshot
                    .active_buffer
                    .as_ref()
                    .is_some_and(|buffer| buffer.text == updated)
            {
                assert!(!snapshot.active_buffer.unwrap().dirty);
                break;
            }
        }
        let snapshot = state.snapshot();
        assert_eq!(state.snapshot().state_revision, snapshot.state_revision);
    }

    #[test]
    fn conflict_cancels_save_and_close_without_clearing_dirty_text() {
        let (_temporary, root, state) = project();
        let original = state.snapshot().active_buffer.unwrap().text;
        edit(&state, format!("{original}\n# mine\n"));
        state.working_copy.finish_pending();
        std::fs::write(root.join(SEQUENCE), format!("{original}\n# theirs\n")).unwrap();
        state.reconcile_external_files().unwrap();
        state.working_copy.finish_pending();
        assert!(
            transition(
                &state,
                WorkspaceTransition::CloseApplication,
                Some(TransitionDecision::SaveAll)
            )
            .is_err()
        );
        assert!(state.snapshot().active_buffer.unwrap().dirty);
    }
}

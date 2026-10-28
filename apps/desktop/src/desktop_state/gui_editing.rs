use std::sync::Arc;

use donder_editor::GuiMutationError;
use donder_project_io::ProjectSession;

use super::{DesktopState, generated_source_texts, lock_unpoisoned};
use crate::state_tasks::GuiHistoryEntry;
use donder_editor::dto::{
    AppSnapshot, GuiDocumentChange, GuiDocumentRequest, GuiEditCommand, GuiEditUpdate,
};

impl DesktopState {
    pub fn resolve_gui_source(
        &self,
        path: &str,
        object_key: &str,
    ) -> Result<GuiDocumentRequest, String> {
        let _authoring = self.settled_authoring();
        let session = self.project_session().ok_or("No project is loaded.")?;
        let identity = donder_editor::source_identity_from_gui(path, object_key);
        let object = session
            .source
            .documents
            .get(identity.document_id())
            .and_then(|document| {
                document
                    .objects()
                    .iter()
                    .find(|object| object.name().as_str() == object_key)
            })
            .ok_or("Source object was not found.")?;
        let path = identity.document().to_path_buf();
        let workspace = lock_unpoisoned(&self.workspace);
        // Camino compares equivalent Windows separators, while the frontend
        // uses the buffer's exact path string as its editor identity.
        let path = match workspace.documents.get(&path) {
            Some(document) => document.buffer.path.clone(),
            None => path.to_string(),
        };
        Ok(GuiDocumentRequest {
            owned_path: Vec::new(),
            project_revision: workspace.view.project_revision,
            path,
            object_key: object.name().as_str().to_string(),
        })
    }

    /// The inspector's view of selected clips in the newest project snapshot.
    pub fn get_sequence_clip_details(
        &self,
        request: GuiDocumentRequest,
        clip_ids: Vec<u32>,
    ) -> Result<donder_editor::dto::SequenceClipDetailsResult, String> {
        let project_revision = self.snapshot().project_revision;
        let project = self
            .project_session()
            .ok_or("The current project source is not ready for GUI editing.")?;
        Ok(donder_editor::dto::SequenceClipDetailsResult {
            project_revision,
            details: donder_editor::sequence_clip_details(&project, &request, &clip_ids)?,
        })
    }

    /// "Select Matching" against the newest project snapshot.
    pub fn match_sequence_clips(
        &self,
        request: GuiDocumentRequest,
        query: donder_editor::dto::SequenceClipMatchRequest,
    ) -> Result<donder_editor::dto::SequenceSelection, String> {
        let project = self
            .project_session()
            .ok_or("The current project source is not ready for GUI editing.")?;
        donder_editor::matching_sequence_clips(&project, &request, &query)
    }

    pub fn get_gui_document(
        &self,
        request: GuiDocumentRequest,
    ) -> donder_editor::dto::GuiDocumentResult {
        let _authoring = lock_unpoisoned(&self.authoring);
        let revision = self.snapshot().project_revision;
        let document = if revision != request.project_revision {
            donder_editor::blocked(
                "The project changed. Request the current GUI document.",
                Vec::new(),
            )
        } else if let Some(project) = self.project_session() {
            donder_editor::project_gui_document(Some(&project), &request)
        } else {
            donder_editor::blocked(
                "The current project source is not ready for GUI editing.",
                self.snapshot().diagnostics,
            )
        };
        donder_editor::dto::GuiDocumentResult {
            request,
            project_revision: revision,
            document,
        }
    }

    /// Rasters always follow the newest accepted project snapshot.
    pub fn request_sequence_clip_rasters(
        &self,
        request: donder_editor::dto::SequenceClipRasterRequest,
    ) -> donder_editor::dto::SequenceClipRasterResponse {
        let settings = self.snapshot().settings.clip_raster;
        let project = self.project_session();
        let sequence_id = self.resolve_sequence_id(&request.document);
        lock_unpoisoned(&self.sequence_clip_raster).request(settings, project, sequence_id, request)
    }

    pub fn take_sequence_clip_raster_results(
        &self,
        request: GuiDocumentRequest,
        since_revision: u32,
    ) -> donder_editor::dto::SequenceClipRasterResultBatch {
        lock_unpoisoned(&self.sequence_clip_raster).take_results(request, since_revision)
    }

    pub fn sequence_clip_raster_pixels(&self, token: &str) -> Option<Vec<u8>> {
        lock_unpoisoned(&self.sequence_clip_raster).pixels_rgba_for_token(token)
    }

    /// Apply one GUI edit. The sequence clipboard changes only when the edit is
    /// accepted.
    pub fn edit_gui_document(
        &self,
        request: GuiDocumentRequest,
        edit: GuiEditCommand,
    ) -> GuiEditUpdate {
        let _authoring = lock_unpoisoned(&self.authoring);
        let mut clipboard = lock_unpoisoned(&self.sequence_clipboard);
        let mut candidate = clipboard.clone();
        match self.mutate_gui_project_locked(&request, |session| {
            donder_editor::apply_edit(session, &request, edit, &mut candidate)
        }) {
            Ok((update, outcome)) => {
                *clipboard = candidate;
                with_outcome(update, outcome)
            }
            Err(error) => self.gui_edit_error(error),
        }
    }

    /// [`Self::edit_gui_document`] with the whole edited document.
    #[cfg(test)]
    pub fn apply_gui_edit(
        &self,
        request: GuiDocumentRequest,
        edit: GuiEditCommand,
    ) -> GuiEditResult {
        let update = self.edit_gui_document(request.clone(), edit);
        let document = match update.change {
            GuiDocumentChange::Document { document } => document,
            GuiDocumentChange::SequenceClips { .. } => {
                let request = GuiDocumentRequest {
                    project_revision: update.snapshot.project_revision,
                    ..request
                };
                self.get_gui_document(request).document
            }
        };
        GuiEditResult { document }
    }

    pub(super) fn mutate_gui_project_locked<T>(
        &self,
        request: &GuiDocumentRequest,
        mutate: impl FnOnce(&mut ProjectSession) -> Result<T, GuiMutationError>,
    ) -> Result<(GuiEditUpdate, T), GuiMutationError> {
        if self.snapshot().project_revision != request.project_revision {
            return Err(GuiMutationError::Blocked(
                "The project changed before the GUI edit arrived.".into(),
            ));
        }
        let before = self
            .project_session()
            .ok_or_else(|| GuiMutationError::Blocked("No project is loaded.".to_string()))?;
        let mut affected_paths = donder_editor::affected_paths(&before, request)?;
        let mut edited = (*before).clone();
        let value = mutate(&mut edited).map_err(|error| error.named(&edited))?;
        affected_paths.extend(donder_editor::affected_paths(&edited, request)?);
        let generated_text = generated_source_texts(
            &edited,
            &affected_paths,
            &mut lock_unpoisoned(&self.document_texts),
        )
        .map_err(GuiMutationError::Invalid)?;
        let edited = Arc::new(edited);
        let snapshot = self
            .accept_gui_sources(Arc::clone(&edited), generated_text, "GUI edit applied")
            .map_err(GuiMutationError::Invalid)?;
        let change = donder_editor::project_gui_document_change(&before, &edited, request);
        lock_unpoisoned(&self.gui_history).push_undo(GuiHistoryEntry {
            before,
            after: Arc::clone(&edited),
            affected_paths: affected_paths.clone(),
            status_path: request.path.clone(),
        });
        Ok((gui_update(snapshot, change), value))
    }

    fn gui_edit_error(&self, error: GuiMutationError) -> GuiEditUpdate {
        gui_update(
            self.snapshot(),
            GuiDocumentChange::Document {
                document: donder_editor::blocked(error.message(), Vec::new()),
            },
        )
    }

    pub fn finish_composition_graph_editing(&self) -> AppSnapshot {
        let _authoring = lock_unpoisoned(&self.authoring);
        self.render_refresh.invalidate_pending();
        let render_error = self
            .project_session()
            .and_then(|session| self.refresh_render_session(&session.project));
        self.update_snapshot(|snapshot| {
            snapshot.render_error =
                render_error.map(|error| format!("Render refresh failed: {error:?}"));
        })
    }

    /// Copy a sequence selection to the clipboard; the project does not change.
    pub fn copy_sequence_selection(
        &self,
        request: GuiDocumentRequest,
        selection: donder_editor::dto::SequenceSelection,
    ) -> GuiEditUpdate {
        let _authoring = lock_unpoisoned(&self.authoring);
        let copied = (|| {
            if self.snapshot().project_revision != request.project_revision {
                return Err(GuiMutationError::Blocked(
                    "The project changed before copying".into(),
                ));
            }
            let project = self
                .project_session()
                .ok_or_else(|| GuiMutationError::Blocked("No project is loaded.".to_string()))?;
            let (clipboard, outcome) =
                donder_editor::copy_selection(&project, &request, &selection)?;
            *lock_unpoisoned(&self.sequence_clipboard) = clipboard;
            let update = gui_update(
                self.snapshot(),
                donder_editor::project_gui_document_change(&project, &project, &request),
            );
            Ok(with_outcome(
                update,
                donder_editor::GuiEditOutcome {
                    selection: Some(selection),
                    ..outcome
                },
            ))
        })();
        copied.unwrap_or_else(|error| self.gui_edit_error(error))
    }

    pub fn undo_active_edit(&self) -> AppSnapshot {
        let _authoring = lock_unpoisoned(&self.authoring);
        let Some(entry) = lock_unpoisoned(&self.gui_history).peek_undo() else {
            return self.update_snapshot(|snapshot| {
                snapshot.status = "No GUI edit to undo".to_string();
            });
        };
        let generated_text = match generated_source_texts(
            &entry.before,
            &entry.affected_paths,
            &mut lock_unpoisoned(&self.document_texts),
        ) {
            Ok(text) => text,
            Err(message) => {
                return self.snapshot_with_error("gui.undo", &entry.status_path, &message);
            }
        };
        let entry = {
            let mut history = lock_unpoisoned(&self.gui_history);
            let Some(entry) = history.pop_undo() else {
                return self.snapshot();
            };
            history.push_redo(entry.clone());
            entry
        };
        self.accept_gui_sources(entry.before, generated_text, "GUI edit undone")
            .unwrap_or_else(|error| {
                self.snapshot_with_error("gui.undo", &entry.status_path, &error)
            })
    }

    pub fn redo_active_edit(&self) -> AppSnapshot {
        let _authoring = lock_unpoisoned(&self.authoring);
        let Some(entry) = lock_unpoisoned(&self.gui_history).peek_redo() else {
            return self.update_snapshot(|snapshot| {
                snapshot.status = "No GUI edit to redo".to_string();
            });
        };
        let generated_text = match generated_source_texts(
            &entry.after,
            &entry.affected_paths,
            &mut lock_unpoisoned(&self.document_texts),
        ) {
            Ok(text) => text,
            Err(message) => {
                return self.snapshot_with_error("gui.redo", &entry.status_path, &message);
            }
        };
        let entry = {
            let mut history = lock_unpoisoned(&self.gui_history);
            let Some(entry) = history.pop_redo() else {
                return self.snapshot();
            };
            history.push_undo_from_redo(entry.clone());
            entry
        };
        self.accept_gui_sources(entry.after, generated_text, "GUI edit redone")
            .unwrap_or_else(|error| {
                self.snapshot_with_error("gui.redo", &entry.status_path, &error)
            })
    }
}

/// An edit's update before its outcome is known.
pub(crate) fn gui_update(snapshot: AppSnapshot, change: GuiDocumentChange) -> GuiEditUpdate {
    GuiEditUpdate {
        snapshot,
        change,
        selection: None,
        copied_count: 0,
        skipped_count: 0,
        created: Vec::new(),
    }
}

fn with_outcome(update: GuiEditUpdate, outcome: donder_editor::GuiEditOutcome) -> GuiEditUpdate {
    GuiEditUpdate {
        selection: outcome.selection,
        copied_count: outcome.copied_count,
        skipped_count: outcome.skipped_count,
        created: outcome.created,
        ..update
    }
}

/// A test's view of an edit: the whole edited document.
#[cfg(test)]
#[derive(Debug)]
pub struct GuiEditResult {
    pub document: donder_editor::dto::GuiDocument,
}

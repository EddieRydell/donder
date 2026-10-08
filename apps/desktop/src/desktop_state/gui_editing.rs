use std::sync::Arc;

use donder_editor::GuiMutationError;
use donder_model::SequenceId;
use donder_project_io::ProjectSession;

use super::{DesktopState, generated_source_texts, lock_unpoisoned};
use crate::state_tasks::GuiHistoryEntry;
use donder_sequence_api::{
    AppSnapshot, GuiDocumentRequest, GuiEditCommand, GuiEditResult, SequenceSelectionEdit,
    SequenceSelectionEditResult,
};

impl DesktopState {
    pub fn resolve_gui_source(
        &self,
        module_id: &str,
        path: &str,
        object_key: &str,
    ) -> Result<GuiDocumentRequest, String> {
        let _authoring = self.settled_authoring();
        let session = self.project_session().ok_or("No project is loaded.")?;
        let identity = donder_editor::source_identity_from_gui(module_id, path, object_key)
            .map_err(|error| error.message().to_string())?;
        let object = session
            .source
            .documents
            .get(identity.document_id())
            .and_then(|document| {
                document
                    .objects()
                    .iter()
                    .find(|object| object.id() == object_key)
            })
            .ok_or("Source object was not found.")?;
        let path = donder_editor::editor_path(&session, identity.document_id())
            .ok_or("Source document has no file location.")?;
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
            object_key: Some(object_key.to_string()),
            view: donder_sequence_api::ObjectKind::from(object.kind())
                .document_view()
                .unwrap_or(donder_sequence_api::DocumentViewId::Text),
        })
    }

    pub fn get_gui_document(
        &self,
        request: GuiDocumentRequest,
    ) -> donder_sequence_api::GuiDocumentResult {
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
        donder_sequence_api::GuiDocumentResult {
            request,
            project_revision: revision,
            document,
        }
    }

    pub fn request_sequence_clip_rasters(
        &self,
        request: donder_sequence_api::SequenceClipRasterRequest,
    ) -> donder_sequence_api::SequenceClipRasterResponse {
        let _authoring = lock_unpoisoned(&self.authoring);
        let snapshot = self.snapshot();
        let project_revision = snapshot.project_revision;
        let raster_settings = snapshot.settings.effect_raster;
        let project = (request.document.project_revision == project_revision)
            .then(|| self.project_session())
            .flatten();
        let setup_id = project
            .as_ref()
            .map(|project| project.project.root().setup.id().clone());
        let sequence_id = self.resolve_sequence_id(&request.document);
        lock_unpoisoned(&self.sequence_clip_raster).request(
            project_revision,
            raster_settings,
            project,
            setup_id,
            sequence_id,
            request,
        )
    }

    pub fn take_sequence_clip_raster_results(
        &self,
        request: GuiDocumentRequest,
        request_id: u32,
    ) -> donder_sequence_api::SequenceClipRasterResultBatch {
        let project_revision = self.snapshot().project_revision;
        lock_unpoisoned(&self.sequence_clip_raster).take_results(
            project_revision,
            request,
            request_id,
        )
    }

    pub fn sequence_clip_raster_pixels(&self, token: &str) -> Option<Vec<u8>> {
        lock_unpoisoned(&self.sequence_clip_raster).pixels_rgba_for_token(token)
    }

    pub fn apply_gui_edit(
        &self,
        request: GuiDocumentRequest,
        edit: GuiEditCommand,
    ) -> GuiEditResult {
        match self.mutate_gui_project(&request, |session| {
            donder_editor::apply_edit(session, &request, edit)
        }) {
            Ok((result, ())) => result,
            Err(error) => self.gui_edit_error(&request, error),
        }
    }

    fn mutate_gui_project<T>(
        &self,
        request: &GuiDocumentRequest,
        mutate: impl FnOnce(&mut ProjectSession) -> Result<T, GuiMutationError>,
    ) -> Result<(GuiEditResult, T), GuiMutationError> {
        let _authoring = lock_unpoisoned(&self.authoring);
        self.mutate_gui_project_locked(request, mutate)
    }

    pub(super) fn mutate_gui_project_locked<T>(
        &self,
        request: &GuiDocumentRequest,
        mutate: impl FnOnce(&mut ProjectSession) -> Result<T, GuiMutationError>,
    ) -> Result<(GuiEditResult, T), GuiMutationError> {
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
        let value = mutate(&mut edited)?;
        affected_paths.extend(donder_editor::affected_paths(&edited, request)?);
        let generated_text =
            generated_source_texts(&edited, &affected_paths).map_err(GuiMutationError::Invalid)?;
        let edited = Arc::new(edited);
        let snapshot = self
            .accept_gui_sources(Arc::clone(&edited), generated_text, "GUI edit applied")
            .map_err(GuiMutationError::Invalid)?;
        let document = match &snapshot.gui_projection {
            Some(projection)
                if projection.request.path == request.path
                    && projection.request.view == request.view
                    && projection.request.object_key == request.object_key
                    && projection.request.owned_path == request.owned_path =>
            {
                projection.document.clone()
            }
            _ => donder_editor::project_gui_document(Some(&edited), request),
        };
        lock_unpoisoned(&self.gui_history).push_undo(GuiHistoryEntry {
            before,
            after: Arc::clone(&edited),
            affected_paths: affected_paths.clone(),
            status_path: request.path.clone(),
        });
        Ok((GuiEditResult { snapshot, document }, value))
    }

    fn gui_edit_error(
        &self,
        _request: &GuiDocumentRequest,
        error: GuiMutationError,
    ) -> GuiEditResult {
        GuiEditResult {
            snapshot: self.snapshot(),
            document: donder_editor::blocked(error.message(), Vec::new()),
        }
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

    pub fn apply_sequence_selection_edit(
        &self,
        request: GuiDocumentRequest,
        edit: SequenceSelectionEdit,
    ) -> SequenceSelectionEditResult {
        let outcome = if let SequenceSelectionEdit::Copy { selection } = edit {
            self.copy_gui_selection(&request, selection)
        } else {
            let _authoring = lock_unpoisoned(&self.authoring);
            let mut clipboard = lock_unpoisoned(&self.sequence_clipboard);
            let mut candidate_clipboard = clipboard.clone();
            let result = self.mutate_gui_project_locked(&request, |session| {
                donder_editor::apply_sequence_selection_edit(
                    session,
                    &request,
                    edit,
                    &mut candidate_clipboard,
                )
            });
            if result.is_ok() {
                *clipboard = candidate_clipboard;
            }
            result
        };
        match outcome {
            Ok((result, mutation)) => SequenceSelectionEditResult {
                snapshot: result.snapshot,
                document: result.document,
                selection: mutation.selection,
                copied_count: mutation.copied_count,
                skipped_count: mutation.skipped_count,
            },
            Err(error) => {
                let result = self.gui_edit_error(&request, error);
                SequenceSelectionEditResult {
                    snapshot: result.snapshot,
                    document: result.document,
                    selection: None,
                    copied_count: 0,
                    skipped_count: 0,
                }
            }
        }
    }

    fn copy_gui_selection(
        &self,
        request: &GuiDocumentRequest,
        selection: donder_sequence_api::SequenceSelection,
    ) -> Result<(GuiEditResult, donder_editor::SequenceSelectionMutation), GuiMutationError> {
        let _authoring = lock_unpoisoned(&self.authoring);
        if self.snapshot().project_revision != request.project_revision {
            return Err(GuiMutationError::Blocked(
                "The project changed before copying".into(),
            ));
        }
        if !matches!(request.view, donder_sequence_api::DocumentViewId::Sequence) {
            return Err(GuiMutationError::Invalid(
                "Copy requires a sequence GUI document.".to_string(),
            ));
        }
        let snapshot = self.snapshot();
        let project = self
            .project_session()
            .ok_or_else(|| GuiMutationError::Blocked("No project is loaded.".to_string()))?;
        let resolved =
            donder_editor::resolve_request(&project, request).map_err(GuiMutationError::Invalid)?;
        donder_editor::ensure_owned_gui_document(&project, &resolved)?;
        let (clipboard, copied_count, skipped_count) = donder_editor::copy_sequence_selection(
            &project,
            &SequenceId(resolved.object_identity()),
            &selection,
        )?;
        *lock_unpoisoned(&self.sequence_clipboard) = clipboard;
        Ok((
            GuiEditResult {
                snapshot,
                document: donder_editor::project_gui_document(Some(&project), request),
            },
            donder_editor::SequenceSelectionMutation {
                selection: Some(selection),
                copied_count,
                skipped_count,
            },
        ))
    }

    pub fn undo_active_edit(&self) -> AppSnapshot {
        let _authoring = lock_unpoisoned(&self.authoring);
        let Some(entry) = lock_unpoisoned(&self.gui_history).peek_undo() else {
            return self.update_snapshot(|snapshot| {
                snapshot.status = "No GUI edit to undo".to_string();
            });
        };
        let generated_text = match generated_source_texts(&entry.before, &entry.affected_paths) {
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
        let generated_text = match generated_source_texts(&entry.after, &entry.affected_paths) {
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

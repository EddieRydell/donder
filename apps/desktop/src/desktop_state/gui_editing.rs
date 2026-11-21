use std::collections::BTreeSet;
use std::sync::Arc;

use donder_editor::GuiMutationError;
use donder_editor::dto::{GuiProblem, GuiProblemSite};
use donder_project_io::ProjectSession;

use super::{DesktopState, generated_source_texts, lock_unpoisoned};
use crate::state_tasks::GuiHistoryEntry;
use donder_editor::dto::{
    AppSnapshot, GuiDocumentChange, GuiDocumentRequest, GuiEditCommand, GuiEditUpdate,
};

impl DesktopState {
    pub(crate) fn resolve_gui_source(
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
    pub(crate) fn get_sequence_clip_details(
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
    pub(crate) fn match_sequence_clips(
        &self,
        request: GuiDocumentRequest,
        query: donder_editor::dto::SequenceClipMatchRequest,
    ) -> Result<donder_editor::dto::SequenceSelection, String> {
        let project = self
            .project_session()
            .ok_or("The current project source is not ready for GUI editing.")?;
        donder_editor::matching_sequence_clips(&project, &request, &query)
    }

    pub(crate) fn get_gui_document(
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
    pub(crate) fn request_sequence_clip_rasters(
        &self,
        request: donder_editor::dto::SequenceClipRasterRequest,
    ) -> donder_editor::dto::SequenceClipRasterResponse {
        let settings = self.snapshot().settings.clip_raster;
        let project = self.project_session();
        let sequence_id = self.resolve_sequence_id(&request.document);
        lock_unpoisoned(&self.sequence_clip_raster).request(settings, project, sequence_id, request)
    }

    pub(crate) fn take_sequence_clip_raster_results(
        &self,
        request: GuiDocumentRequest,
        since_revision: u32,
    ) -> donder_editor::dto::SequenceClipRasterResultBatch {
        lock_unpoisoned(&self.sequence_clip_raster).take_results(request, since_revision)
    }

    pub(crate) fn sequence_clip_raster_pixels(&self, token: &str) -> Option<Vec<u8>> {
        lock_unpoisoned(&self.sequence_clip_raster).pixels_rgba_for_token(token)
    }

    /// Apply one GUI edit. An edit that also changes objects it left alone
    /// waits for [`Self::confirm_gui_edit`].
    pub(crate) fn edit_gui_document(
        &self,
        request: GuiDocumentRequest,
        edit: GuiEditCommand,
    ) -> GuiEditUpdate {
        let _authoring = lock_unpoisoned(&self.authoring);
        let site = donder_editor::edit_site(&edit);
        let prepared = self.prepare_gui_edit(&request, site, |session| {
            donder_editor::apply_edit(session, &request, edit)
        });
        let (prepared, outcome) = match prepared {
            Ok(prepared) => prepared,
            Err(problem) => return self.gui_edit_error(problem),
        };
        if !prepared.changes.is_empty() {
            let changes = prepared.changes.clone();
            *lock_unpoisoned(&self.pending_gui_edit) = Some(PendingGuiEdit { prepared, outcome });
            return gui_update(
                self.snapshot(),
                GuiDocumentChange::NeedsConfirmation { changes },
            );
        }
        match self.commit_gui_edit(prepared) {
            Ok(update) => with_outcome(update, outcome),
            Err(problem) => self.gui_edit_error(problem),
        }
    }

    /// Apply the edit of `request` that waits for confirmation, unless the
    /// project changed since.
    pub(crate) fn confirm_gui_edit(&self, request: GuiDocumentRequest) -> GuiEditUpdate {
        let _authoring = lock_unpoisoned(&self.authoring);
        let pending = lock_unpoisoned(&self.pending_gui_edit).take();
        let Some(PendingGuiEdit { prepared, outcome }) = pending.filter(|pending| {
            pending.prepared.request == request
                && self.snapshot().project_revision == request.project_revision
        }) else {
            return self.gui_edit_error(GuiProblem::without_site(
                "The project changed before the edit was confirmed.",
            ));
        };
        match self.commit_gui_edit(prepared) {
            Ok(update) => with_outcome(update, outcome),
            Err(problem) => self.gui_edit_error(problem),
        }
    }

    /// Remove a sequence selection as one edit, with its clipboard text: none
    /// when nothing was copied.
    pub(crate) fn cut_sequence_selection(
        &self,
        request: GuiDocumentRequest,
        selection: donder_editor::dto::SequenceSelection,
    ) -> (GuiEditUpdate, Option<String>) {
        let _authoring = lock_unpoisoned(&self.authoring);
        match self.mutate_gui_project_locked(&request, None, |session| {
            donder_editor::cut_selection(session, &request, &selection)
        }) {
            Ok((update, (text, outcome))) => (with_outcome(update, outcome), text),
            Err(problem) => (
                self.clipboard_failed(&request, Some(selection), problem.message()),
                None,
            ),
        }
    }

    /// Paste the clipboard's `text` into the sequence `request` opens, as one
    /// edit.
    pub(crate) fn paste_sequence(
        &self,
        request: GuiDocumentRequest,
        anchor: donder_editor::dto::SequencePasteAnchor,
        text: &str,
    ) -> GuiEditUpdate {
        let _authoring = lock_unpoisoned(&self.authoring);
        match self.mutate_gui_project_locked(&request, None, |session| {
            donder_editor::paste(
                session,
                &request,
                anchor,
                text,
                &mut lock_unpoisoned(&self.document_texts),
            )
        }) {
            Ok((update, outcome)) => with_outcome(update, outcome),
            Err(problem) => self.clipboard_failed(&request, None, problem.message()),
        }
    }

    /// What would refuse `edit` of the object `request` opens; none when it
    /// would be accepted. A draft form checks its edit before applying it.
    pub(crate) fn check_gui_edit(
        &self,
        request: &GuiDocumentRequest,
        edit: GuiEditCommand,
    ) -> Option<GuiProblem> {
        let project = self.project_session()?;
        donder_editor::check_edit(&project, request, edit)
    }

    /// `request`'s document as it is, after a clipboard action that changed
    /// nothing.
    pub(crate) fn unchanged_gui_update(&self, request: &GuiDocumentRequest) -> GuiEditUpdate {
        self.unchanged_gui_update_with(self.snapshot(), request)
    }

    /// `request`'s document as it is, with `message` in the status bar: a
    /// clipboard action that failed, which keeps `selection`. Clipboard
    /// failures are not document errors, so the document stays open.
    fn clipboard_failed(
        &self,
        request: &GuiDocumentRequest,
        selection: Option<donder_editor::dto::SequenceSelection>,
        message: &str,
    ) -> GuiEditUpdate {
        let snapshot = self.update_snapshot(|snapshot| snapshot.status = message.to_string());
        GuiEditUpdate {
            selection,
            ..self.unchanged_gui_update_with(snapshot, request)
        }
    }

    fn unchanged_gui_update_with(
        &self,
        snapshot: AppSnapshot,
        request: &GuiDocumentRequest,
    ) -> GuiEditUpdate {
        match self.project_session() {
            Some(project) => gui_update(
                snapshot,
                donder_editor::project_gui_document_change(&project, &project, request),
            ),
            None => self.gui_edit_error(GuiProblem::without_site("No project is loaded.")),
        }
    }

    /// [`Self::edit_gui_document`] with the whole edited document.
    #[cfg(test)]
    pub(crate) fn apply_gui_edit(
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
            GuiDocumentChange::Refused { problem } => {
                donder_editor::blocked(problem.message(), Vec::new())
            }
            GuiDocumentChange::NeedsConfirmation { changes } => {
                donder_editor::blocked(changes.join(". "), Vec::new())
            }
        };
        GuiEditResult { document }
    }

    /// Apply `mutate`, an edit of the object `request` opens that sets the
    /// field `edit_site` when it names one, as one undo step; a refused edit
    /// changes nothing. An edit that would also change objects it left alone
    /// is refused: only [`Self::edit_gui_document`] asks to confirm one.
    pub(super) fn mutate_gui_project_locked<T>(
        &self,
        request: &GuiDocumentRequest,
        edit_site: Option<GuiProblemSite>,
        mutate: impl FnOnce(&mut ProjectSession) -> Result<T, GuiMutationError>,
    ) -> Result<(GuiEditUpdate, T), GuiProblem> {
        let (prepared, value) = self.prepare_gui_edit(request, edit_site, mutate)?;
        if !prepared.changes.is_empty() {
            return Err(GuiProblem::without_site(format!(
                "This edit would also change other objects. {}.",
                prepared.changes.join(". ")
            )));
        }
        Ok((self.commit_gui_edit(prepared)?, value))
    }

    /// Make an edit on a copy of the project, as
    /// [`Self::mutate_gui_project_locked`], without applying it. A new edit
    /// drops the one waiting for confirmation.
    fn prepare_gui_edit<T>(
        &self,
        request: &GuiDocumentRequest,
        edit_site: Option<GuiProblemSite>,
        mutate: impl FnOnce(&mut ProjectSession) -> Result<T, GuiMutationError>,
    ) -> Result<(PreparedGuiEdit, T), GuiProblem> {
        lock_unpoisoned(&self.pending_gui_edit).take();
        if self.snapshot().project_revision != request.project_revision {
            return Err(GuiProblem::without_site(
                "The project changed before the edit arrived.",
            ));
        }
        let before = self
            .project_session()
            .ok_or_else(|| GuiProblem::without_site("No project is loaded."))?;
        let mut edited = (*before).clone();
        let refused = |edited: &ProjectSession, error| {
            donder_editor::refusal(&before, edited, request, edit_site.clone(), error)
        };
        let mut affected_paths = donder_editor::affected_paths(&before, request)
            .map_err(|error| refused(&edited, error))?;
        let value = mutate(&mut edited).map_err(|error| refused(&edited, error))?;
        affected_paths.extend(
            donder_editor::affected_paths(&edited, request)
                .map_err(|error| refused(&edited, error))?,
        );
        let repairs = edited.project.take_repairs();
        let changes = donder_editor::repair_changes(&before, &repairs);
        Ok((
            PreparedGuiEdit {
                request: request.clone(),
                before,
                edited,
                affected_paths,
                changes,
            },
            value,
        ))
    }

    /// Apply a prepared edit as one undo step.
    fn commit_gui_edit(&self, prepared: PreparedGuiEdit) -> Result<GuiEditUpdate, GuiProblem> {
        let PreparedGuiEdit {
            request,
            before,
            edited,
            affected_paths,
            changes,
        } = prepared;
        let generated_text = generated_source_texts(
            &edited,
            &affected_paths,
            &mut lock_unpoisoned(&self.document_texts),
        )
        .map_err(GuiProblem::without_site)?;
        let status = if changes.is_empty() {
            "GUI edit applied".to_string()
        } else {
            format!("GUI edit applied. {}.", changes.join(". "))
        };
        let edited = Arc::new(edited);
        let snapshot = self
            .accept_gui_sources(Arc::clone(&edited), generated_text, &status)
            .map_err(GuiProblem::without_site)?;
        let change = donder_editor::project_gui_document_change(&before, &edited, &request);
        lock_unpoisoned(&self.gui_history).push_undo(GuiHistoryEntry {
            before,
            after: Arc::clone(&edited),
            affected_paths,
        });
        Ok(gui_update(snapshot, change))
    }

    fn gui_edit_error(&self, problem: GuiProblem) -> GuiEditUpdate {
        gui_update(self.snapshot(), GuiDocumentChange::Refused { problem })
    }

    pub(crate) fn finish_composition_graph_editing(&self) -> AppSnapshot {
        let _authoring = lock_unpoisoned(&self.authoring);
        if let Some(session) = self.project_session() {
            self.schedule_render_refresh(session);
        }
        self.snapshot()
    }

    /// A sequence selection's clipboard text, none when nothing was copied;
    /// the project does not change.
    pub(crate) fn copy_sequence_selection(
        &self,
        request: GuiDocumentRequest,
        selection: donder_editor::dto::SequenceSelection,
    ) -> (GuiEditUpdate, Option<String>) {
        let _authoring = lock_unpoisoned(&self.authoring);
        let copied = (|| {
            if self.snapshot().project_revision != request.project_revision {
                return Err(GuiProblem::without_site(
                    "The project changed before copying.",
                ));
            }
            let project = self
                .project_session()
                .ok_or_else(|| GuiProblem::without_site("No project is loaded."))?;
            let (text, outcome) = donder_editor::copy_selection(&project, &request, &selection)
                .map_err(|error| {
                    donder_editor::refusal(&project, &project, &request, None, error)
                })?;
            let update = gui_update(
                self.snapshot(),
                donder_editor::project_gui_document_change(&project, &project, &request),
            );
            Ok((
                with_outcome(
                    update,
                    donder_editor::GuiEditOutcome {
                        selection: Some(selection.clone()),
                        ..outcome
                    },
                ),
                text,
            ))
        })();
        copied.unwrap_or_else(|problem| {
            (
                self.clipboard_failed(&request, Some(selection), problem.message()),
                None,
            )
        })
    }

    pub(crate) fn undo_active_edit(&self) -> AppSnapshot {
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
                return self.snapshot_with_error(&message);
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
            .unwrap_or_else(|error| self.snapshot_with_error(&error))
    }

    pub(crate) fn redo_active_edit(&self) -> AppSnapshot {
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
                return self.snapshot_with_error(&message);
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
            .unwrap_or_else(|error| self.snapshot_with_error(&error))
    }
}

/// An edit made on a copy of the project, not yet applied.
pub(super) struct PreparedGuiEdit {
    request: GuiDocumentRequest,
    before: Arc<ProjectSession>,
    edited: ProjectSession,
    affected_paths: BTreeSet<String>,
    /// The changes to objects the edit left alone that need confirmation, a
    /// sentence each.
    changes: Vec<String>,
}

/// An edit that waits for the person to confirm its changes to other objects.
pub(super) struct PendingGuiEdit {
    prepared: PreparedGuiEdit,
    outcome: donder_editor::GuiEditOutcome,
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
pub(crate) struct GuiEditResult {
    pub(crate) document: donder_editor::dto::GuiDocument,
}

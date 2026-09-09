use std::sync::Arc;

use crate::gui::GuiMutationError;
use dawn_language::sequence::SequenceId;
use dawn_project_io::ProjectSession;

use super::{DesktopState, generated_source_texts, lock_unpoisoned};
use crate::dto::{
    AppSnapshot, GuiDocumentRequest, GuiEditCommand, GuiEditResult, SequenceSelectionEdit,
    SequenceSelectionEditResult,
};
use crate::state_tasks::GuiHistoryEntry;

impl DesktopState {
    pub fn resolve_gui_source(
        &self,
        module_id: &str,
        path: &str,
        object_key: &str,
    ) -> Result<GuiDocumentRequest, String> {
        let _authoring = self.settled_authoring();
        let session = self.project_session().ok_or("No project is loaded.")?;
        let identity = crate::gui::model::source_identity_from_gui(module_id, path, object_key)
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
        let path = crate::source_documents::editor_path(&session, identity.document_id())
            .ok_or("Source document has no file location.")?;
        Ok(GuiDocumentRequest {
            project_revision: self.snapshot().project_revision,
            path: path.to_string(),
            object_key: Some(object_key.to_string()),
            view: crate::dto::ObjectKind::from(object.kind())
                .document_view()
                .unwrap_or(crate::dto::DocumentViewId::Text),
        })
    }

    pub fn get_gui_document(&self, request: GuiDocumentRequest) -> crate::dto::GuiDocumentResult {
        let _authoring = lock_unpoisoned(&self.authoring);
        let revision = self.snapshot().project_revision;
        let document = if revision != request.project_revision {
            crate::gui::blocked(
                "The project changed. Request the current GUI document.",
                Vec::new(),
            )
        } else if let Some(project) = self.project_session() {
            crate::gui::project_gui_document(Some(&project), &request)
        } else {
            crate::gui::blocked(
                "The current project source is not ready for GUI editing.",
                self.snapshot().diagnostics,
            )
        };
        crate::dto::GuiDocumentResult {
            request,
            project_revision: revision,
            document,
        }
    }

    pub fn request_sequence_clip_rasters(
        &self,
        request: crate::dto::SequenceClipRasterRequest,
    ) -> crate::dto::SequenceClipRasterResponse {
        let _authoring = lock_unpoisoned(&self.authoring);
        let snapshot = self.snapshot();
        let project_revision = snapshot.project_revision;
        let raster_settings = snapshot.settings.effect_raster;
        let project = (request.document.project_revision == project_revision)
            .then(|| self.project_session())
            .flatten();
        let setup_id = project
            .as_ref()
            .map(|project| project.project.root.setup.clone());
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
    ) -> crate::dto::SequenceClipRasterResultBatch {
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
            crate::gui::apply_edit(session, &request, edit)
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

    fn mutate_gui_project_locked<T>(
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
        let mut affected_paths = crate::gui::affected_paths(&before, request)?;
        let mut edited = (*before).clone();
        let value = mutate(&mut edited)?;
        affected_paths.extend(crate::gui::affected_paths(&edited, request)?);
        dawn_language::validation::validate_project(&edited.project)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
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
                    && projection.request.object_key == request.object_key =>
            {
                projection.document.clone()
            }
            _ => crate::gui::project_gui_document(Some(&edited), request),
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
            document: crate::gui::blocked(error.message(), Vec::new()),
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
                crate::gui::apply_sequence_selection_edit(
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
        selection: crate::dto::SequenceSelection,
    ) -> Result<(GuiEditResult, crate::gui::SequenceSelectionMutation), GuiMutationError> {
        let _authoring = lock_unpoisoned(&self.authoring);
        if self.snapshot().project_revision != request.project_revision {
            return Err(GuiMutationError::Blocked(
                "The project changed before copying".into(),
            ));
        }
        if !matches!(request.view, crate::dto::DocumentViewId::Sequence) {
            return Err(GuiMutationError::Invalid(
                "Copy requires a sequence GUI document.".to_string(),
            ));
        }
        let snapshot = self.snapshot();
        let project = self
            .project_session()
            .ok_or_else(|| GuiMutationError::Blocked("No project is loaded.".to_string()))?;
        let resolved =
            crate::gui::resolve_request(&project, request).map_err(GuiMutationError::Invalid)?;
        crate::gui::ensure_owned_gui_document(&project, &resolved)?;
        let (clipboard, copied_count, skipped_count) = crate::gui::copy_sequence_selection(
            &project,
            &SequenceId(resolved.identity),
            &selection,
        )?;
        *lock_unpoisoned(&self.sequence_clipboard) = clipboard;
        Ok((
            GuiEditResult {
                snapshot,
                document: crate::gui::project_gui_document(Some(&project), request),
            },
            crate::gui::SequenceSelectionMutation {
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

#[cfg(test)]
mod fixed_parameter_tests {
    use super::*;
    use crate::dto::{
        DocumentViewId, GuiDocument, SequenceAutomationMapping, SequenceAutomationTarget,
        SequenceEffectParamValue, SequenceEffectReference, SequenceGuiEdit,
    };
    use dawn_language::effect::EffectParamValue;

    #[test]
    fn fixed_edits_history_detachment_and_persistence_use_canonical_metadata() {
        let (_temporary, root) = crate::desktop_foundation_tests::tests::starter_copy();
        std::fs::write(root.join("effects/mark-impact-burst.effect.dawn"), "effect MarkImpactBurst { fixed param float level = 0.5; color sample() { return rgb(level, level, level); } } effect LiveLevel { param float level = 0.5; color sample() { return rgb(level, level, level); } }").unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let mut settings = state.snapshot().settings;
        settings.autosave_project_edits = false;
        state.update_app_settings(settings);
        let initial = state.project_session().unwrap();
        let sequence = initial
            .project
            .sequences
            .values()
            .find(|sequence| !sequence.effects.is_empty())
            .unwrap();
        let sequence_id = sequence.id.clone();
        let effect_id = sequence.effects[0].id.0;
        let module_id = sequence.id.0.module_id().to_string();
        let request = || GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: sequence_id.0.document().to_string(),
            view: DocumentViewId::Sequence,
            object_key: Some(sequence_id.0.object().into()),
        };
        let edit = |edit| state.apply_gui_edit(request(), GuiEditCommand::Sequence { edit });
        let reference = |name: &str| SequenceEffectReference::Custom {
            module_id: module_id.clone(),
            path: "effects/mark-impact-burst.effect.dawn".into(),
            effect_name: name.into(),
        };
        let result = edit(SequenceGuiEdit::ChangeEffectDefinition {
            id: effect_id,
            effect: reference("MarkImpactBurst"),
            initial_color: sequence.layers[0].color.to_hex(),
        });
        let GuiDocument::Sequence { document } = result.document else {
            panic!("definition edit rejected")
        };
        let param = &document
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)
            .unwrap()
            .params[0];
        assert!(param.fixed);
        assert!(!param.supports_automation);
        let result = edit(SequenceGuiEdit::UpdateEffectParam {
            id: effect_id,
            name: "level".into(),
            value: SequenceEffectParamValue::Float { value: 0.8 },
        });
        assert!(matches!(result.document, GuiDocument::Sequence { .. }));
        let level = dawn_language::dsl::Identifier::new("level".into()).unwrap();
        let current_level = || {
            state.project_session().unwrap().project.sequences[&sequence_id]
                .effects
                .iter()
                .find(|effect| effect.id.0 == effect_id)
                .unwrap()
                .param_overrides
                .get(&level)
                .cloned()
        };
        assert_eq!(current_level(), Some(EffectParamValue::Float(0.8)));
        state.undo_active_edit();
        assert_eq!(current_level(), None);
        state.redo_active_edit();
        assert_eq!(current_level(), Some(EffectParamValue::Float(0.8)));
        let target = || SequenceAutomationTarget::EffectParam {
            effect_id,
            param: "level".into(),
        };
        let mapping = || SequenceAutomationMapping::Float { min: 0.0, max: 1.0 };
        let before = state.project_session().unwrap();
        let revision = state.snapshot().project_revision;
        let rejected = edit(SequenceGuiEdit::CreateAndBindAutomationClip {
            target: target(),
            mapping: mapping(),
        });
        assert!(
            matches!(rejected.document, GuiDocument::Blocked { reason, .. } if reason.contains("fixed"))
        );
        assert_eq!(state.snapshot().project_revision, revision);
        assert!(Arc::ptr_eq(&before, &state.project_session().unwrap()));
        assert!(matches!(
            edit(SequenceGuiEdit::ChangeEffectDefinition {
                id: effect_id,
                effect: reference("LiveLevel"),
                initial_color: sequence.layers[0].color.to_hex(),
            })
            .document,
            GuiDocument::Sequence { .. }
        ));
        assert!(matches!(
            edit(SequenceGuiEdit::CreateAndBindAutomationClip {
                target: target(),
                mapping: mapping()
            })
            .document,
            GuiDocument::Sequence { .. }
        ));
        let clip_id = state.project_session().unwrap().project.sequences[&sequence_id]
            .automation_clips
            .iter()
            .find(|clip| {
                clip.bindings.iter().any(|binding| {
                    binding
                        .effect_param()
                        .is_some_and(|(id, name)| id.0 == effect_id && name == &level)
                })
            })
            .unwrap()
            .id
            .0;
        assert!(matches!(
            edit(SequenceGuiEdit::ChangeEffectDefinition {
                id: effect_id,
                effect: reference("MarkImpactBurst"),
                initial_color: sequence.layers[0].color.to_hex(),
            })
            .document,
            GuiDocument::Sequence { .. }
        ));
        let detached = state.project_session().unwrap();
        let clip = detached.project.sequences[&sequence_id]
            .automation_clips
            .iter()
            .find(|clip| clip.id.0 == clip_id)
            .unwrap();
        assert!(clip.bindings.is_empty());
        assert_eq!(clip.detached_bindings.len(), 1);
        let rejected = edit(SequenceGuiEdit::RebindDetachedAutomation {
            clip_id,
            detached_index: 0,
            target: target(),
            mapping: mapping(),
        });
        assert!(
            matches!(rejected.document, GuiDocument::Blocked { reason, .. } if reason.contains("fixed"))
        );
        assert!(Arc::ptr_eq(&detached, &state.project_session().unwrap()));
        state.undo_active_edit();
        assert_eq!(
            state.project_session().unwrap().project.sequences[&sequence_id]
                .automation_clips
                .iter()
                .find(|clip| clip.id.0 == clip_id)
                .unwrap()
                .bindings
                .len(),
            1
        );
        state.redo_active_edit();
        let final_session = state.project_session().unwrap();
        state.save_all().unwrap();
        let reloaded = dawn_project_io::load_package(&root).unwrap().session;
        assert_eq!(reloaded.project, final_session.project);
    }
}

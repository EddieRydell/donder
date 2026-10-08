use super::*;

pub fn apply_edit(
    session: &mut ProjectSession,
    request: &GuiDocumentRequest,
    edit: GuiEditCommand,
) -> Result<(), GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    ensure_owned_gui_document(session, &resolved)?;
    match (request.view.clone(), edit) {
        (_, GuiEditCommand::Ownership { slot, edit }) => {
            super::ownership::edit(session, &resolved, slot, edit)?
        }
        (view, GuiEditCommand::Description { description }) => {
            super::description::edit(session, &resolved, &view, description)?;
        }
        (DocumentViewId::Sequence, GuiEditCommand::Sequence { edit }) => {
            edit_sequence(session, &resolved, edit)?;
        }

        (DocumentViewId::Setup, GuiEditCommand::Setup { edit }) => {
            edit_setup(session, &resolved, edit)?;
        }
        (DocumentViewId::Layout, GuiEditCommand::Layout { edit }) => {
            edit_layout(session, &resolved, edit)?;
        }
        (DocumentViewId::Fixture, GuiEditCommand::Fixture { edit }) => {
            edit_fixture(session, &resolved, edit)?;
        }
        (DocumentViewId::Curve, GuiEditCommand::Curve { points }) => {
            super::library::edit_curve(session, &resolved, points)?;
        }
        (DocumentViewId::Gradient, GuiEditCommand::Gradient { stops }) => {
            super::library::edit_gradient(session, &resolved, stops)?;
        }
        (DocumentViewId::Controller, GuiEditCommand::Controller { config, ports }) => {
            super::controller::edit(session, &resolved, config, ports)?;
        }

        (DocumentViewId::Patch, GuiEditCommand::Patch { routes }) => {
            super::patch::replace(
                session,
                &donder_model::PatchId(resolved.object_identity()),
                routes,
            )?;
        }
        _ => {
            return Err(GuiMutationError::Invalid(
                "GUI edit type does not match the requested document view.".to_string(),
            ));
        }
    }
    Ok(())
}

#[derive(Clone)]
pub enum SequenceClipboard {
    Clips {
        effects: Vec<ClipboardEffect>,
        automation: Vec<ClipboardAutomation>,
        source: SequenceId,
        cut: bool,
    },
    Marks(Vec<ClipboardMark>),
}

#[derive(Clone)]
pub struct ClipboardEffect {
    pub effect: EffectInst,
    pub start_seconds: f32,
    pub lane_index: usize,
}

#[derive(Clone)]
pub struct ClipboardAutomation {
    pub clip: donder_model::AutomationClip,
    pub lane_index: usize,
}

#[derive(Clone)]
pub struct ClipboardMark {
    pub collection_key: String,
    pub time_seconds: f32,
}

pub struct SequenceSelectionMutation {
    pub selection: Option<SequenceSelection>,
    pub copied_count: u32,
    pub skipped_count: u32,
}

pub fn apply_sequence_selection_edit(
    session: &mut ProjectSession,
    request: &GuiDocumentRequest,
    edit: SequenceSelectionEdit,
    clipboard: &mut Option<SequenceClipboard>,
) -> Result<SequenceSelectionMutation, GuiMutationError> {
    if !matches!(request.view, DocumentViewId::Sequence) {
        return Err(GuiMutationError::Invalid(
            "Sequence selection edits require a sequence GUI document.".to_string(),
        ));
    }
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    ensure_owned_gui_document(session, &resolved)?;
    let sequence_id = SequenceId(resolved.object_identity());
    match edit {
        SequenceSelectionEdit::Copy { .. } => Err(GuiMutationError::Invalid(
            "Copy must use the read-only selection path.".to_string(),
        )),
        SequenceSelectionEdit::Cut { selection } => {
            let (next_clipboard, copied_count, skipped_count) =
                copy_sequence_selection(session, &sequence_id, &selection)?;
            *clipboard = next_clipboard;
            if let Some(SequenceClipboard::Clips { cut, .. }) = clipboard {
                *cut = true;
            }
            delete_sequence_selection(session, &sequence_id, &selection)?;
            Ok(SequenceSelectionMutation {
                selection: None,
                copied_count,
                skipped_count,
            })
        }
        SequenceSelectionEdit::Delete { selection } => {
            delete_sequence_selection(session, &sequence_id, &selection)?;
            Ok(SequenceSelectionMutation {
                selection: None,
                copied_count: 0,
                skipped_count: 0,
            })
        }
        SequenceSelectionEdit::Paste { anchor } => {
            paste_sequence_clipboard(session, &sequence_id, anchor, clipboard.as_ref())
        }
        SequenceSelectionEdit::MoveClips {
            effect_ids,
            automation_ids,
            time_delta_seconds,
            anchor_lane,
            lane_delta,
        } => {
            move_clip_selection(
                session,
                &sequence_id,
                &effect_ids,
                &automation_ids,
                time_delta_seconds,
                anchor_lane as usize,
                lane_delta,
            )?;
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Clips {
                    effect_ids,
                    automation_ids,
                }),
                copied_count: 0,
                skipped_count: 0,
            })
        }
        SequenceSelectionEdit::ResizeClips {
            effect_ids,
            automation_ids,
            edge,
            automation,
            time_delta_seconds,
        } => {
            resize_clip_selection(
                session,
                &sequence_id,
                &effect_ids,
                &automation_ids,
                edge,
                automation,
                time_delta_seconds,
            )?;
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Clips {
                    effect_ids,
                    automation_ids,
                }),
                copied_count: 0,
                skipped_count: 0,
            })
        }
        SequenceSelectionEdit::EditEffects { effect_ids, edit } => {
            edit_effect_selection(session, &resolved.identity, &sequence_id, &effect_ids, edit)?;
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Clips {
                    effect_ids,
                    automation_ids: Vec::new(),
                }),
                copied_count: 0,
                skipped_count: 0,
            })
        }
        SequenceSelectionEdit::MoveMarks {
            marks,
            time_delta_seconds,
        } => {
            let moved = move_mark_selection(session, &sequence_id, &marks, time_delta_seconds)?;
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Marks { marks: moved }),
                copied_count: 0,
                skipped_count: 0,
            })
        }
    }
}

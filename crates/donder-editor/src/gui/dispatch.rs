use super::*;
use crate::dto::SequenceGuiEdit;

/// What an edit reports beyond the edited project: the sequence selection a
/// selection edit leaves, what a cut or paste took, and the names it created.
#[derive(Clone, Debug, Default)]
pub struct GuiEditOutcome {
    pub selection: Option<SequenceSelection>,
    pub copied_count: u32,
    pub skipped_count: u32,
    pub created: Vec<String>,
}

impl GuiEditOutcome {
    fn created(created: Vec<String>) -> Self {
        Self {
            created,
            ..Self::default()
        }
    }

    fn selection(selection: SequenceSelection) -> Self {
        Self {
            selection: Some(selection),
            ..Self::default()
        }
    }
}

/// Apply one GUI edit to the object `request` opens. The object's kind picks
/// the editor; an edit for another kind of object is refused.
pub fn apply_edit(
    session: &mut ProjectSession,
    request: &GuiDocumentRequest,
    edit: GuiEditCommand,
    clipboard: &mut Option<SequenceClipboard>,
) -> Result<GuiEditOutcome, GuiMutationError> {
    use super::item::Verb;
    use donder_model::ObjectKind as Kind;
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    let none = |()| GuiEditOutcome::default();
    match (resolved.kind, edit) {
        (_, GuiEditCommand::Ownership { member, edit }) => {
            super::ownership::edit(session, &resolved, member, edit).map(none)
        }
        (_, GuiEditCommand::Rename { item, name }) => {
            super::item::edit(session, &resolved, item, Verb::Rename(name))
                .map(GuiEditOutcome::created)
        }
        (_, GuiEditCommand::SetDescription { item, description }) => {
            super::item::edit(session, &resolved, item, Verb::Describe(description))
                .map(GuiEditOutcome::created)
        }
        (_, GuiEditCommand::SetColor { item, color }) => {
            super::item::edit(session, &resolved, item, Verb::Color(color))
                .map(GuiEditOutcome::created)
        }
        (_, GuiEditCommand::Delete { item }) => {
            super::item::edit(session, &resolved, item, Verb::Delete).map(GuiEditOutcome::created)
        }
        (Kind::Sequence, GuiEditCommand::Sequence { edit }) => {
            edit_sequence_selection(session, &resolved, edit, clipboard)
        }
        (Kind::Setup, GuiEditCommand::Setup { edit }) => {
            edit_setup(session, &resolved, edit).map(none)
        }
        (Kind::Layout, GuiEditCommand::Layout { edit }) => {
            edit_layout(session, &resolved, edit).map(none)
        }
        (Kind::FixtureDefinition, GuiEditCommand::Fixture { edit }) => {
            edit_fixture(session, &resolved, edit).map(none)
        }
        (Kind::Curve, GuiEditCommand::Curve { points }) => {
            super::library::edit_curve(session, &resolved, points).map(none)
        }
        (Kind::Gradient, GuiEditCommand::Gradient { stops }) => {
            super::library::edit_gradient(session, &resolved, stops).map(none)
        }
        (Kind::Controller, GuiEditCommand::Controller { config, ports }) => {
            super::controller::edit(session, &resolved, config, ports).map(none)
        }
        (Kind::InputDevice, GuiEditCommand::InputDevice { driver, controls }) => {
            super::input_device::edit(session, &resolved, driver, controls).map(none)
        }
        (Kind::Rig, GuiEditCommand::Rig { edit }) => {
            super::rig::edit_rig(session, &resolved, edit).map(GuiEditOutcome::created)
        }
        (Kind::Project, GuiEditCommand::Project { edit }) => {
            super::project::edit_project(session, &resolved, edit).map(none)
        }
        (Kind::Patch, GuiEditCommand::Patch { routes }) => {
            super::patch::replace(session, &resolved.id::<donder_model::PatchId>(), routes)
                .map(none)
        }
        _ => Err(GuiMutationError::Invalid(
            "This edit does not apply to the open object.".to_string(),
        )),
    }
}

#[derive(Clone)]
pub enum SequenceClipboard {
    Clips {
        clips: Vec<ClipboardClip>,
        automation: Vec<ClipboardAutomation>,
        sequence: SequenceId,
        cut: bool,
    },
    Marks(Vec<ClipboardMark>),
}

#[derive(Clone)]
pub struct ClipboardClip {
    pub(crate) clip: Clip,
    pub(crate) start_seconds: f64,
    pub(crate) lane_index: usize,
}

#[derive(Clone)]
pub struct ClipboardAutomation {
    pub(crate) clip: donder_model::AutomationClip,
    pub(crate) lane_index: usize,
}

#[derive(Clone)]
pub struct ClipboardMark {
    pub(crate) collection_key: String,
    pub(crate) time_seconds: f64,
    pub(crate) label: Option<String>,
}

/// Copy `selection` of the sequence `request` opens; the project does not
/// change.
pub fn copy_selection(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
    selection: &SequenceSelection,
) -> Result<(Option<SequenceClipboard>, GuiEditOutcome), GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    let sequence_id = SequenceId::try_from(resolved.object).map_err(|_| {
        GuiMutationError::Invalid("Copy requires a sequence GUI document.".to_string())
    })?;
    let (clipboard, copied_count, skipped_count) =
        copy_sequence_selection(session, &sequence_id, selection)?;
    Ok((
        clipboard,
        GuiEditOutcome {
            copied_count,
            skipped_count,
            ..GuiEditOutcome::default()
        },
    ))
}

/// A sequence edit: selection edits change the clips and marks they name,
/// other edits the sequence as a whole.
fn edit_sequence_selection(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: SequenceGuiEdit,
    clipboard: &mut Option<SequenceClipboard>,
) -> Result<GuiEditOutcome, GuiMutationError> {
    let sequence_id = resolved.id::<SequenceId>();
    match edit {
        SequenceGuiEdit::CutSelection { selection } => {
            let (next_clipboard, copied_count, skipped_count) =
                copy_sequence_selection(session, &sequence_id, &selection)?;
            *clipboard = next_clipboard;
            if let Some(SequenceClipboard::Clips { cut, .. }) = clipboard {
                *cut = true;
            }
            delete_sequence_selection(session, &sequence_id, &selection)?;
            Ok(GuiEditOutcome {
                copied_count,
                skipped_count,
                ..GuiEditOutcome::default()
            })
        }
        SequenceGuiEdit::DeleteSelection { selection } => {
            delete_sequence_selection(session, &sequence_id, &selection)?;
            Ok(GuiEditOutcome::default())
        }
        SequenceGuiEdit::Paste { anchor } => {
            paste_sequence_clipboard(session, &sequence_id, anchor, clipboard.as_ref())
        }
        SequenceGuiEdit::MoveClips {
            clips,
            time_delta_seconds,
            anchor_lane,
            lane_delta,
        } => {
            let (clip_ids, automation) = super::selection::split_clips(&clips);
            move_clip_selection(
                session,
                &sequence_id,
                &clip_ids,
                &automation,
                time_delta_seconds,
                anchor_lane as usize,
                lane_delta,
            )?;
            Ok(GuiEditOutcome::selection(SequenceSelection::Clips {
                clips,
            }))
        }
        SequenceGuiEdit::ResizeClips {
            clips,
            edge,
            automation: resize,
            time_delta_seconds,
        } => {
            let (clip_ids, automation) = super::selection::split_clips(&clips);
            resize_clip_selection(
                session,
                &sequence_id,
                &clip_ids,
                &automation,
                edge,
                resize,
                time_delta_seconds,
            )?;
            Ok(GuiEditOutcome::selection(SequenceSelection::Clips {
                clips,
            }))
        }
        SequenceGuiEdit::ArrangeClips { clips, arrangement } => {
            let (clip_ids, automation) = super::selection::split_clips(&clips);
            arrange_clip_selection(session, &sequence_id, &clip_ids, &automation, &arrangement)?;
            Ok(GuiEditOutcome::selection(SequenceSelection::Clips {
                clips,
            }))
        }
        SequenceGuiEdit::EditClips { clips, edit } => {
            let (clip_ids, automation) = super::selection::split_clips(&clips);
            if !automation.is_empty() {
                return Err(GuiMutationError::Invalid(
                    "Only clips have these properties.".to_string(),
                ));
            }
            edit_clip_selection(
                session,
                resolved.document_id(),
                &sequence_id,
                &clip_ids,
                edit,
            )?;
            Ok(GuiEditOutcome::selection(SequenceSelection::Clips {
                clips,
            }))
        }
        SequenceGuiEdit::MoveMarks {
            marks,
            time_delta_seconds,
        } => {
            let moved = move_mark_selection(session, &sequence_id, &marks, time_delta_seconds)?;
            Ok(GuiEditOutcome::selection(SequenceSelection::Marks {
                marks: moved,
            }))
        }
        edit => edit_sequence(session, resolved, edit).map(GuiEditOutcome::created),
    }
}

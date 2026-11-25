use super::put::DeletedMember;
use super::*;
use crate::dto::GuiEdit;
use crate::dto::SequenceGuiEdit;

/// What an edit reports beyond the edited project: the sequence selection a
/// selection edit leaves, what a cut or paste took, and the names it created.
#[derive(Default)]
pub struct GuiEditOutcome {
    pub selection: Option<SequenceSelection>,
    pub created: Vec<String>,
    /// The object a removal took, whose file the caller deletes.
    pub deleted: Option<DeletedMember>,
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
    edit: GuiEdit,
) -> Result<GuiEditOutcome, GuiMutationError> {
    use donder_model::ObjectKind as Kind;
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    let none = |()| GuiEditOutcome::default();
    match (resolved.kind, edit) {
        (_, GuiEdit::Put { sites, value }) => {
            super::put::put(session, &resolved, sites, value).map(GuiEditOutcome::created)
        }
        (_, GuiEdit::Remove { site }) => {
            super::put::remove(session, &resolved, &site).map(|deleted| GuiEditOutcome {
                deleted,
                ..GuiEditOutcome::default()
            })
        }
        (Kind::Sequence, GuiEdit::Sequence { edit }) => {
            edit_sequence_selection(session, &resolved, edit)
        }
        (Kind::Layout, GuiEdit::Layout { edit }) => edit_layout(session, &resolved, edit).map(none),
        (Kind::FixtureDefinition, GuiEdit::Fixture { edit }) => {
            edit_fixture(session, &resolved, edit).map(none)
        }
        (Kind::Console, GuiEdit::Console { edit }) => {
            super::console::edit_console(session, &resolved, edit).map(GuiEditOutcome::created)
        }
        (Kind::Project, GuiEdit::Project { edit }) => {
            super::project::edit_project(session, edit).map(none)
        }
        _ => Err(GuiMutationError::Invalid(
            "This edit does not apply to the open object.".to_string(),
        )),
    }
}

/// The sequence `request` opens.
fn opened_sequence(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<ResolvedGuiObject, GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    if resolved.kind != donder_model::ObjectKind::Sequence {
        return Err(GuiMutationError::Invalid(
            "Open a sequence to copy or paste clips.".to_string(),
        ));
    }
    Ok(resolved)
}

/// `selection` of the sequence `request` opens as clipboard text, none when
/// nothing was copied; the project does not change.
pub fn copy_selection(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
    selection: &SequenceSelection,
) -> Result<(Option<String>, GuiEditOutcome), GuiMutationError> {
    let sequence = opened_sequence(session, request)?;
    let text = copy_sequence_selection(session, &sequence, selection)?;
    Ok((text, GuiEditOutcome::default()))
}

/// Copy `selection`, as [`copy_selection`] does, and remove it.
pub fn cut_selection(
    session: &mut ProjectSession,
    request: &GuiDocumentRequest,
    selection: &SequenceSelection,
) -> Result<(Option<String>, GuiEditOutcome), GuiMutationError> {
    let copied = copy_selection(session, request, selection)?;
    delete_sequence_selection(session, &opened_sequence(session, request)?, selection)?;
    Ok(copied)
}

/// Paste the clipboard's `text` into the sequence `request` opens; the
/// pasted items become the selection. `texts` keeps printed clips between
/// calls.
pub fn paste(
    session: &mut ProjectSession,
    request: &GuiDocumentRequest,
    anchor: crate::dto::SequencePasteAnchor,
    text: &str,
    texts: &mut donder_project_io::DocumentTextCache,
) -> Result<GuiEditOutcome, GuiMutationError> {
    let sequence = opened_sequence(session, request)?;
    paste_sequence_clipboard(session, &sequence, anchor, text, texts)
}

/// A sequence edit: selection edits change the clips and marks they name,
/// other edits the sequence as a whole.
fn edit_sequence_selection(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: SequenceGuiEdit,
) -> Result<GuiEditOutcome, GuiMutationError> {
    match edit {
        SequenceGuiEdit::DeleteSelection { selection } => {
            delete_sequence_selection(session, resolved, &selection)?;
            Ok(GuiEditOutcome::default())
        }
        SequenceGuiEdit::MoveClips {
            ids,
            time_delta_seconds,
            anchor_lane,
            lane_delta,
        } => {
            move_clip_selection(
                session,
                resolved,
                &ids,
                time_delta_seconds,
                anchor_lane as usize,
                lane_delta,
            )?;
            Ok(GuiEditOutcome::selection(SequenceSelection::Clips { ids }))
        }
        SequenceGuiEdit::ResizeClips {
            ids,
            edge,
            automation: resize,
            time_delta_seconds,
        } => {
            resize_clip_selection(session, resolved, &ids, edge, resize, time_delta_seconds)?;
            Ok(GuiEditOutcome::selection(SequenceSelection::Clips { ids }))
        }
        SequenceGuiEdit::ArrangeClips { ids, arrangement } => {
            arrange_clip_selection(session, resolved, &ids, &arrangement)?;
            Ok(GuiEditOutcome::selection(SequenceSelection::Clips { ids }))
        }
        SequenceGuiEdit::MoveMarks {
            marks,
            time_delta_seconds,
        } => {
            let moved = move_mark_selection(session, resolved, &marks, time_delta_seconds)?;
            Ok(GuiEditOutcome::selection(SequenceSelection::Marks {
                marks: moved,
            }))
        }
        edit => edit_sequence(session, resolved, edit).map(GuiEditOutcome::created),
    }
}

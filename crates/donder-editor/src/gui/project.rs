use super::model::object_ref;
use donder_project_io::ProjectSession;

use super::{ResolvedGuiObject, blocked};
use crate::dto::{GuiDocument, ProjectGuiDocument, ProjectGuiEdit};

/// A new sequence's or console's frame rate: Perform's target rate.
const NEW_FRAME_RATE: u32 = 100;
/// A new sequence's length.
const NEW_SEQUENCE_SECONDS: f64 = 60.0;

pub(super) fn edit_project(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: ProjectGuiEdit,
) -> Result<(), super::GuiMutationError> {
    if resolved.object != session.project.root().id.into() {
        return Err(super::GuiMutationError::Invalid(
            "The requested project is not the active project.".into(),
        ));
    }
    let (id, name, storage) = match edit {
        ProjectGuiEdit::AddSequence {
            initial_color,
            storage,
        } => {
            let color = super::model::parse_color(&initial_color)?;
            let duration = super::checked_gui_duration(NEW_SEQUENCE_SECONDS)?;
            let id =
                donder_model::add_sequence(&mut session.project, duration, NEW_FRAME_RATE, color)
                    .map_err(super::GuiMutationError::from)?;
            // `add_sequence` appends the sequence inline under a fresh name.
            let name = new_member_name(session.project.root().sequences.last())?;
            (id.into(), name, storage)
        }
        ProjectGuiEdit::AddConsole {
            initial_color,
            storage,
        } => {
            let color = super::model::parse_color(&initial_color)?;
            let id = donder_model::add_console(&mut session.project, NEW_FRAME_RATE, color)
                .map_err(super::GuiMutationError::from)?;
            // `add_console` appends the console inline under a fresh name.
            let name = new_member_name(session.project.root().consoles.last())?;
            (id.into(), name, storage)
        }
    };
    super::ownership::store_new_member(session, resolved, resolved.object, id, &name, storage)
}

fn new_member_name<T, I>(
    member: Option<&donder_model::OwnedMember<T, I>>,
) -> Result<String, super::GuiMutationError> {
    member
        .and_then(donder_model::OwnedMember::name)
        .map(|name| name.as_str().to_string())
        .ok_or_else(|| super::GuiMutationError::Invalid("The new member is missing.".into()))
}

pub(super) fn project_root(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    if resolved.object != session.project.root().id.into() {
        return blocked("The requested project is missing.", Vec::new());
    }

    GuiDocument::Project {
        document: ProjectGuiDocument {
            description: session.project.root().description.clone(),
            available_declarations: super::ownership::available_declarations(
                session,
                &[
                    donder_model::ObjectKind::Setup,
                    donder_model::ObjectKind::Sequence,
                    donder_model::ObjectKind::Console,
                ],
            ),
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            setup: object_ref(session, *session.project.root().setup.id()),
            sequences: session
                .project
                .root()
                .sequences
                .iter()
                .map(|sequence| object_ref(session, *sequence.id()))
                .collect(),
            consoles: session
                .project
                .root()
                .consoles
                .iter()
                .map(|console| object_ref(session, *console.id()))
                .collect(),
        },
    }
}

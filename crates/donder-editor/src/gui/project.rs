use super::model::object_ref;
use donder_project_io::ProjectSession;

use super::ResolvedGuiObject;
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
    match edit {
        ProjectGuiEdit::AddSequence {
            initial_color,
            name,
        } => {
            let color = super::model::parse_color(&initial_color)?;
            let duration = super::checked_gui_duration(NEW_SEQUENCE_SECONDS)?;
            let id = session.edit_project(|project| {
                donder_model::add_sequence(project, duration, NEW_FRAME_RATE, color)
                    .map_err(super::GuiMutationError::from)
            })?;
            super::ownership::store_new_member(session, resolved.object, id.into(), &name)
        }
        ProjectGuiEdit::AddConsole {
            initial_color,
            name,
        } => {
            let color = super::model::parse_color(&initial_color)?;
            let id = session.edit_project(|project| {
                donder_model::add_console(project, NEW_FRAME_RATE, color)
                    .map_err(super::GuiMutationError::from)
            })?;
            super::ownership::store_new_member(session, resolved.object, id.into(), &name)
        }
    }
}

pub(super) fn project_root(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    GuiDocument::Project {
        document: ProjectGuiDocument {
            description: session.project().root().description.clone(),
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            setup: object_ref(session, *session.project().root().setup.id()),
            sequences: session
                .project()
                .root()
                .sequences
                .iter()
                .map(|sequence| object_ref(session, *sequence.id()))
                .collect(),
            consoles: session
                .project()
                .root()
                .consoles
                .iter()
                .map(|console| object_ref(session, *console.id()))
                .collect(),
            new_sequence_name: super::model::new_object_name(
                session,
                donder_model::ObjectKind::Sequence,
            ),
            new_console_name: super::model::new_object_name(
                session,
                donder_model::ObjectKind::Console,
            ),
        },
    }
}

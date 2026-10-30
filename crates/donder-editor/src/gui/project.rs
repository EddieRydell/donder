use super::model::object_ref;
use donder_project_io::ProjectSession;

use super::{ResolvedGuiObject, blocked};
use crate::dto::{GuiDocument, ProjectGuiDocument, ProjectGuiEdit};

/// A new console's frame rate: Perform's target rate.
const NEW_CONSOLE_FRAME_RATE: u32 = 100;

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
    match edit {
        ProjectGuiEdit::AddConsole { initial_color } => {
            let color = super::model::parse_color(&initial_color)?;
            donder_model::add_console(&mut session.project, NEW_CONSOLE_FRAME_RATE, color)
                .map_err(super::GuiMutationError::from)?;
        }
    }
    Ok(())
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

pub fn create_sequence(
    session: &mut ProjectSession,
    owner: &super::ResolvedGuiObject,
    request: crate::dto::NewSequenceRequest,
) -> Result<crate::dto::GuiObjectRef, super::GuiMutationError> {
    use super::GuiMutationError;
    use crate::dto::{GuiOwnershipEdit, NewSequenceStorage, ReusableStorage};
    let duration = super::checked_gui_duration(request.duration_seconds)?;
    let color = super::model::parse_color(&request.initial_color)?;
    let id = donder_model::add_sequence(&mut session.project, duration, request.frame_rate, color)
        .map_err(GuiMutationError::from)?;
    let reusable = match request.storage {
        NewSequenceStorage::Inline => None,
        NewSequenceStorage::SameFile { name } => Some((name, ReusableStorage::SameFile)),
        NewSequenceStorage::NewFile { name } => Some((name, ReusableStorage::NewFile)),
    };
    if let Some((name, storage)) = reusable {
        super::ownership::edit_site(
            session,
            owner,
            donder_model::OwnershipSite::Member {
                owner: session.project.root().id.into(),
                member: id.into(),
            },
            GuiOwnershipEdit::MakeReusable { name, storage },
        )?;
    }
    Ok(object_ref(session, id))
}

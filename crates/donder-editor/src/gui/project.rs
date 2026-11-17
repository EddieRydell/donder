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
        ProjectGuiEdit::AddConsole {
            initial_color,
            storage,
        } => {
            let color = super::model::parse_color(&initial_color)?;
            let id = donder_model::add_console(&mut session.project, NEW_CONSOLE_FRAME_RATE, color)
                .map_err(super::GuiMutationError::from)?;
            // `add_console` appends the console inline under a fresh name.
            let name = session
                .project
                .root()
                .consoles
                .last()
                .and_then(donder_model::OwnedMember::name)
                .map(|name| name.as_str().to_string())
                .ok_or_else(|| {
                    super::GuiMutationError::Invalid("The new console is missing.".into())
                })?;
            super::ownership::store_new_member(
                session,
                resolved,
                resolved.object,
                id.into(),
                &name,
                storage,
            )?;
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
    use crate::dto::{GuiField, GuiProblemSite};
    let duration = super::checked_gui_duration(request.duration_seconds)
        .map_err(|error| error.at(GuiProblemSite::object(GuiField::Duration)))?;
    let color = super::model::parse_color(&request.initial_color)?;
    let id = donder_model::add_sequence(&mut session.project, duration, request.frame_rate, color)
        .map_err(GuiMutationError::from)?;
    let project = session.project.root().id.into();
    super::ownership::store_new_member(
        session,
        owner,
        project,
        id.into(),
        &request.name,
        request.storage,
    )
    .map_err(|error| error.at(GuiProblemSite::object(GuiField::Name)))?;
    Ok(object_ref(session, id))
}

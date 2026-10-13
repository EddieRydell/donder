use super::patch::object_ref;
use donder_project_io::{ProjectSession, SourceObjectKind};

use super::{ResolvedGuiObject, blocked};
use donder_sequence_api::{GuiDocument, ProjectGuiDocument, ProjectGuiEdit};

/// A new rig's frame rate: Perform's target rate.
const NEW_RIG_FRAME_RATE: u32 = 100;

pub(super) fn edit_project(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: ProjectGuiEdit,
) -> Result<(), super::GuiMutationError> {
    if session.project.root().id.0 != resolved.identity {
        return Err(super::GuiMutationError::Invalid(
            "The requested project is not the active project.".into(),
        ));
    }
    match edit {
        ProjectGuiEdit::AddRig { initial_color } => {
            let color = super::model::parse_color(&initial_color)?;
            donder_model::add_rig(&mut session.project, NEW_RIG_FRAME_RATE, color)
                .map_err(super::GuiMutationError::Invalid)?;
        }
    }
    Ok(())
}

pub(super) fn project_root(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    if session.project.root().id.0 != resolved.identity {
        return blocked("The requested project is missing.", Vec::new());
    }

    GuiDocument::Project {
        document: ProjectGuiDocument {
            description: session.project.root().description.clone(),
            available_sources: super::ownership::available_sources(
                session,
                resolved.identity.document_id(),
                &[
                    SourceObjectKind::Setup,
                    SourceObjectKind::Sequence,
                    SourceObjectKind::Rig,
                ],
            ),
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            setup: object_ref(
                &session.project.root().setup.id().0,
                SourceObjectKind::Setup,
            ),
            sequences: session
                .project
                .root()
                .sequences
                .iter()
                .map(|sequence| object_ref(&sequence.id().0, SourceObjectKind::Sequence))
                .collect(),
            rigs: session
                .project
                .root()
                .rigs
                .iter()
                .map(|rig| object_ref(&rig.id().0, SourceObjectKind::Rig))
                .collect(),
        },
    }
}

pub fn create_sequence(
    session: &mut ProjectSession,
    owner: &super::ResolvedGuiObject,
    request: donder_sequence_api::NewSequenceRequest,
) -> Result<donder_sequence_api::GuiObjectRef, super::GuiMutationError> {
    use super::GuiMutationError;
    use donder_sequence_api::{
        GuiOwnershipEdit, GuiOwnershipSlot, NewSequenceStorage, ReusableStorage,
    };
    super::ensure_owned_gui_document(session, owner)?;
    let duration =
        std::time::Duration::try_from_secs_f32(request.duration_seconds).map_err(|_| {
            GuiMutationError::Invalid("Sequence duration is outside the supported range.".into())
        })?;
    let color = super::model::parse_color(&request.initial_color)?;
    let index = session.project.root().sequences.len();
    donder_model::add_sequence(
        &mut session.project,
        donder_language::DonderDuration(duration),
        request.frame_rate,
        color,
    )
    .map_err(GuiMutationError::Invalid)?;
    let reusable = match request.storage {
        NewSequenceStorage::Inline => None,
        NewSequenceStorage::SameFile { name } => Some((name, ReusableStorage::SameFile)),
        NewSequenceStorage::NewFile { name } => Some((name, ReusableStorage::NewFile)),
    };
    if let Some((name, storage)) = reusable {
        super::ownership::edit(
            session,
            owner,
            GuiOwnershipSlot::Sequence {
                index: index as u32,
            },
            GuiOwnershipEdit::MakeReusable { name, storage },
        )?;
    }
    Ok(super::patch::object_ref(
        &session.project.root().sequences[index].id().0,
        donder_project_io::SourceObjectKind::Sequence,
    ))
}

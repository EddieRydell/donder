use super::patch::object_ref;
use donder_project_io::{ProjectSession, SourceObjectKind};

use super::{ResolvedGuiObject, blocked};
use crate::dto::{GuiDocument, ProjectGuiDocument};

pub(super) fn project_root(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    if session.project.root().id.0 != resolved.identity {
        return blocked("The requested project is missing.", Vec::new());
    }

    GuiDocument::Project {
        document: ProjectGuiDocument {
            available_sources: super::ownership::available_sources(
                session,
                resolved.identity.document_id(),
                &[SourceObjectKind::Setup, SourceObjectKind::Sequence],
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
        },
    }
}

pub(crate) fn create_sequence(
    session: &mut ProjectSession,
    owner: &super::ResolvedGuiObject,
    request: crate::dto::NewSequenceRequest,
) -> Result<crate::dto::GuiObjectRef, super::GuiMutationError> {
    use super::GuiMutationError;
    use crate::dto::{GuiOwnershipEdit, GuiOwnershipSlot, NewSequenceStorage, ReusableStorage};
    super::ensure_owned_gui_document(session, owner)?;
    let duration =
        std::time::Duration::try_from_secs_f32(request.duration_seconds).map_err(|_| {
            GuiMutationError::Invalid("Sequence duration is outside the supported range.".into())
        })?;
    let color = super::model::parse_color(&request.initial_color)?;
    let index = session.project.root().sequences.len();
    donder_language::ownership::edit::add_sequence(
        &mut session.project,
        donder_language::values::DonderDuration(duration),
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

use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::{GuiOwnershipEdit, GuiOwnershipSlot, ReusableStorage};
use donder_model::{
    FixtureInstanceId, LayoutId, OwnershipSite, SetupId, make_independent, make_reusable,
    use_existing,
};
use donder_project_io::{ProjectSession, SourceObjectKind};

pub(super) fn edit(
    session: &mut ProjectSession,
    owner: &ResolvedGuiObject,
    slot: GuiOwnershipSlot,
    edit: GuiOwnershipEdit,
) -> Result<(), GuiMutationError> {
    if owner.kind == SourceObjectKind::Project && owner.identity != session.project.root().id.0 {
        return Err(GuiMutationError::Invalid(
            "The requested project is not the active project.".into(),
        ));
    }
    let identity = owner.object_identity();
    let (site, kind, directory) = match (&owner.kind, slot) {
        (SourceObjectKind::Project, GuiOwnershipSlot::Setup) => (
            OwnershipSite::ProjectSetup,
            SourceObjectKind::Setup,
            "setups",
        ),
        (SourceObjectKind::Project, GuiOwnershipSlot::Sequence { index }) => (
            OwnershipSite::ProjectSequence(index as usize),
            SourceObjectKind::Sequence,
            "sequences",
        ),
        (SourceObjectKind::Setup, GuiOwnershipSlot::Layout) => (
            OwnershipSite::SetupLayout(SetupId(identity)),
            SourceObjectKind::Layout,
            "layouts",
        ),
        (SourceObjectKind::Setup, GuiOwnershipSlot::Patch) => (
            OwnershipSite::SetupPatch(SetupId(identity)),
            SourceObjectKind::Patch,
            "patches",
        ),
        (SourceObjectKind::Setup, GuiOwnershipSlot::Controller { index }) => (
            OwnershipSite::SetupController {
                setup: SetupId(identity),
                index: index as usize,
            },
            SourceObjectKind::Controller,
            "controllers",
        ),
        (SourceObjectKind::Layout, GuiOwnershipSlot::Fixture { id }) => (
            OwnershipSite::LayoutFixture {
                layout: LayoutId(identity),
                fixture: FixtureInstanceId(id),
            },
            SourceObjectKind::FixtureDefinition,
            "fixtures",
        ),
        _ => {
            return Err(GuiMutationError::Invalid(
                "This ownership slot does not belong to the open object.".into(),
            ));
        }
    };
    match edit {
        GuiOwnershipEdit::UseExisting { source } => {
            if source.kind != crate::dto::ObjectKind::from(&kind) || !source.owned_path.is_empty() {
                return Err(GuiMutationError::Invalid(
                    "Choose a reusable source of the matching kind.".into(),
                ));
            }
            let identity = super::model::object_identity_from_gui(&source)?;
            let source = identity.source().ok_or_else(|| {
                GuiMutationError::Invalid("Owned values cannot be linked.".into())
            })?;
            donder_project_io::link_reusable_source(
                session,
                owner.identity.document_id(),
                kind,
                source,
            )
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            use_existing(&mut session.project, &site, source.clone())
                .map_err(GuiMutationError::Invalid)?;
        }
        GuiOwnershipEdit::MakeIndependent => {
            make_independent(&mut session.project, &site).map_err(GuiMutationError::Invalid)?;
        }
        GuiOwnershipEdit::MakeReusable { name, storage } => {
            if name.trim().is_empty() {
                return Err(GuiMutationError::Invalid(
                    "Enter a name for the reusable source.".into(),
                ));
            }
            let destination = match storage {
                ReusableStorage::SameFile => session
                    .source
                    .add_object(
                        owner.identity.document_id(),
                        kind,
                        donder_language::object_name(&name).as_str(),
                    )
                    .map_err(GuiMutationError::Invalid)?,
                ReusableStorage::NewFile => {
                    super::model::create_object_document(session, kind, &name, directory)?
                }
            };
            make_reusable(&mut session.project, &site, destination)
                .map_err(GuiMutationError::Invalid)?;
        }
    }
    donder_project_io::maintain_ownership_sources(session)
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))
}

pub(super) fn available_sources(
    session: &ProjectSession,
    owner: &donder_model::DocumentId,
    kinds: &[SourceObjectKind],
) -> Vec<crate::dto::GuiObjectRef> {
    donder_project_io::available_reusable_sources(session, owner, kinds)
        .into_iter()
        .map(|(kind, identity)| super::patch::object_ref(&identity.into(), kind))
        .collect()
}

use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::{GuiOwnershipEdit, OwnershipMember, ReusableStorage};
use donder_model::{
    FixtureInstanceId, ObjectId, ObjectKind, OwnershipSite, make_independent, make_reusable,
    use_existing,
};
use donder_project_io::ProjectSession;

pub(super) fn edit(
    session: &mut ProjectSession,
    owner: &ResolvedGuiObject,
    member: OwnershipMember,
    edit: GuiOwnershipEdit,
) -> Result<(), GuiMutationError> {
    if owner.kind == ObjectKind::Project
        && owner.object != ObjectId::from(session.project.root().id)
    {
        return Err(GuiMutationError::Invalid(
            "The requested project is not the active project.".into(),
        ));
    }
    let site = match member {
        OwnershipMember::Fixture { id } if owner.kind == ObjectKind::Layout => {
            OwnershipSite::Fixture {
                layout: owner.id(),
                fixture: FixtureInstanceId(id),
            }
        }
        OwnershipMember::Fixture { .. } => {
            return Err(GuiMutationError::Invalid(
                "Only a layout owns fixtures.".into(),
            ));
        }
        OwnershipMember::Object { source } => OwnershipSite::Member {
            owner: owner.object,
            member: super::model::object_id_from_gui(session, &source)?,
        },
    };
    edit_site(session, owner, site, edit)
}

pub(super) fn edit_site(
    session: &mut ProjectSession,
    owner: &ResolvedGuiObject,
    site: OwnershipSite,
    edit: GuiOwnershipEdit,
) -> Result<(), GuiMutationError> {
    let kind = match site {
        OwnershipSite::Member { member, .. } => member.kind(),
        OwnershipSite::Fixture { .. } => ObjectKind::FixtureDefinition,
    };
    match edit {
        GuiOwnershipEdit::UseExisting { declaration } => {
            if declaration.kind != crate::dto::ObjectKind::from(kind)
                || !declaration.owned_path.is_empty()
            {
                return Err(GuiMutationError::Invalid(
                    "Choose a reusable declaration of the matching kind.".into(),
                ));
            }
            let id: ObjectId = super::model::declared_from_gui(
                session,
                &declaration.path,
                &declaration.object_key,
            )?;
            if id.kind() != kind {
                return Err(GuiMutationError::Invalid(
                    "Choose a reusable declaration of the matching kind.".into(),
                ));
            }
            donder_project_io::link_declaration(session, owner.document_id(), id)
                .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            use_existing(&mut session.project, &site, id, &|id| {
                session.source.declared_name(id)
            })
            .map_err(GuiMutationError::from)?;
        }
        GuiOwnershipEdit::MakeIndependent => {
            make_independent(&mut session.project, &site, &|id| {
                session.source.declared_name(id)
            })
            .map_err(GuiMutationError::from)?;
        }
        GuiOwnershipEdit::MakeReusable { name, storage } => {
            if name.trim().is_empty() {
                return Err(GuiMutationError::Invalid(
                    "Enter a name for the reusable declaration.".into(),
                ));
            }
            let id = make_reusable(&mut session.project, &site).map_err(GuiMutationError::from)?;
            match storage {
                ReusableStorage::SameFile => {
                    session
                        .source
                        .add_object(
                            owner.document_id(),
                            id,
                            donder_language::object_name(&name).as_str(),
                        )
                        .map_err(GuiMutationError::Invalid)?;
                }
                ReusableStorage::NewFile => {
                    super::model::create_object_document(session, id, &name)?;
                }
            }
        }
    }
    donder_project_io::maintain_ownership_sources(session)
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))
}

pub(super) fn available_declarations(
    session: &ProjectSession,
    kinds: &[ObjectKind],
) -> Vec<crate::dto::GuiObjectRef> {
    donder_project_io::available_declarations(session, kinds)
        .into_iter()
        .map(|id| super::model::object_ref(session, id))
        .collect()
}

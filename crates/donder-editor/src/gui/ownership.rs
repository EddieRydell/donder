//! The members an object holds: a new member gets a file of its own, and the
//! object that holds a member renames or deletes it.
use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::GuiObjectRef;
use donder_model::{ObjectId, ObjectKind, OwnershipSite, make_reusable};
use donder_project_io::{DocumentId, ProjectSession};

/// Moves `member`, just added inside `owner`, to a file of its own that
/// declares it as `name`.
pub(super) fn store_new_member(
    session: &mut ProjectSession,
    owner: ObjectId,
    member: ObjectId,
    name: &str,
) -> Result<(), GuiMutationError> {
    let id = make_reusable(
        &mut session.project,
        &OwnershipSite::Member { owner, member },
    )
    .map_err(GuiMutationError::from)?;
    super::model::create_object_document(session, id, name)?;
    donder_project_io::maintain_ownership_sources(session)
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))
}

/// The member of `owner` that `source` names.
fn member(
    session: &ProjectSession,
    owner: &ResolvedGuiObject,
    source: &GuiObjectRef,
) -> Result<ObjectId, GuiMutationError> {
    if !matches!(owner.kind, ObjectKind::Project | ObjectKind::Setup) {
        return Err(GuiMutationError::Invalid(
            "Only a project or setup renames and deletes its members.".into(),
        ));
    }
    super::model::object_id_from_gui(session, source)
}

/// The document that declares `id` and nothing else.
fn own_document(session: &ProjectSession, id: ObjectId) -> Option<DocumentId> {
    let (document, _) = session.declaration(id)?;
    (session.source.documents[document].objects().len() == 1).then(|| document.clone())
}

/// Where the file of `source` moves when it is renamed `name`: a member
/// alone in its file keeps the file named after it. None when it has no file
/// of its own or the file already has that name.
pub fn member_file_rename(
    session: &ProjectSession,
    owner: &ResolvedGuiObject,
    source: &GuiObjectRef,
    name: &str,
) -> Result<Option<(String, String)>, GuiMutationError> {
    let id = member(session, owner, source)?;
    let Some(document) = own_document(session, id) else {
        return Ok(None);
    };
    let to = donder_project_io::object_document_path(id.kind(), &super::model::typed_name(name)?);
    if to == document.path() {
        return Ok(None);
    }
    if session.source.has_document_at(&to) {
        return Err(GuiMutationError::Invalid(format!(
            "The file {to} already exists."
        )));
    }
    Ok(Some((document.path().to_string(), to.to_string())))
}

/// Where a refused rename of a member shows: the name being chosen.
pub fn member_name_site() -> crate::dto::GuiProblemSite {
    crate::dto::GuiProblemSite::object(crate::dto::GuiField::Name)
}

/// Name `source`, a member of `owner`, `name`. A member stored inside its
/// owner moves to a file of its own by that name.
pub fn rename_member(
    session: &mut ProjectSession,
    owner: &ResolvedGuiObject,
    source: &GuiObjectRef,
    name: &str,
) -> Result<(), GuiMutationError> {
    let id = member(session, owner, source)?;
    if source.owned_path.is_empty() {
        session
            .source
            .rename_object(id, super::model::typed_name(name)?)
            .map_err(GuiMutationError::Invalid)
    } else {
        store_new_member(session, owner.object, id, name)
    }
}

/// A member removed from the project: what it was called, and the document
/// it alone was declared in, which the caller deletes.
pub struct DeletedMember {
    pub name: String,
    pub document: Option<DocumentId>,
}

/// Remove `source`, a member of `owner`, from the project, and what refers
/// to it gives way.
pub fn delete_member(
    session: &mut ProjectSession,
    owner: &ResolvedGuiObject,
    source: &GuiObjectRef,
) -> Result<DeletedMember, GuiMutationError> {
    let id = member(session, owner, source)?;
    let name = session
        .address_of(id)
        .map(|address| address.declared_name().to_string())
        .ok_or_else(|| GuiMutationError::Invalid("The member is missing.".into()))?;
    if matches!(
        id.kind(),
        ObjectKind::Setup | ObjectKind::Layout | ObjectKind::Patch
    ) {
        return Err(GuiMutationError::Invalid(format!(
            "A project needs its {}.",
            id.kind()
        )));
    }
    if let Some(document) = own_document(session, id) {
        donder_project_io::remove_documents(session, std::slice::from_ref(&document))
            .map_err(GuiMutationError::from)?;
        return Ok(DeletedMember {
            name,
            document: Some(document),
        });
    }
    if !source.owned_path.is_empty() {
        // Removal applies to reusable objects, which keep their ids.
        make_reusable(
            &mut session.project,
            &OwnershipSite::Member {
                owner: owner.object,
                member: id,
            },
        )
        .map_err(GuiMutationError::from)?;
    }
    session
        .project
        .apply_edits([donder_model::ProjectEdit::Remove(id)])
        .map_err(GuiMutationError::from)?;
    session.source.remove_object(id);
    Ok(DeletedMember {
        name,
        document: None,
    })
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

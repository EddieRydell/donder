//! The objects of a project, each declared in a document: a new one gets a
//! file of its own, renaming one alone in its file renames the file, and
//! removing one deletes the file it alone was declared in.
use super::GuiMutationError;
use crate::dto::{GuiDocumentRequest, GuiEdit, GuiField, GuiValue};
use donder_model::ObjectId;
use donder_project_io::{DocumentId, ProjectSession};

/// Declare `object`, the new object `id`, in a file of its own named `name`.
pub(super) fn add_in_own_file<I: Into<ObjectId> + Copy, T>(
    session: &mut ProjectSession,
    id: I,
    object: T,
    name: &str,
) -> Result<(), GuiMutationError>
where
    (I, T): Into<donder_model::Object>,
{
    let name = super::model::typed_name(name)?;
    session.edit(|edit| {
        edit.add_object_document(id.into(), name)?;
        edit.project_mut().put((id, object))?;
        Ok(())
    })
}

/// The document that declares `id` and nothing else.
pub(super) fn own_document(session: &ProjectSession, id: ObjectId) -> Option<DocumentId> {
    let (document, _) = session.declaration(id)?;
    (session.source().documents()[document].objects().len() == 1).then(|| document.clone())
}

/// Where the file of the object `edit` renames moves first: an object alone
/// in its file keeps the file named after it. None when the edit renames no
/// object with a file of its own, or the file already has that name.
pub fn renamed_file(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
    edit: &GuiEdit,
) -> Result<Option<(String, String)>, GuiMutationError> {
    let GuiEdit::Put {
        sites,
        value: GuiValue::Text { text },
    } = edit
    else {
        return Ok(None);
    };
    let [site] = sites.as_slice() else {
        return Ok(None);
    };
    if site.member.is_some() || site.field != Some(GuiField::Name) {
        return Ok(None);
    }
    let id = match &site.object {
        Some(object) => super::model::object_id_from_gui(session, object)?,
        None => {
            super::resolve_request(session, request)
                .map_err(GuiMutationError::Invalid)?
                .object
        }
    };
    let Some(document) = own_document(session, id) else {
        return Ok(None);
    };
    let to = donder_project_io::object_document_path(id.kind(), &super::model::typed_name(text)?);
    if to == document.path() {
        return Ok(None);
    }
    if session.source().has_document_at(&to) {
        return Err(GuiMutationError::Invalid(format!(
            "The file {to} already exists."
        )));
    }
    Ok(Some((document.path().to_string(), to.to_string())))
}

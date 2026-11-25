//! The canonical text of a project-owned data document, from typed state.
use super::encode::{
    Encoder, controller_document, curve_document, fixture_definition_document, gradient_document,
    input_device_document,
};
use super::text_cache::DocumentTextCache;
use super::{Declaration, write};
use crate::ExportProjectError;
use crate::address::{DeclaredObject, DocumentId};
use crate::source::{ProjectSession, SourceDocument};
use donder_language::data::NO_SPAN;
use donder_language::data::{DataImport, Spanned};
use donder_model::ObjectId;
use donder_runtime_types::Identifier;

pub(crate) fn data_document_text(
    session: &ProjectSession,
    document_id: &DocumentId,
    document: &SourceDocument,
    cache: &mut DocumentTextCache,
) -> Result<String, ExportProjectError> {
    cache.begin(&session.project);
    let addresses = session.addresses();
    let encoder = Encoder {
        session,
        addresses: &addresses,
        document: document_id,
    };
    let imports = document
        .imports()
        .iter()
        .map(|import| {
            let alias = Identifier::new(import.alias().to_string()).map_err(|_| {
                ExportProjectError::InvalidReference {
                    path: document_id.path().to_path_buf(),
                    reference: import.alias().to_string(),
                    message: "invalid import alias".to_string(),
                }
            })?;
            Ok(DataImport {
                alias: Spanned::new(alias, NO_SPAN),
                paths: import
                    .targets()
                    .iter()
                    .map(|target| Spanned::new(target.path().to_string(), NO_SPAN))
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, ExportProjectError>>()?;
    let declarations = document
        .objects()
        .iter()
        .map(|object| Ok((object.name().clone(), declaration(&encoder, object)?)))
        .collect::<Result<Vec<_>, ExportProjectError>>()?;
    // A sequence's clips are printed one by one and reused while unchanged.
    let clips = document
        .objects()
        .iter()
        .enumerate()
        .filter_map(|(index, object)| match object.id() {
            ObjectId::Sequence(id) => Some((index, id)),
            _ => None,
        })
        .map(|(index, id)| {
            let sequence = declared::<donder_model::Sequence>(session, id);
            Ok((index, cache.clips(&encoder, &imports, id, sequence)?))
        })
        .collect::<Result<Vec<_>, ExportProjectError>>()?;
    let items = clips
        .iter()
        .map(|(_, texts)| texts.iter().map(AsRef::as_ref).collect::<Vec<&str>>())
        .collect::<Vec<_>>();
    let splices = clips
        .iter()
        .zip(&items)
        .map(
            |((declaration, _), items)| donder_language::data::ListSplice {
                declaration: *declaration,
                field: "clips",
                items,
            },
        )
        .collect::<Vec<_>>();
    Ok(write(imports, &declarations, &splices))
}

/// The object a declaration of `session` writes.
fn declared<T: donder_model::ProjectObject>(session: &ProjectSession, id: T::Id) -> &T {
    session
        .project
        .reusable::<T>()
        .get(&id)
        .unwrap_or_else(|| unreachable!("a session declares exactly its objects"))
}

fn declaration(
    encoder: &Encoder<'_>,
    object: &DeclaredObject,
) -> Result<Declaration, ExportProjectError> {
    let session = encoder.session;
    Ok(match object.id() {
        ObjectId::Project(_) => Declaration::Project(encoder.project()?),
        ObjectId::Controller(id) => {
            Declaration::Controller(controller_document(declared(session, id)))
        }
        ObjectId::InputDevice(id) => {
            Declaration::InputDevice(input_device_document(declared(session, id)))
        }
        ObjectId::Layout(id) => Declaration::Layout(encoder.layout(declared(session, id))?),
        ObjectId::FixtureDefinition(id) => {
            Declaration::FixtureDefinition(fixture_definition_document(declared(session, id)))
        }
        ObjectId::Sequence(id) => Declaration::Sequence(encoder.sequence(declared(session, id))?),
        ObjectId::Console(id) => Declaration::Console(encoder.console(declared(session, id))?),
        ObjectId::Curve(id) => Declaration::Curve(curve_document(declared(session, id))),
        ObjectId::Gradient(id) => Declaration::Gradient(gradient_document(declared(session, id))),
        ObjectId::Script(_) => {
            unreachable!("a data document declares no scripts")
        }
    })
}

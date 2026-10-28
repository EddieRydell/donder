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
            let documents = import.documents();
            Ok(DataImport {
                alias: Spanned::new(alias, NO_SPAN),
                paths: documents
                    .iter()
                    .map(|path| Spanned::new(path.to_string(), NO_SPAN))
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, ExportProjectError>>()?;
    let declarations = document
        .objects()
        .iter()
        .map(|object| {
            Ok((
                object.name().clone(),
                declaration(&encoder, document_id, object)?,
            ))
        })
        .collect::<Result<Vec<_>, ExportProjectError>>()?;
    // A sequence's clips are printed one by one and reused while unchanged.
    let clips = document
        .objects()
        .iter()
        .enumerate()
        .filter_map(|(index, object)| match object.id() {
            ObjectId::Sequence(id) => Some((index, object, id)),
            _ => None,
        })
        .map(|(index, object, id)| {
            let sequence = session
                .project
                .reusable::<donder_model::Sequence>()
                .get(&id)
                .ok_or_else(|| missing(document_id, object))?;
            Ok((index, cache.clips(&encoder, &imports, sequence)?))
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

fn missing(document: &DocumentId, object: &DeclaredObject) -> ExportProjectError {
    crate::serialization::missing_typed_object(document, object)
}

fn declaration(
    encoder: &Encoder<'_>,
    document: &DocumentId,
    object: &DeclaredObject,
) -> Result<Declaration, ExportProjectError> {
    let project = &encoder.session.project;
    let missing = || missing(document, object);
    Ok(match object.id() {
        ObjectId::Project(_) => Declaration::Project(encoder.project()?),
        ObjectId::Setup(id) => Declaration::Setup(
            encoder.setup(
                project
                    .reusable::<donder_model::Setup>()
                    .get(&id)
                    .ok_or_else(missing)?,
            )?,
        ),
        ObjectId::Controller(id) => Declaration::Controller(controller_document(
            project
                .reusable::<donder_model::Controller>()
                .get(&id)
                .ok_or_else(missing)?,
        )),
        ObjectId::InputDevice(id) => Declaration::InputDevice(input_device_document(
            project
                .reusable::<donder_model::InputDevice>()
                .get(&id)
                .ok_or_else(missing)?,
        )),
        ObjectId::Layout(id) => Declaration::Layout(
            encoder.layout(
                project
                    .reusable::<donder_model::Layout>()
                    .get(&id)
                    .ok_or_else(missing)?,
            )?,
        ),
        ObjectId::Patch(id) => Declaration::Patch(
            encoder.patch(
                project
                    .reusable::<donder_model::Patch>()
                    .get(&id)
                    .ok_or_else(missing)?,
            )?,
        ),
        ObjectId::FixtureDefinition(id) => {
            Declaration::FixtureDefinition(fixture_definition_document(
                project
                    .reusable::<donder_model::FixtureDefinition>()
                    .get(&id)
                    .ok_or_else(missing)?,
            ))
        }
        ObjectId::Sequence(id) => Declaration::Sequence(
            encoder.sequence(
                project
                    .reusable::<donder_model::Sequence>()
                    .get(&id)
                    .ok_or_else(missing)?,
            )?,
        ),
        ObjectId::Rig(id) => Declaration::Rig(
            encoder.rig(
                project
                    .reusable::<donder_model::Rig>()
                    .get(&id)
                    .ok_or_else(missing)?,
            )?,
        ),
        ObjectId::Curve(id) => Declaration::Curve(curve_document(
            project
                .reusable::<donder_model::CurveDefinition>()
                .get(&id)
                .ok_or_else(missing)?,
        )),
        ObjectId::Gradient(id) => Declaration::Gradient(gradient_document(
            project
                .reusable::<donder_model::GradientDefinition>()
                .get(&id)
                .ok_or_else(missing)?,
        )),
        ObjectId::EffectDefinition(_) | ObjectId::OperatorDefinition(_) => {
            return Err(ExportProjectError::InvalidReference {
                path: document.path().to_path_buf(),
                reference: object.name().as_str().to_string(),
                message: "scripts are not data declarations".to_string(),
            });
        }
    })
}

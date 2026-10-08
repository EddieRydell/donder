//! The canonical text of a project-owned data document, from typed state.
use super::encode::{
    Encoder, controller_document, curve_document, fixture_definition_document, gradient_document,
};
use super::{Declaration, write};
use crate::ExportProjectError;
use crate::source::{
    ImportSource, ProjectSession, SourceDocument, SourceObjectId, SourceObjectKind,
};
use donder_language::data::NO_SPAN;
use donder_language::data::{DataImport, Spanned};
use donder_model::ControllerId;
use donder_model::FixtureDefinitionId;
use donder_model::LayoutId;
use donder_model::PatchId;
use donder_model::SequenceId;
use donder_model::SetupId;
use donder_model::{CurveId, GradientId};
use donder_model::{DocumentId, SourceIdentity};
use donder_runtime_types::Identifier;

pub(crate) fn data_document_text(
    session: &ProjectSession,
    document_id: &DocumentId,
    document: &SourceDocument,
) -> Result<String, ExportProjectError> {
    let encoder = Encoder {
        session,
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
            let ImportSource::LocalDocuments { documents } = import.source();
            Ok(DataImport {
                alias: Spanned::new(alias, NO_SPAN),
                paths: documents
                    .iter()
                    .map(|path| Spanned::new(path.to_string(), NO_SPAN))
                    .collect(),
            })
        })
        .collect::<Result<_, ExportProjectError>>()?;
    let declarations = document
        .objects()
        .iter()
        .map(|object| {
            let name = Identifier::new(object.id().to_string())
                .map_err(|_| missing(document_id, object))?;
            Ok((name, declaration(&encoder, document_id, object)?))
        })
        .collect::<Result<Vec<_>, ExportProjectError>>()?;
    Ok(write(imports, &declarations))
}

fn missing(document: &DocumentId, object: &SourceObjectId) -> ExportProjectError {
    crate::serialization::missing_typed_object(document, object)
}

fn declaration(
    encoder: &Encoder<'_>,
    document: &DocumentId,
    object: &SourceObjectId,
) -> Result<Declaration, ExportProjectError> {
    let project = &encoder.session.project;
    let identity = SourceIdentity::from_document(document.clone(), object.id().to_string());
    let missing = || missing(document, object);
    Ok(match object.kind() {
        SourceObjectKind::Project => Declaration::Project(encoder.project()?),
        SourceObjectKind::Setup => Declaration::Setup(
            encoder.setup(
                project
                    .reusable_setups()
                    .get(&SetupId(identity.into()))
                    .ok_or_else(missing)?,
            )?,
        ),
        SourceObjectKind::Controller => Declaration::Controller(controller_document(
            project
                .reusable_controllers()
                .get(&ControllerId(identity.into()))
                .ok_or_else(missing)?,
        )),
        SourceObjectKind::Layout => Declaration::Layout(
            encoder.layout(
                project
                    .reusable_layouts()
                    .get(&LayoutId(identity.into()))
                    .ok_or_else(missing)?,
            )?,
        ),
        SourceObjectKind::Patch => Declaration::Patch(
            encoder.patch(
                project
                    .reusable_patches()
                    .get(&PatchId(identity.into()))
                    .ok_or_else(missing)?,
            )?,
        ),
        SourceObjectKind::FixtureDefinition => {
            Declaration::FixtureDefinition(fixture_definition_document(
                project
                    .definitions()
                    .fixtures
                    .definitions
                    .get(&FixtureDefinitionId(identity))
                    .ok_or_else(missing)?,
            ))
        }
        SourceObjectKind::Sequence => Declaration::Sequence(
            encoder.sequence(
                project
                    .reusable_sequences()
                    .get(&SequenceId(identity.into()))
                    .ok_or_else(missing)?,
            )?,
        ),
        SourceObjectKind::Curve => Declaration::Curve(curve_document(
            project
                .definitions()
                .curves
                .definitions
                .get(&CurveId(identity))
                .ok_or_else(missing)?,
        )),
        SourceObjectKind::Gradient => Declaration::Gradient(gradient_document(
            project
                .definitions()
                .gradients
                .definitions
                .get(&GradientId(identity))
                .ok_or_else(missing)?,
        )),
        SourceObjectKind::EffectDefinition
        | SourceObjectKind::OperatorDefinition
        | SourceObjectKind::EffectInstance => {
            return Err(ExportProjectError::InvalidReference {
                path: document.path().to_path_buf(),
                reference: object.id().to_string(),
                message: "scripts are not data declarations".to_string(),
            });
        }
    })
}

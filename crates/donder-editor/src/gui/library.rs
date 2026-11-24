use donder_model::{CurveId, GradientId};
use donder_project_io::ProjectSession;

use super::{GuiMutationError, ResolvedGuiObject, model};
use crate::dto::{
    CurveGuiDocument, GradientGuiDocument, GuiDocument, SequenceCurvePoint, SequenceGradient,
};

pub(super) fn project_curve(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let definition = resolved.view::<donder_model::CurveDefinition>(session);
    GuiDocument::Curve {
        document: CurveGuiDocument {
            description: definition.description.clone(),
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            points: model::curve_points(&definition.curve),
        },
    }
}

pub(super) fn project_gradient(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let definition = resolved.view::<donder_model::GradientDefinition>(session);
    GuiDocument::Gradient {
        document: GradientGuiDocument {
            description: definition.description.clone(),
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            gradient: model::gradient_dto(&definition.gradient),
        },
    }
}

pub(super) fn edit_curve(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    points: Vec<SequenceCurvePoint>,
) -> Result<(), GuiMutationError> {
    let mut definition = donder_model::CurveDefinition::clone(&resolved.view(session));
    definition.curve = model::curve_from_points(points);
    session
        .apply_edits([donder_model::ProjectEdit::Put(donder_model::Object::Curve(
            resolved.id::<CurveId>(),
            definition,
        ))])
        .map_err(GuiMutationError::from)
}

pub(super) fn edit_gradient(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    gradient: SequenceGradient,
) -> Result<(), GuiMutationError> {
    let mut definition = donder_model::GradientDefinition::clone(&resolved.view(session));
    definition.gradient = model::gradient_from_dto(gradient)?;
    session
        .apply_edits([donder_model::ProjectEdit::Put(
            donder_model::Object::Gradient(resolved.id::<GradientId>(), definition),
        )])
        .map_err(GuiMutationError::from)
}

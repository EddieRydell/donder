use donder_model::{CurveId, GradientId};
use donder_project_io::ProjectSession;

use super::{GuiMutationError, ResolvedGuiObject, blocked, model};
use donder_sequence_api::{
    CurveGuiDocument, GradientGuiDocument, GuiDocument, SequenceCurvePoint, SequenceGradientStop,
};

pub(super) fn project_curve(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let Some(definition) = session
        .project
        .definitions()
        .curves
        .get(&CurveId(resolved.identity.clone()))
    else {
        return blocked("Curve definition was not found.", Vec::new());
    };
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
    let Some(definition) = session
        .project
        .definitions()
        .gradients
        .get(&GradientId(resolved.identity.clone()))
    else {
        return blocked("Gradient definition was not found.", Vec::new());
    };
    GuiDocument::Gradient {
        document: GradientGuiDocument {
            description: definition.description.clone(),
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            stops: model::gradient_stops(&definition.gradient),
        },
    }
}

pub(super) fn edit_curve(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    points: Vec<SequenceCurvePoint>,
) -> Result<(), GuiMutationError> {
    let mut definition = session
        .project
        .definitions()
        .curves
        .definitions
        .get(&CurveId(resolved.identity.clone()))
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Curve definition was not found.".into()))?;
    let curve = model::curve_from_points(points);
    curve
        .validate()
        .map_err(|error| GuiMutationError::Invalid(format!("Invalid curve: {error:?}")))?;
    definition.curve = curve;
    session
        .project
        .apply_edits([donder_model::ProjectEdit::SetCurveDefinition {
            id: CurveId(resolved.identity.clone()),
            value: definition,
        }])
        .map_err(GuiMutationError::Invalid)
}

pub(super) fn edit_gradient(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    stops: Vec<SequenceGradientStop>,
) -> Result<(), GuiMutationError> {
    let mut definition = session
        .project
        .definitions()
        .gradients
        .definitions
        .get(&GradientId(resolved.identity.clone()))
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Gradient definition was not found.".into()))?;
    let gradient = model::gradient_from_stops(stops)?;
    gradient
        .validate()
        .map_err(|error| GuiMutationError::Invalid(format!("Invalid gradient: {error:?}")))?;
    definition.gradient = gradient;
    session
        .project
        .apply_edits([donder_model::ProjectEdit::SetGradientDefinition {
            id: GradientId(resolved.identity.clone()),
            value: definition,
        }])
        .map_err(GuiMutationError::Invalid)
}

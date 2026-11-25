pub(super) fn clip_mut(
    sequence: &mut donder_model::Sequence,
    id: u32,
) -> Result<&mut Clip, GuiMutationError> {
    sequence
        .composition
        .clips
        .iter_mut()
        .find(|clip| clip.id.0 == id)
        .map(std::sync::Arc::make_mut)
        .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".to_string()))
}

pub(super) fn graph_operator_from_gui(
    session: &ProjectSession,
    operator: &SequenceGraphOperator,
) -> Result<ScriptId, GuiMutationError> {
    declared_from_gui(
        session,
        &operator.path,
        identifier(&operator.object_key)?.as_str(),
    )
}

pub fn source_identity_from_gui(path: &str, object: &str) -> SourceIdentity {
    SourceIdentity::from_document(
        donder_project_io::DocumentId::new(Utf8PathBuf::from(path)),
        object.to_string(),
    )
}

/// The declared object a GUI path and key name, as the id its kind takes.
pub(crate) fn declared_from_gui<I: TryFrom<ObjectId>>(
    session: &ProjectSession,
    path: &str,
    object: &str,
) -> Result<I, GuiMutationError> {
    session
        .declared_id(&source_identity_from_gui(path, object))
        .and_then(|id| I::try_from(id).ok())
        .ok_or_else(|| GuiMutationError::Invalid(format!("{object} was not found.")))
}

/// The object a GUI reference addresses, as the id its kind takes.
pub fn object_id_from_gui<I: TryFrom<ObjectId>>(
    session: &ProjectSession,
    reference: &crate::dto::GuiObjectRef,
) -> Result<I, GuiMutationError> {
    session
        .declared_id(&source_identity_from_gui(
            &reference.path,
            &reference.object_key,
        ))
        .and_then(|id| I::try_from(id).ok())
        .ok_or_else(|| GuiMutationError::Invalid("The referenced object was not found.".into()))
}

pub(super) fn mark_collection_mut(
    sequence: &mut donder_model::Sequence,
    id: u32,
) -> Result<&mut MarkCollection, GuiMutationError> {
    sequence
        .composition
        .marks
        .iter_mut()
        .find(|collection| collection.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Mark collection was not found.".to_string()))
}

/// A name typed in the GUI, as an object name: `Porch Left` becomes
/// `porch_left`. Uniqueness is checked by the edited object's validation.
pub(super) fn typed_name(text: &str) -> Result<Identifier, GuiMutationError> {
    if !text
        .chars()
        .any(|character| character.is_ascii_alphanumeric())
    {
        return Err(GuiMutationError::Invalid(
            "Names need at least one letter or digit.".into(),
        ));
    }
    Ok(donder_language::object_name(text))
}

/// A fresh name from `text`, made unique against `taken`.
pub(super) fn fresh_name(text: &str, taken: impl Fn(&str) -> bool) -> Identifier {
    donder_language::unique_name(donder_language::object_name(text).as_str(), taken)
}

pub(super) fn identifier(value: &str) -> Result<Identifier, GuiMutationError> {
    Identifier::new(value.to_string())
        .map_err(|_| GuiMutationError::Invalid(format!("{value} is not a valid name.")))
}

pub(super) fn clip_scope(scope: SequenceClipScope) -> TargetScope {
    match scope {
        SequenceClipScope::PerFixture => TargetScope::PerFixture,
        SequenceClipScope::WholeTarget => TargetScope::WholeTarget,
    }
}

pub(super) fn scope_to_gui(scope: TargetScope) -> SequenceClipScope {
    match scope {
        TargetScope::PerFixture => SequenceClipScope::PerFixture,
        TargetScope::WholeTarget => SequenceClipScope::WholeTarget,
    }
}

/// A fixture or group of the project's layout.
pub(super) fn target_from_gui(target: FixtureTarget) -> FixtureId {
    FixtureId(target.fixture)
}

pub(super) fn effect_ref_from_gui(
    session: &ProjectSession,
    reference: SequenceEffectReference,
) -> Result<ScriptId, GuiMutationError> {
    declared_from_gui(session, &reference.path, &reference.effect_name)
}

/// The values a new invocation gives the `params` without defaults, starting
/// colors at `initial_color`.
pub(super) fn required_param_values(
    params: &[ParamDecl],
    initial_color: Color,
) -> Result<IndexMap<Identifier, ParamValue>, GuiMutationError> {
    params
        .iter()
        .filter(|param| param.default.is_none())
        .map(|param| {
            ParamValue::initial_for_param(param, initial_color)
                .map(|value| (param.name.clone(), value))
                .ok_or_else(|| {
                    GuiMutationError::Invalid(format!(
                        "Parameter {} needs a value.",
                        param.name.as_str()
                    ))
                })
        })
        .collect()
}

/// A chosen effect definition, with its name and the values a new clip of
/// it gives its parameters without defaults.
pub(super) fn chosen_effect(
    session: &ProjectSession,
    reference: SequenceEffectReference,
    initial_color: &str,
) -> Result<(ScriptId, String, IndexMap<Identifier, ParamValue>), GuiMutationError> {
    let definition = effect_ref_from_gui(session, reference)?;
    // The session's inventory declares exactly its project's objects.
    let effect = &session
        .project()
        .reusable::<donder_model::ScriptDefinition>()[&definition];
    let name = effect.name().to_string();
    let params = required_param_values(effect.params(), parse_color(initial_color)?)?;
    Ok((definition, name, params))
}

pub(crate) fn param_value_from_gui(
    session: &mut ProjectSession,
    owner: &DocumentId,
    value: SequenceParamValue,
) -> Result<ParamValue, GuiMutationError> {
    Ok(match value {
        SequenceParamValue::Int { value } => ParamValue::Int(value as i32),
        SequenceParamValue::Float { value } => ParamValue::Float(value),
        SequenceParamValue::Bool { value } => ParamValue::Bool(value),
        SequenceParamValue::Color { value } => ParamValue::Color(parse_color(&value)?),
        SequenceParamValue::Enum { value, fields } => ParamValue::Enum {
            option: identifier(&value)?,
            fields: fields
                .into_iter()
                .map(|field| {
                    Ok((
                        identifier(&field.name)?,
                        param_value_from_gui(session, owner, field.value)?,
                    ))
                })
                .collect::<Result<_, GuiMutationError>>()?,
        },
        SequenceParamValue::Marks { id } => ParamValue::Marks(id.map(MarkCollectionId)),
        SequenceParamValue::Text { value } => ParamValue::Text(value),
        SequenceParamValue::Font { value: None } => ParamValue::Font(None),
        SequenceParamValue::Font { value: Some(font) } => {
            let file = session
                .reference_asset(owner, &font.file)
                .map_err(|error| {
                    GuiMutationError::Invalid(format!("Font file {}: {error}", font.file))
                })?;
            ParamValue::Font(Some(donder_model::FontFace {
                file,
                height: font.height,
            }))
        }
        SequenceParamValue::Image { value: None } => ParamValue::Image(None),
        SequenceParamValue::Image { value: Some(image) } => {
            let file = session
                .reference_asset(owner, &image.file)
                .map_err(|error| {
                    GuiMutationError::Invalid(format!("Image file {}: {error}", image.file))
                })?;
            ParamValue::Image(Some(donder_model::ImageSource {
                file,
                size: image.size,
            }))
        }
        SequenceParamValue::Curve { value } => ParamValue::Curve(
            match library_identity(session, donder_model::ObjectKind::Curve, value.link)? {
                Some(id) => CurveValue::Linked(library_id(id)?),
                None => CurveValue::Literal(curve_from_points(value.points)),
            },
        ),
        SequenceParamValue::Gradient { value } => ParamValue::Gradient(
            match library_identity(session, donder_model::ObjectKind::Gradient, value.link)? {
                Some(id) => GradientValue::Linked(library_id(id)?),
                None => GradientValue::Literal(gradient_from_dto(value.gradient)?),
            },
        ),
        SequenceParamValue::Array { values } => ParamValue::Array(
            values
                .into_iter()
                .map(|value| param_value_from_gui(session, owner, value))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    })
}

fn library_identity(
    session: &ProjectSession,
    kind: donder_model::ObjectKind,
    link: SequenceLibraryLink,
) -> Result<Option<ObjectId>, GuiMutationError> {
    let SequenceLibraryLink::Library {
        path, object_key, ..
    } = link
    else {
        return Ok(None);
    };
    let id = session
        .declared_id(&source_identity_from_gui(&path, &object_key))
        .filter(|id| id.kind() == kind)
        .ok_or_else(|| GuiMutationError::Invalid(format!("{object_key} was not found.")))?;
    Ok(Some(id))
}

fn library_id<I: TryFrom<ObjectId>>(id: ObjectId) -> Result<I, GuiMutationError> {
    I::try_from(id).map_err(|_| GuiMutationError::Invalid("Choose a library resource.".into()))
}

pub(super) fn curve_from_points(points: Vec<SequenceCurvePoint>) -> Curve {
    Curve {
        points: points
            .into_iter()
            .map(|point| CurvePoint {
                position: point.time,
                value: point.value,
            })
            .collect(),
    }
}

pub(super) fn curve_points(curve: &Curve) -> Vec<SequenceCurvePoint> {
    curve
        .points
        .iter()
        .map(|point| SequenceCurvePoint {
            time: point.position,
            value: point.value,
        })
        .collect()
}

pub(super) fn gradient_dto(gradient: &Gradient) -> SequenceGradient {
    SequenceGradient {
        space: match gradient.space {
            GradientSpace::LinearRgb => SequenceGradientSpace::LinearRgb,
            GradientSpace::Hsv => SequenceGradientSpace::Hsv,
            GradientSpace::Oklab => SequenceGradientSpace::Oklab,
        },
        stops: gradient
            .stops
            .iter()
            .map(|stop| SequenceGradientStop {
                time: stop.position,
                value: stop.color.to_hex(),
            })
            .collect(),
    }
}

pub(super) fn gradient_from_dto(gradient: SequenceGradient) -> Result<Gradient, GuiMutationError> {
    Ok(Gradient {
        space: match gradient.space {
            SequenceGradientSpace::LinearRgb => GradientSpace::LinearRgb,
            SequenceGradientSpace::Hsv => GradientSpace::Hsv,
            SequenceGradientSpace::Oklab => GradientSpace::Oklab,
        },
        stops: gradient
            .stops
            .into_iter()
            .map(|stop| {
                Ok(GradientStop {
                    position: stop.time,
                    color: parse_color(&stop.value)?,
                })
            })
            .collect::<Result<Vec<_>, GuiMutationError>>()?,
    })
}

pub(super) fn parse_color(value: &str) -> Result<Color, GuiMutationError> {
    Color::from_hex(value)
        .ok_or_else(|| GuiMutationError::Invalid(format!("{value} is not a color.")))
}

pub(super) fn domain_point3_meters(point: Point3Meters) -> Point3 {
    Point3 {
        x: Distance::from_meters(point.x_meters),
        y: Distance::from_meters(point.y_meters),
        z: Distance::from_meters(point.z_meters),
    }
}

pub(super) fn rotation3_degrees(rotation: Rotation3Degrees) -> DomainRotation3 {
    DomainRotation3 {
        x: rotation.x_degrees,
        y: rotation.y_degrees,
        z: rotation.z_degrees,
    }
}

pub(super) fn scale3(scale: Scale3) -> DomainScale3 {
    DomainScale3 {
        x: scale.x,
        y: scale.y,
        z: scale.z,
    }
}

pub(crate) fn point3_meters(point: Point3) -> Point3Meters {
    Point3Meters {
        x_meters: point.x.as_meters_f32(),
        y_meters: point.y.as_meters_f32(),
        z_meters: point.z.as_meters_f32(),
    }
}

use camino::Utf8PathBuf;
use donder_language::compiler::ParamDecl;
use donder_language::{
    Distance, DonderDuration, DonderTime, Point3, Rotation3 as DomainRotation3,
    Scale3 as DomainScale3,
};
use donder_model::FixtureId;
use donder_model::ObjectId;
use donder_model::{Clip, CurveValue, GradientValue, ParamValue, ScriptId};
use donder_model::{MarkCollection, MarkCollectionId};
use donder_project_io::SourceIdentity;
use donder_project_io::{DocumentId, ProjectSession};
use donder_runtime_types::Identifier;
use donder_runtime_types::TargetScope;
use donder_runtime_types::{Color, Curve, CurvePoint, Gradient, GradientSpace, GradientStop};
use indexmap::IndexMap;

use super::GuiMutationError;
use crate::dto::{
    FixtureTarget, Point3Meters, Rotation3Degrees, Scale3, SequenceClipScope, SequenceCurvePoint,
    SequenceEffectReference, SequenceGradient, SequenceGradientSpace, SequenceGradientStop,
    SequenceGraphOperator, SequenceLibraryLink, SequenceParamValue,
};

/// The name a new object of `kind` is offered.
pub fn new_object_name(session: &ProjectSession, kind: donder_model::ObjectKind) -> String {
    session.source().new_object_name(kind).as_str().to_string()
}

/// The GUI's reference to an object of the session.
pub(crate) fn object_ref(
    session: &ProjectSession,
    id: impl Into<ObjectId>,
) -> crate::dto::GuiObjectRef {
    let id = id.into();
    let identity = session.address_of(id);
    crate::dto::GuiObjectRef {
        path: identity.document().to_string(),
        object_key: identity.object().to_string(),
        kind: crate::dto::ObjectKind::from(id.kind()),
    }
}

/// The document and name of a declared object of the session. Definitions
/// and library resources are always declared.
pub(crate) fn declared_identity(
    session: &ProjectSession,
    id: impl Into<ObjectId>,
) -> SourceIdentity {
    session
        .source_identity(id.into())
        .unwrap_or_else(|| unreachable!("definitions are declared"))
}

/// Drop points of an edited curve that cannot affect sampling: exact repeats
/// of the previous point and the interior of three or more points at one
/// position. A step needs only the first and last point at its position.
pub(super) fn collapse_coincident_points(curve: &mut Curve) {
    let mut points: Vec<CurvePoint> = Vec::with_capacity(curve.points.len());
    for point in curve.points.drain(..) {
        match points.as_mut_slice() {
            [.., previous] if previous == &point => {}
            [.., first, last]
                if first.position == point.position && last.position == point.position =>
            {
                *last = point;
            }
            _ => points.push(point),
        }
    }
    curve.points = points;
}

/// A sequence clip's start and duration: validation keeps a sequence's clips
/// timed.
pub(super) fn timed(clip: &Clip) -> (&DonderTime, &DonderDuration) {
    clip.timing()
        .unwrap_or_else(|| unreachable!("validation keeps a sequence's clips timed"))
}

pub(super) fn timed_mut(clip: &mut Clip) -> (&mut DonderTime, &mut DonderDuration) {
    match &mut clip.placement {
        donder_model::Placement::Timed { start, duration } => (start, duration),
        donder_model::Placement::Played(_) => {
            unreachable!("validation keeps a sequence's clips timed")
        }
    }
}

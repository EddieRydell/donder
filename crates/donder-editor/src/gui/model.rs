pub(super) fn clip_mut(
    sequence: &mut donder_model::Sequence,
    id: u32,
) -> Result<&mut Clip, GuiMutationError> {
    sequence
        .clips
        .iter_mut()
        .find(|clip| clip.id.0 == id)
        .map(std::sync::Arc::make_mut)
        .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".to_string()))
}

pub(super) fn graph_operator_from_gui(
    session: &ProjectSession,
    operator: &SequenceGraphOperator,
) -> Result<OperatorDefinitionId, GuiMutationError> {
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
        .ok_or_else(|| GuiMutationError::Invalid(format!("`{object}` was not found.")))
}

/// The object a GUI reference addresses, as the id its kind takes.
pub(crate) fn object_id_from_gui<I: TryFrom<ObjectId>>(
    session: &ProjectSession,
    reference: &crate::dto::GuiObjectRef,
) -> Result<I, GuiMutationError> {
    session
        .resolve_address(&object_identity_from_gui(reference)?)
        .and_then(|id| I::try_from(id).ok())
        .ok_or_else(|| GuiMutationError::Invalid("The referenced object was not found.".into()))
}

pub(super) fn mark_collection_mut<'a>(
    sequence: &'a mut donder_model::Sequence,
    key: &str,
) -> Result<&'a mut MarkCollection, GuiMutationError> {
    sequence
        .mark_collections
        .iter_mut()
        .find(|collection| collection.key.name.as_str() == key)
        .ok_or_else(|| GuiMutationError::Invalid("Mark collection was not found.".to_string()))
}

pub(super) fn automation_clip_mut(
    sequence: &mut donder_model::Sequence,
    id: u32,
) -> Result<&mut AutomationClip, GuiMutationError> {
    sequence
        .automation_clips
        .iter_mut()
        .find(|clip| clip.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Automation clip was not found.".to_string()))
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
        .map_err(|_| GuiMutationError::Invalid(format!("Invalid identifier `{value}`.")))
}

pub(super) fn clip_scope(scope: SequenceClipScope) -> ClipScope {
    match scope {
        SequenceClipScope::PerFixture => ClipScope::PerFixture,
        SequenceClipScope::WholeTarget => ClipScope::WholeTarget,
    }
}

pub(super) fn layout_target_to_clip_target(
    layout: &LayoutId,
    target: FixtureTarget,
) -> DomainFixtureTarget {
    DomainFixtureTarget {
        layout: *layout,
        fixture: FixtureInstanceId(target.fixture),
    }
}

pub(super) fn effect_ref_from_gui(
    session: &ProjectSession,
    reference: SequenceEffectReference,
) -> Result<EffectDefinitionId, GuiMutationError> {
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
                        "Parameter `{}` requires an explicit value.",
                        param.name.as_str()
                    ))
                })
        })
        .collect()
}

/// An effect definition `owner` may name, which it then imports, with its
/// name and the values a new clip of it gives its parameters without defaults.
pub(super) fn imported_effect(
    session: &mut ProjectSession,
    owner: &DocumentId,
    reference: SequenceEffectReference,
    initial_color: &str,
) -> Result<(EffectDefinitionId, String, IndexMap<Identifier, ParamValue>), GuiMutationError> {
    let definition = effect_ref_from_gui(session, reference)?;
    let effect = session
        .project
        .get::<donder_model::EffectDefinition>(definition)
        .ok_or_else(|| GuiMutationError::Invalid("Effect definition was not found.".to_string()))?;
    let name = effect.name().to_string();
    let params = required_param_values(effect.params(), parse_color(initial_color)?)?;
    donder_project_io::ensure_document_can_reference_object(session, owner, definition.into())
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    Ok((definition, name, params))
}

/// Whether a parameter value names the mark collection `key`, itself or in
/// a marks array.
pub(super) fn names_marks(value: &ParamValue, key: &MarkCollectionKey) -> bool {
    match value {
        ParamValue::Marks(reference) => reference.as_ref() == Some(key),
        ParamValue::Array(values) => values.iter().any(|value| names_marks(value, key)),
        _ => false,
    }
}

/// Point a parameter value's references to the mark collection `from`,
/// including a marks array's, at `to`.
pub(super) fn replace_marks(
    value: &mut ParamValue,
    from: &MarkCollectionKey,
    to: Option<&MarkCollectionKey>,
) {
    match value {
        ParamValue::Marks(reference) if reference.as_ref() == Some(from) => {
            *reference = to.cloned();
        }
        ParamValue::Array(values) => {
            for value in values {
                replace_marks(value, from, to);
            }
        }
        _ => {}
    }
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
        SequenceParamValue::Enum { value } => ParamValue::Enum(identifier(&value)?),
        SequenceParamValue::Marks { key } => ParamValue::Marks(
            key.map(|key| identifier(&key).map(|name| MarkCollectionKey { name }))
                .transpose()?,
        ),
        SequenceParamValue::Curve { value } => ParamValue::Curve(
            match library_identity(session, owner, donder_model::ObjectKind::Curve, value.link)? {
                Some(id) => OwnedCurve::Reference(library_id(id)?),
                None => OwnedCurve::Inline(curve_from_points(value.points)),
            },
        ),
        SequenceParamValue::Gradient { value } => ParamValue::Gradient(
            match library_identity(
                session,
                owner,
                donder_model::ObjectKind::Gradient,
                value.link,
            )? {
                Some(id) => OwnedGradient::Reference(library_id(id)?),
                None => OwnedGradient::Inline(gradient_from_dto(value.gradient)?),
            },
        ),
        SequenceParamValue::IntArray { values } => ParamValue::Array(
            values
                .into_iter()
                .map(|value| ParamValue::Int(value as i32))
                .collect(),
        ),
        SequenceParamValue::FloatArray { values } => {
            ParamValue::Array(values.into_iter().map(ParamValue::Float).collect())
        }
        SequenceParamValue::BoolArray { values } => {
            ParamValue::Array(values.into_iter().map(ParamValue::Bool).collect())
        }
        SequenceParamValue::ColorArray { values } => ParamValue::Array(
            values
                .into_iter()
                .map(|value| parse_color(&value).map(ParamValue::Color))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        SequenceParamValue::CurveArray { values } => ParamValue::Array(
            values
                .into_iter()
                .map(|value| {
                    param_value_from_gui(session, owner, SequenceParamValue::Curve { value })
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        SequenceParamValue::GradientArray { values } => ParamValue::Array(
            values
                .into_iter()
                .map(|value| {
                    param_value_from_gui(session, owner, SequenceParamValue::Gradient { value })
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    })
}

fn library_identity(
    session: &mut ProjectSession,
    owner: &DocumentId,
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
        .ok_or_else(|| GuiMutationError::Invalid(format!("`{object_key}` was not found.")))?;
    donder_project_io::link_declaration(session, owner, id)
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    Ok(Some(id))
}

fn library_id<I: TryFrom<ObjectId>>(id: ObjectId) -> Result<I, GuiMutationError> {
    I::try_from(id).map_err(|_| GuiMutationError::Invalid("Choose a library resource.".into()))
}

pub(super) fn automation_binding_value_at(
    clip: &AutomationClip,
    window: Window,
    mapping: &AutomationMapping,
    seconds: f64,
) -> Result<ParamValue, GuiMutationError> {
    automation_value_at(clip, window, mapping, seconds)
        .map(|value| match value {
            AutomationValue::Int(value) => ParamValue::Int(value),
            AutomationValue::Float(value) => ParamValue::Float(value),
            AutomationValue::Bool(value) => ParamValue::Bool(value),
            AutomationValue::Enum(value) => ParamValue::Enum(value.clone()),
            AutomationValue::Curve(value) => ParamValue::Curve(OwnedCurve::Inline(value)),
        })
        .ok_or_else(|| {
            GuiMutationError::Invalid("Enum automation mapping has no values.".to_string())
        })
}

pub(super) fn default_automation_curve() -> Curve {
    Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 1.0,
                value: 1.0,
            },
        ],
    }
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
        .ok_or_else(|| GuiMutationError::Invalid(format!("Invalid color `{value}`.")))
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
use donder_language::{Distance, Point3, Rotation3 as DomainRotation3, Scale3 as DomainScale3};
use donder_model::{
    AutomationClip, MarkCollection, MarkCollectionKey, Window, automation_value_at,
};
use donder_model::{Clip, ClipScope, EffectDefinitionId, OwnedCurve, OwnedGradient, ParamValue};
use donder_model::{FixtureInstanceId, FixtureTarget as DomainFixtureTarget, LayoutId};
use donder_model::{ObjectId, OperatorDefinitionId};
use donder_project_io::SourceIdentity;
use donder_project_io::{DocumentId, ProjectSession};
use donder_runtime_types::Identifier;
use donder_runtime_types::{AutomationMapping, AutomationValue};
use donder_runtime_types::{Color, Curve, CurvePoint, Gradient, GradientSpace, GradientStop};
use indexmap::IndexMap;

use super::GuiMutationError;
use crate::dto::{
    FixtureTarget, Point3Meters, Rotation3Degrees, Scale3, SequenceClipScope, SequenceCurvePoint,
    SequenceEffectReference, SequenceGradient, SequenceGradientSpace, SequenceGradientStop,
    SequenceGraphOperator, SequenceLibraryLink, SequenceParamValue,
};

pub(super) fn create_object_document(
    session: &mut ProjectSession,
    id: ObjectId,
    name: &str,
) -> Result<donder_project_io::SourceIdentity, GuiMutationError> {
    use donder_model::ObjectKind as Kind;
    let directory = match id.kind() {
        Kind::Project => "projects",
        Kind::Setup => "setups",
        Kind::Layout => "layouts",
        Kind::Patch => "patches",
        Kind::Controller => "controllers",
        Kind::InputDevice => "input_devices",
        Kind::Sequence => "sequences",
        Kind::Console => "consoles",
        Kind::FixtureDefinition => "fixtures",
        Kind::Curve => "curves",
        Kind::Gradient => "gradients",
        Kind::EffectDefinition => "effects",
        Kind::OperatorDefinition => "operators",
    };
    let path = |stem: &str| {
        camino::Utf8PathBuf::from(format!(
            "{directory}/{stem}{}",
            donder_language::data::DATA_DOCUMENT_SUFFIX
        ))
    };
    let stem = fresh_name(name, |stem| {
        let path = path(stem);
        session
            .source
            .documents
            .contains_key(&session.source.project_document(path.clone()))
            || session.source.project_root().join(&path).exists()
    });
    let document = session
        .source
        .add_data_document(path(stem.as_str()), vec![(id, stem.clone())])
        .map_err(GuiMutationError::Invalid)?;
    Ok(donder_project_io::SourceIdentity::from_document(
        document,
        stem.as_str().to_string(),
    ))
}

fn object_identity_from_gui(
    reference: &crate::dto::GuiObjectRef,
) -> Result<donder_project_io::ObjectAddress, GuiMutationError> {
    let root = source_identity_from_gui(&reference.path, &reference.object_key);
    reference.owned_path.iter().try_fold(
        root.into(),
        |address: donder_project_io::ObjectAddress, step| {
            Ok(address.owned(step.try_into().map_err(GuiMutationError::Invalid)?))
        },
    )
}

/// Where an object of the session is written. Every object of an accepted
/// session is declared or owned by a declared object.
pub(crate) fn object_address(
    session: &ProjectSession,
    id: impl Into<ObjectId>,
) -> donder_project_io::ObjectAddress {
    session
        .address_of(id.into())
        .unwrap_or_else(|| unreachable!("every object of a session has an address"))
}

/// The GUI's reference to an object of the session.
pub(crate) fn object_ref(
    session: &ProjectSession,
    id: impl Into<ObjectId>,
) -> crate::dto::GuiObjectRef {
    let id = id.into();
    address_ref(&object_address(session, id), id.kind())
}

/// The GUI's reference to the object of `kind` at `address`.
fn address_ref(
    address: &donder_project_io::ObjectAddress,
    kind: donder_model::ObjectKind,
) -> crate::dto::GuiObjectRef {
    let root = address.root_source();
    crate::dto::GuiObjectRef {
        owned_path: address.owned_path().iter().map(Into::into).collect(),
        path: root.document().to_string(),
        object_key: root.object().to_string(),
        kind: crate::dto::ObjectKind::from(kind),
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

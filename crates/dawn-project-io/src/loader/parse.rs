pub(crate) fn parse_automation_curve(
    path: &Utf8Path,
    value: &Value,
) -> Result<Curve, LoadProjectError> {
    parse_curve(path, value)
}

pub(crate) fn parse_sequence_layer(
    path: &Utf8Path,
    value: &Value,
) -> Result<SequenceLayer, LoadProjectError> {
    require_allowed_mapping_keys(path, value, &["id", "name", "color", "enabled"], "layer")?;
    Ok(SequenceLayer {
        id: SequenceLayerId(u32_field(path, value, "id")?),
        name: string_field(path, value, "name")?.to_string(),
        color: parse_color(string_field(path, value, "color")?).map_err(|error| {
            with_yaml_location(
                error,
                path,
                source_range_for_field_value(path, value, "color"),
            )
        })?,
        enabled: optional_field(value, "enabled")
            .map(|enabled| {
                enabled
                    .as_bool()
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "enabled"),
                        message: "layer enabled must be a bool".to_string(),
                    })
            })
            .transpose()?
            .unwrap_or(true),
    })
}

pub(crate) fn parse_automation_binding(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationBinding, LoadProjectError> {
    require_allowed_mapping_keys(path, value, &["target", "mapping"], "automation binding")?;
    let target = parse_automation_target(path, required_field(path, value, "target")?)?;
    Ok(AutomationBinding {
        target,
        mapping: parse_automation_mapping(path, required_field(path, value, "mapping")?)?,
    })
}

pub(crate) fn parse_detached_automation_binding(
    path: &Utf8Path,
    value: &Value,
) -> Result<DetachedAutomationBinding, LoadProjectError> {
    require_allowed_mapping_keys(
        path,
        value,
        &["target", "mapping", "reason"],
        "detached automation binding",
    )?;
    let target = parse_automation_target(path, required_field(path, value, "target")?)?;
    let reason = match string_field(path, value, "reason")? {
        "target_deleted" => AutomationDetachmentReason::TargetDeleted,
        "definition_changed" => AutomationDetachmentReason::DefinitionChanged,
        other => {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, "reason"),
                message: format!("unsupported automation detachment reason `{other}`"),
            });
        }
    };
    Ok(DetachedAutomationBinding {
        target,
        mapping: parse_automation_mapping(path, required_field(path, value, "mapping")?)?,
        reason,
    })
}

pub(crate) fn parse_automation_target(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationTarget, LoadProjectError> {
    require_allowed_mapping_keys(
        path,
        value,
        &["type", "effect_id", "node_id", "param"],
        "automation target",
    )?;
    Ok(match string_field(path, value, "type")? {
        "effect_param" => AutomationTarget::EffectParam {
            effect_id: EffectInstId(u32_field(path, value, "effect_id")?),
            param: parse_identifier_field(path, value, "param")?,
        },
        "composition_node_param" => AutomationTarget::CompositionNodeParam {
            node_id: CompositionGraphNodeId(u32_field(path, value, "node_id")?),
            param: parse_identifier_field(path, value, "param")?,
        },
        other => {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, "type"),
                message: format!("unsupported automation target `{other}`"),
            });
        }
    })
}

pub(crate) fn parse_automation_mapping(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationMapping, LoadProjectError> {
    require_allowed_mapping_keys(
        path,
        value,
        &["type", "min", "max", "values"],
        "automation mapping",
    )?;
    Ok(match string_field(path, value, "type")? {
        "float" => AutomationMapping::Float {
            min: f32_field(path, value, "min")?,
            max: f32_field(path, value, "max")?,
        },
        "int" => AutomationMapping::Int {
            min: i32_field(path, value, "min")?,
            max: i32_field(path, value, "max")?,
        },
        "bool" => AutomationMapping::Bool,
        "enum" => AutomationMapping::Enum {
            values: sequence_field(path, value, "values")?
                .into_iter()
                .map(|enum_value| {
                    Identifier::new(enum_value).map_err(|_| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "values"),
                        message: "enum automation values must be identifiers".to_string(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        },
        "curve" => AutomationMapping::Curve {
            min: f32_field(path, value, "min")?,
            max: f32_field(path, value, "max")?,
        },
        other => {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, "type"),
                message: format!("unsupported automation mapping `{other}`"),
            });
        }
    })
}

pub(crate) fn parse_identifier_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<Identifier, LoadProjectError> {
    let raw = string_field(path, value, key)?;
    Identifier::new(raw.to_string()).map_err(|_| LoadProjectError::InvalidDocument {
        path: path.to_path_buf(),
        range: source_range_for_field_value(path, value, key),
        message: format!("invalid identifier `{raw}`"),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ResolvedObject {
    Project(ProjectId),
    Setup(SetupId),
    Controller(ControllerId),
    ElementTree(ElementTreeId),
    PreviewLayout(PreviewLayoutId),
    Patch(PatchId),
    PropDefinition(PropDefinitionId),
    FixtureProfile(FixtureProfileId),
    Curve(CurveId),
    Gradient(GradientId),
    Sequence(SequenceId),
    EffectDefinition(EffectDefinitionId),
    OperatorDefinition(OperatorDefinitionId),
}

impl ResolvedObject {
    pub(crate) fn source_identity(&self) -> &SourceIdentity {
        match self {
            Self::Project(id) => &id.0,
            Self::Setup(id) => &id.0,
            Self::Controller(id) => &id.0,
            Self::ElementTree(id) => &id.0,
            Self::PreviewLayout(id) => &id.0,
            Self::Patch(id) => &id.0,
            Self::PropDefinition(id) => &id.0,
            Self::FixtureProfile(id) => &id.0,
            Self::Curve(id) => &id.0,
            Self::Gradient(id) => &id.0,
            Self::Sequence(id) => &id.0,
            Self::EffectDefinition(id) => &id.0,
            Self::OperatorDefinition(id) => &id.0,
        }
    }

    pub(crate) fn source_kind(&self) -> SourceObjectKind {
        match self {
            Self::Project(_) => SourceObjectKind::Project,
            Self::Setup(_) => SourceObjectKind::Setup,
            Self::Controller(_) => SourceObjectKind::Controller,
            Self::ElementTree(_) => SourceObjectKind::ElementTree,
            Self::PreviewLayout(_) => SourceObjectKind::PreviewLayout,
            Self::Patch(_) => SourceObjectKind::Patch,
            Self::PropDefinition(_) => SourceObjectKind::PropDefinition,
            Self::FixtureProfile(_) => SourceObjectKind::FixtureProfile,
            Self::Curve(_) => SourceObjectKind::Curve,
            Self::Gradient(_) => SourceObjectKind::Gradient,
            Self::Sequence(_) => SourceObjectKind::Sequence,
            Self::EffectDefinition(_) => SourceObjectKind::EffectDefinition,
            Self::OperatorDefinition(_) => SourceObjectKind::OperatorDefinition,
        }
    }

    pub(crate) fn id_string(&self) -> String {
        match self {
            Self::Project(id) => id.0.object().to_string(),
            Self::Setup(id) => id.0.object().to_string(),
            Self::Controller(id) => id.0.object().to_string(),
            Self::ElementTree(id) => id.0.object().to_string(),
            Self::PreviewLayout(id) => id.0.object().to_string(),
            Self::Patch(id) => id.0.object().to_string(),
            Self::PropDefinition(id) => id.0.object().to_string(),
            Self::FixtureProfile(id) => id.0.object().to_string(),
            Self::Curve(id) => id.0.object().to_string(),
            Self::Gradient(id) => id.0.object().to_string(),
            Self::Sequence(id) => id.0.object().to_string(),
            Self::EffectDefinition(id) => id.0.object().to_string(),
            Self::OperatorDefinition(id) => id.0.object().to_string(),
        }
    }
}

pub(crate) struct SourceObjectValue<'a> {
    pub(crate) key: String,
    pub(crate) value: &'a Value,
}

pub(crate) fn require_allowed_mapping_keys(
    path: &Utf8Path,
    value: &Value,
    allowed: &[&str],
    label: &str,
) -> Result<(), LoadProjectError> {
    let mapping = mapping(value).ok_or_else(|| LoadProjectError::InvalidDocument {
        path: path.to_path_buf(),
        range: source_range_for_value(path, value),
        message: format!("{label} must be a mapping"),
    })?;
    for key in mapping.keys() {
        let key = key
            .as_str()
            .ok_or_else(|| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: format!("{label} keys must be strings"),
            })?;
        if !allowed.contains(&key) {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, key),
                message: format!("{label} has an unknown field `{key}`"),
            });
        }
    }
    Ok(())
}

pub(crate) fn parse_mark_collection(
    path: &Utf8Path,
    value: &Value,
) -> Result<MarkCollection, LoadProjectError> {
    Ok(MarkCollection {
        key: MarkCollectionKey {
            name: string_field(path, value, "key")?.to_string(),
        },
        name: string_field(path, value, "name")?.to_string(),
        display_color: parse_color(string_field(path, value, "color")?).map_err(|error| {
            with_yaml_location(
                error,
                path,
                source_range_for_field_value(path, value, "color"),
            )
        })?,
        marks: sequence_values(path, value, "marks")?
            .iter()
            .map(|mark| {
                mark.as_str()
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "marks must be duration strings".to_string(),
                    })
                    .and_then(|duration| {
                        parse_duration_as_time(duration).map_err(|error| {
                            with_yaml_location(error, path, source_range_for_value(path, mark))
                        })
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub(crate) fn parse_effect_scope(
    path: &Utf8Path,
    value: &Value,
) -> Result<EffectScope, LoadProjectError> {
    match string_field(path, value, "scope")? {
        "per_fixture" => Ok(EffectScope::PerFixture),
        "whole_target" => Ok(EffectScope::WholeTarget),
        other => Err(LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, "scope"),
            message: format!("invalid effect scope `{other}`"),
        }),
    }
}

pub(crate) fn parse_graph_position(
    path: &Utf8Path,
    value: &Value,
) -> Result<GraphNodePosition, LoadProjectError> {
    Ok(GraphNodePosition {
        x: f32_field(path, value, "x")?,
        y: f32_field(path, value, "y")?,
    })
}

pub(crate) fn parse_graph_edge(
    path: &Utf8Path,
    value: &Value,
) -> Result<EffectGraphEdge, LoadProjectError> {
    Ok(EffectGraphEdge {
        from: CompositionGraphNodeId(u32_field(path, value, "from")?),
        from_port: GraphPortId(string_field(path, value, "from_port")?.to_string()),
        to: CompositionGraphNodeId(u32_field(path, value, "to")?),
        to_port: GraphPortId(string_field(path, value, "to_port")?.to_string()),
    })
}

pub(crate) fn parse_prop_definition(
    path: &Utf8Path,
    value: &Value,
) -> Result<PropDefinition, LoadProjectError> {
    require_allowed_mapping_keys(
        path,
        value,
        &["type", "bulb_diameter", "geometry"],
        "prop definition",
    )?;
    let bulb_diameter = f32_field(path, value, "bulb_diameter")?;
    let geometry_value = required_field(path, value, "geometry")?;
    let geometry_type = string_field(path, geometry_value, "type")?;
    let fields: &[&str] = match geometry_type {
        "points" => &["type", "points"],
        "lines" => &["type", "points", "point_count"],
        "arc" => &[
            "type",
            "center",
            "radius",
            "startDegrees",
            "endDegrees",
            "point_count",
        ],
        _ => &["type"],
    };
    if matches!(geometry_type, "points" | "lines" | "arc") {
        require_allowed_mapping_keys(path, geometry_value, fields, "prop geometry")?;
    }
    let geometry = match geometry_type {
        "points" => PropGeometry::Points {
            points: sequence_values(path, geometry_value, "points")?
                .iter()
                .map(|point| parse_point3(path, point))
                .collect::<Result<Vec<_>, _>>()?,
        },
        "lines" => PropGeometry::Lines {
            points: sequence_values(path, geometry_value, "points")?
                .iter()
                .map(|point| parse_point3(path, point))
                .collect::<Result<Vec<_>, _>>()?,
            point_count: u32_field(path, geometry_value, "point_count")?,
        },
        "arc" => PropGeometry::Arc {
            center: parse_point3(path, required_field(path, geometry_value, "center")?)?,
            radius: DistanceSpan::from_meters(f32_field(path, geometry_value, "radius")?),
            start_degrees: f32_field(path, geometry_value, "startDegrees")?,
            end_degrees: f32_field(path, geometry_value, "endDegrees")?,
            point_count: u32_field(path, geometry_value, "point_count")?,
        },
        other => {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, geometry_value, "type"),
                message: format!("unsupported prop geometry `{other}`"),
            });
        }
    };
    Ok(PropDefinition {
        bulb_radius: DistanceSpan::from_meters(bulb_diameter / 2.0),
        geometry,
    })
}

pub(crate) fn parse_curve(path: &Utf8Path, value: &Value) -> Result<Curve, LoadProjectError> {
    require_allowed_mapping_keys(path, value, &["type", "points", "curve"], "curve")?;
    let points = sequence_values(path, value, "points")?
        .iter()
        .map(|point| {
            let point: crate::schema::CurvePoint =
                crate::diagnostics::deserialize_yaml(path, point)?;
            Ok(CurvePoint {
                position: point.position,
                value: point.value,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let curve = Curve { points };
    curve
        .validate()
        .map_err(|error| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, "points"),
            message: format!("invalid curve: {error:?}"),
        })?;
    Ok(curve)
}

pub(crate) fn parse_gradient(path: &Utf8Path, value: &Value) -> Result<Gradient, LoadProjectError> {
    require_allowed_mapping_keys(path, value, &["type", "stops", "gradient"], "gradient")?;
    let stops = sequence_values(path, value, "stops")?
        .iter()
        .map(|stop| {
            let fields: crate::schema::GradientStop =
                crate::diagnostics::deserialize_yaml(path, stop)?;
            let color = parse_color(&fields.color).map_err(|error| {
                with_yaml_location(
                    error,
                    path,
                    source_range_for_field_value(path, stop, "color"),
                )
            })?;
            Ok(GradientStop {
                position: fields.position,
                color,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Gradient { stops })
}

pub(crate) fn parse_point3(path: &Utf8Path, value: &Value) -> Result<Point3, LoadProjectError> {
    require_allowed_mapping_keys(path, value, &["x", "y", "z"], "point")?;
    Ok(Point3 {
        x: Distance::from_meters(f32_field(path, value, "x")?),
        y: Distance::from_meters(f32_field(path, value, "y")?),
        z: Distance::from_meters(f32_field(path, value, "z")?),
    })
}

pub(crate) fn parse_rotation3(
    path: &Utf8Path,
    value: &Value,
) -> Result<Rotation3, LoadProjectError> {
    require_allowed_mapping_keys(path, value, &["x", "y", "z"], "rotation")?;
    Ok(Rotation3 {
        x: f32_field(path, value, "x")?,
        y: f32_field(path, value, "y")?,
        z: f32_field(path, value, "z")?,
    })
}

pub(crate) fn parse_scale3(path: &Utf8Path, value: &Value) -> Result<Scale3, LoadProjectError> {
    require_allowed_mapping_keys(path, value, &["x", "y", "z"], "scale")?;
    Ok(Scale3 {
        x: f32_field(path, value, "x")?,
        y: f32_field(path, value, "y")?,
        z: f32_field(path, value, "z")?,
    })
}

pub(crate) fn parse_duration(value: &str) -> Result<DawnDuration, LoadProjectError> {
    parse_microseconds(value).map(DawnDuration::from_micros)
}

pub(crate) fn parse_duration_as_time(value: &str) -> Result<DawnTime, LoadProjectError> {
    parse_microseconds(value).map(DawnTime::from_micros)
}

fn parse_microseconds(value: &str) -> Result<u64, LoadProjectError> {
    if value.starts_with('-') {
        return Err(LoadProjectError::InvalidDocument {
            path: Utf8PathBuf::from("<duration>"),
            range: None,
            message: format!("duration must not be negative: {value}"),
        });
    }
    let invalid = || LoadProjectError::InvalidDocument {
        path: Utf8PathBuf::from("<duration>"),
        range: None,
        message: format!("invalid microsecond duration: {value}"),
    };
    if !value.ends_with('s') {
        return Err(LoadProjectError::InvalidDocument {
            path: Utf8PathBuf::from("<duration>"),
            range: None,
            message: format!("duration must end in `s`: {value}"),
        });
    }
    let duration = dur::parse(value).map_err(|_| invalid())?;
    u64::try_from((duration.as_nanos() + 500) / 1_000).map_err(|_| invalid())
}

pub(crate) fn parse_color(value: &str) -> Result<Color, LoadProjectError> {
    Color::from_hex(value).ok_or_else(|| LoadProjectError::InvalidDocument {
        path: Utf8PathBuf::from("<color>"),
        range: None,
        message: format!("invalid color: {value}"),
    })
}

pub(crate) fn mapping(value: &Value) -> Option<&Mapping> {
    match value {
        Value::Mapping(mapping) => Some(mapping),
        _ => None,
    }
}

pub(crate) fn required_field<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a Value, LoadProjectError> {
    mapping(value)
        .and_then(|mapping| mapping.get(Value::String(key.to_string())))
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_value(path, value),
            message: format!("missing field `{key}`"),
        })
}

pub(crate) fn optional_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    mapping(value).and_then(|mapping| mapping.get(Value::String(key.to_string())))
}

pub(crate) fn optional_mapping<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<Option<&'a Mapping>, LoadProjectError> {
    optional_field(value, key)
        .map(|field| {
            mapping(field).ok_or_else(|| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, key),
                message: format!("field `{key}` must be a mapping"),
            })
        })
        .transpose()
}

pub(crate) fn optional_sequence<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<Option<&'a Vec<Value>>, LoadProjectError> {
    optional_field(value, key)
        .map(|field| {
            field
                .as_sequence()
                .ok_or_else(|| LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, key),
                    message: format!("field `{key}` must be a sequence"),
                })
        })
        .transpose()
}

pub(crate) fn sequence_values<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a Vec<Value>, LoadProjectError> {
    required_field(path, value, key)?
        .as_sequence()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a sequence"),
        })
}

pub(crate) fn sequence_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<Vec<String>, LoadProjectError> {
    sequence_values(path, value, key)?
        .iter()
        .map(|value| {
            value.as_str().map(ToString::to_string).ok_or_else(|| {
                LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_value(path, value),
                    message: format!("field `{key}` values must be strings"),
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()
}

pub(crate) fn string_field<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a str, LoadProjectError> {
    required_field(path, value, key)?
        .as_str()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a string"),
        })
}

pub(crate) fn optional_string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    optional_field(value, key).and_then(Value::as_str)
}

pub(crate) fn u32_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<u32, LoadProjectError> {
    required_field(path, value, key)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a u32"),
        })
}

pub(crate) fn usize_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<usize, LoadProjectError> {
    required_field(path, value, key)?
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a usize"),
        })
}

pub(crate) fn i32_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<i32, LoadProjectError> {
    required_field(path, value, key)?
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be an integer"),
        })
}

pub(crate) fn f32_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<f32, LoadProjectError> {
    required_field(path, value, key)?
        .as_f64()
        .map(|value| value as f32)
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a number"),
        })
}

pub(crate) fn bool_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<bool, LoadProjectError> {
    required_field(path, value, key)?
        .as_bool()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a bool"),
        })
}

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::controller::ControllerId;
use dawn_language::dsl::Identifier;
use dawn_language::effect::{CurveId, EffectDefinitionId, EffectInstId, EffectScope, GradientId};
use dawn_language::element::ElementTreeId;
use dawn_language::fixture_profile::FixtureProfileId;
use dawn_language::identity::SourceIdentity;
use dawn_language::model::ProjectId;
use dawn_language::operator::OperatorDefinitionId;
use dawn_language::patch::PatchId;
use dawn_language::preview::{PreviewLayoutId, PropDefinition, PropDefinitionId, PropGeometry};
use dawn_language::sequence::{
    AutomationBinding, AutomationDetachmentReason, AutomationMapping, AutomationTarget,
    CompositionGraphNodeId, DetachedAutomationBinding, EffectGraphEdge, GraphNodePosition,
    GraphPortId, MarkCollection, MarkCollectionKey, SequenceId, SequenceLayer, SequenceLayerId,
};
use dawn_language::setup::SetupId;
use dawn_language::values::{
    Color, Curve, CurvePoint, DawnDuration, DawnTime, Distance, DistanceSpan, Gradient,
    GradientStop, Point3, Rotation3, Scale3,
};
use yaml_serde::{Mapping, Value};

use crate::diagnostics::{
    source_range_for_field_value, source_range_for_value, with_yaml_location,
};
use crate::{LoadProjectError, SourceObjectKind};

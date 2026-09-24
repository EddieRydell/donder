use super::mapping::{MappingReader, parse_mapping};
use donder_language::fixture::FixtureDefinitionId;
use donder_language::layout::LayoutId;
pub(crate) fn parse_project_fields(
    path: &Utf8Path,
    value: &Value,
) -> Result<(String, Vec<String>), LoadProjectError> {
    parse_mapping(path, value, "project", |fields| {
        fields.string("type")?;
        Ok((
            fields.string("setup")?.to_owned(),
            fields.strings("sequences")?,
        ))
    })
}
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
    parse_mapping(path, value, "layer", |fields| {
        Ok(SequenceLayer {
            id: SequenceLayerId(fields.u32("id")?),
            name: fields.string("name")?.to_string(),
            color: parse_color(fields.string("color")?).map_err(|error| {
                with_yaml_location(
                    error,
                    path,
                    source_range_for_field_value(path, value, "color"),
                )
            })?,
            enabled: fields
                .optional("enabled")
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
    })
}

pub(crate) fn parse_automation_binding(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationBinding, LoadProjectError> {
    parse_mapping(path, value, "automation binding", |fields| {
        let target = parse_automation_target(path, fields.required("target")?)?;
        Ok(AutomationBinding {
            target,
            mapping: parse_automation_mapping(path, fields.required("mapping")?)?,
        })
    })
}

pub(crate) fn parse_detached_automation_binding(
    path: &Utf8Path,
    value: &Value,
) -> Result<DetachedAutomationBinding, LoadProjectError> {
    parse_mapping(path, value, "detached automation binding", |fields| {
        let target = parse_automation_target(path, fields.required("target")?)?;
        let reason = match fields.string("reason")? {
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
            mapping: parse_automation_mapping(path, fields.required("mapping")?)?,
            reason,
        })
    })
}

pub(crate) fn parse_automation_target(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationTarget, LoadProjectError> {
    parse_mapping(path, value, "automation target", |fields| {
        Ok(match fields.string("type")? {
            "effect_param" => AutomationTarget::EffectParam {
                effect_id: EffectInstId(fields.u32("effect_id")?),
                param: parse_identifier_field(path, value, fields, "param")?,
            },
            "composition_node_param" => AutomationTarget::CompositionNodeParam {
                node_id: CompositionGraphNodeId(fields.u32("node_id")?),
                param: parse_identifier_field(path, value, fields, "param")?,
            },
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, "type"),
                    message: format!("unsupported automation target `{other}`"),
                });
            }
        })
    })
}

pub(crate) fn parse_automation_mapping(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationMapping, LoadProjectError> {
    parse_mapping(path, value, "automation mapping", |fields| {
        Ok(match fields.string("type")? {
            "float" => AutomationMapping::Float {
                min: fields.f32("min")?,
                max: fields.f32("max")?,
            },
            "int" => AutomationMapping::Int {
                min: fields.i32("min")?,
                max: fields.i32("max")?,
            },
            "bool" => AutomationMapping::Bool,
            "enum" => AutomationMapping::Enum {
                values: fields
                    .strings("values")?
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
                min: fields.f32("min")?,
                max: fields.f32("max")?,
            },
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, "type"),
                    message: format!("unsupported automation mapping `{other}`"),
                });
            }
        })
    })
}

pub(crate) fn parse_identifier_field(
    path: &Utf8Path,
    value: &Value,
    fields: &MappingReader<'_>,
    key: &str,
) -> Result<Identifier, LoadProjectError> {
    let raw = fields.string(key)?;
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
    Layout(LayoutId),
    Patch(PatchId),
    FixtureDefinition(FixtureDefinitionId),
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
            Self::Layout(id) => &id.0,
            Self::Patch(id) => &id.0,
            Self::FixtureDefinition(id) => &id.0,
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
            Self::Layout(_) => SourceObjectKind::Layout,
            Self::Patch(_) => SourceObjectKind::Patch,
            Self::FixtureDefinition(_) => SourceObjectKind::FixtureDefinition,
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
            Self::Layout(id) => id.0.object().to_string(),
            Self::Patch(id) => id.0.object().to_string(),
            Self::FixtureDefinition(id) => id.0.object().to_string(),
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

pub(crate) fn parse_mark_collection(
    path: &Utf8Path,
    value: &Value,
) -> Result<MarkCollection, LoadProjectError> {
    parse_mapping(path, value, "mark collection", |fields| {
        Ok(MarkCollection {
            key: MarkCollectionKey {
                name: fields.string("key")?.to_string(),
            },
            name: fields.string("name")?.to_string(),
            display_color: parse_color(fields.string("color")?).map_err(|error| {
                with_yaml_location(
                    error,
                    path,
                    source_range_for_field_value(path, value, "color"),
                )
            })?,
            marks: fields
                .sequence("marks")?
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
    })
}

pub(crate) fn parse_effect_scope(
    path: &Utf8Path,
    value: &Value,
    fields: &MappingReader<'_>,
) -> Result<EffectScope, LoadProjectError> {
    match fields.string("scope")? {
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
    parse_mapping(path, value, "graph position", |fields| {
        Ok(GraphNodePosition {
            x: fields.f32("x")?,
            y: fields.f32("y")?,
        })
    })
}

pub(crate) fn parse_graph_edge(
    path: &Utf8Path,
    value: &Value,
) -> Result<EffectGraphEdge, LoadProjectError> {
    parse_mapping(path, value, "graph edge", |fields| {
        Ok(EffectGraphEdge {
            from: CompositionGraphNodeId(fields.u32("from")?),
            from_port: GraphPortId(fields.string("from_port")?.to_string()),
            to: CompositionGraphNodeId(fields.u32("to")?),
            to_port: GraphPortId(fields.string("to_port")?.to_string()),
        })
    })
}

pub(crate) fn parse_curve(path: &Utf8Path, value: &Value) -> Result<Curve, LoadProjectError> {
    parse_mapping(path, value, "curve", |fields| {
        parse_curve_fields(path, value, fields)
    })
}
pub(crate) fn parse_curve_fields(
    path: &Utf8Path,
    value: &Value,
    fields: &MappingReader<'_>,
) -> Result<Curve, LoadProjectError> {
    if let Some(kind) = fields.optional("type")
        && kind.as_str() != Some("curve")
    {
        return Err(LoadProjectError::InvalidDocument {
            path: path.to_owned(),
            range: source_range_for_value(path, kind),
            message: "invalid curve type".into(),
        });
    }
    let points = fields
        .sequence("points")?
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
    parse_mapping(path, value, "gradient", |fields| {
        parse_gradient_fields(path, fields)
    })
}
pub(crate) fn parse_gradient_fields(
    path: &Utf8Path,
    fields: &MappingReader<'_>,
) -> Result<Gradient, LoadProjectError> {
    if let Some(kind) = fields.optional("type")
        && kind.as_str() != Some("gradient")
    {
        return Err(LoadProjectError::InvalidDocument {
            path: path.to_owned(),
            range: source_range_for_value(path, kind),
            message: "invalid gradient type".into(),
        });
    }
    let stops = fields
        .sequence("stops")?
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
    parse_mapping(path, value, "point", |fields| {
        let distance = |axis| {
            let meters = fields.required(axis)?.as_f64().ok_or_else(|| {
                LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, axis),
                    message: "Coordinate must be a number.".into(),
                }
            })?;
            if !meters.is_finite() || meters.abs() > 2_000.0 {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, axis),
                    message: "Coordinates must be finite and within 2,000 meters of the origin."
                        .into(),
                });
            }
            Ok(Distance {
                micrometers: (meters * 1_000_000.0).round() as i32,
            })
        };
        Ok(Point3 {
            x: distance("x")?,
            y: distance("y")?,
            z: distance("z")?,
        })
    })
}

pub(crate) fn parse_rotation3(
    path: &Utf8Path,
    value: &Value,
) -> Result<Rotation3, LoadProjectError> {
    parse_mapping(path, value, "rotation", |fields| {
        Ok(Rotation3 {
            x: fields.f32("x")?,
            y: fields.f32("y")?,
            z: fields.f32("z")?,
        })
    })
}

pub(crate) fn parse_scale3(path: &Utf8Path, value: &Value) -> Result<Scale3, LoadProjectError> {
    parse_mapping(path, value, "scale", |fields| {
        Ok(Scale3 {
            x: fields.f32("x")?,
            y: fields.f32("y")?,
            z: fields.f32("z")?,
        })
    })
}

pub(crate) fn parse_duration(value: &str) -> Result<DonderDuration, LoadProjectError> {
    parse_microseconds(value).map(DonderDuration::from_micros)
}

pub(crate) fn parse_duration_as_time(value: &str) -> Result<DonderTime, LoadProjectError> {
    parse_microseconds(value).map(DonderTime::from_micros)
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

use camino::{Utf8Path, Utf8PathBuf};
use donder_language::controller::ControllerId;
use donder_language::dsl::Identifier;
use donder_language::effect::{CurveId, EffectDefinitionId, EffectInstId, EffectScope, GradientId};
use donder_language::identity::SourceIdentity;
use donder_language::model::ProjectId;
use donder_language::operator::OperatorDefinitionId;
use donder_language::patch::PatchId;
use donder_language::sequence::{
    AutomationBinding, AutomationDetachmentReason, AutomationMapping, AutomationTarget,
    CompositionGraphNodeId, DetachedAutomationBinding, EffectGraphEdge, GraphNodePosition,
    GraphPortId, MarkCollection, MarkCollectionKey, SequenceId, SequenceLayer, SequenceLayerId,
};
use donder_language::setup::SetupId;
use donder_language::values::{
    Color, Curve, CurvePoint, Distance, DonderDuration, DonderTime, Gradient, GradientStop, Point3,
    Rotation3, Scale3,
};
use yaml_serde::{Mapping, Value};

use crate::diagnostics::{
    source_range_for_field_value, source_range_for_value, with_yaml_location,
};
use crate::{LoadProjectError, SourceObjectKind};

#[cfg(test)]
mod strict_mapping_tests {
    use super::*;

    #[test]
    fn automation_mappings_only_accept_their_own_variant_fields() {
        let path = Utf8Path::new("test.donder");
        for (source, forbidden) in [
            ("{type: bool}", "min"),
            ("{type: float, min: 0, max: 1}", "values"),
            ("{type: int, min: 0, max: 1}", "values"),
            ("{type: curve, min: 0, max: 1}", "values"),
            ("{type: enum, values: [one, two]}", "max"),
        ] {
            let mut value: Value = yaml_serde::from_str(source).unwrap();
            parse_automation_mapping(path, &value).unwrap();
            value
                .as_mapping_mut()
                .unwrap()
                .insert(Value::String(forbidden.into()), Value::Null);
            let error = parse_automation_mapping(path, &value).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(&format!("unknown field `{forbidden}`")),
                "{error}"
            );
        }
    }

    #[test]
    fn automation_targets_only_accept_their_own_variant_fields() {
        let path = Utf8Path::new("test.donder");
        for (source, forbidden) in [
            (
                "{type: effect_param, effect_id: 1, param: level}",
                "node_id",
            ),
            (
                "{type: composition_node_param, node_id: 1, param: level}",
                "effect_id",
            ),
        ] {
            let mut value: Value = yaml_serde::from_str(source).unwrap();
            parse_automation_target(path, &value).unwrap();
            value
                .as_mapping_mut()
                .unwrap()
                .insert(Value::String(forbidden.into()), Value::Number(2.into()));
            let error = parse_automation_target(path, &value).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(&format!("unknown field `{forbidden}`")),
                "{error}"
            );
        }
    }
}

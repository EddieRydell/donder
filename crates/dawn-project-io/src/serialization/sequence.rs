pub(super) fn sequence_value(
    session: &ProjectSession,
    from_document: &DocumentId,
    sequence: &Sequence,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("sequence");
    value.insert(
        string_value("duration"),
        Value::String(microseconds_string(sequence.duration.as_micros_rounded())),
    );
    value.insert(
        string_value("frame_rate"),
        serialized_value(sequence.frame_rate)?,
    );
    match &sequence.audio {
        SequenceAudio::None => {
            value.insert(string_value("audio"), Value::Null);
        }
        SequenceAudio::Asset(id) => {
            let asset = session
                .source
                .referenced_assets
                .iter()
                .find(|asset| asset.id == *id)
                .ok_or_else(|| ExportProjectError::InvalidReference {
                    path: from_document.path().to_path_buf(),
                    reference: id.0.to_string(),
                    message: "sequence audio asset is missing from source metadata".to_string(),
                })?;
            value.insert(
                string_value("audio"),
                Value::String(asset.relative_path.to_string()),
            );
        }
    }
    value.insert(
        string_value("mark_collections"),
        Value::Sequence(
            sequence
                .mark_collections
                .iter()
                .map(mark_collection_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("layers"),
        Value::Sequence(
            sequence
                .layers
                .iter()
                .map(sequence_layer_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("effects"),
        Value::Sequence(
            sequence
                .effects
                .iter()
                .map(|effect| sequence_effect_value(session, from_document, effect))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("composition_graph"),
        composition_graph_value(session, from_document, &sequence.composition_graph)?,
    );
    value.insert(
        string_value("automation_clips"),
        Value::Sequence(
            sequence
                .automation_clips
                .iter()
                .map(automation_clip_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("control_clips"),
        Value::Sequence(
            sequence
                .control_clips
                .iter()
                .map(|clip| control_clip_value(session, from_document, clip))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn sequence_layer_value(layer: &SequenceLayer) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), serialized_value(layer.id.0)?);
    value.insert(string_value("name"), Value::String(layer.name.clone()));
    value.insert(string_value("color"), Value::String(layer.color.to_hex()));
    value.insert(string_value("enabled"), Value::Bool(layer.enabled));
    Ok(Value::Mapping(value))
}

pub(super) fn sequence_effect_value(
    session: &ProjectSession,
    from_document: &DocumentId,
    effect: &EffectInst,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), serialized_value(effect.id.0)?);
    value.insert(
        string_value("layer_id"),
        serialized_value(effect.layer_id.0)?,
    );
    value.insert(
        string_value("start"),
        Value::String(microseconds_string(effect.start.as_micros_rounded())),
    );
    value.insert(
        string_value("duration"),
        Value::String(microseconds_string(effect.duration.as_micros_rounded())),
    );
    value.insert(
        string_value("target"),
        element_selection_value(session, from_document, &effect.target)?,
    );
    value.insert(
        string_value("scope"),
        Value::String(
            match effect.scope {
                EffectScope::PerFixture => "per_fixture",
                EffectScope::WholeTarget => "whole_target",
            }
            .to_string(),
        ),
    );
    write_effect_fields(
        session,
        from_document,
        &mut value,
        &effect.definition,
        &effect.param_overrides,
    )?;
    Ok(Value::Mapping(value))
}

pub(super) fn composition_graph_value(
    session: &ProjectSession,
    from_document: &DocumentId,
    graph: &SequenceCompositionGraph,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("nodes"),
        Value::Sequence(
            graph
                .nodes
                .iter()
                .map(|node| composition_graph_node_value(session, from_document, node))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("edges"),
        Value::Sequence(
            graph
                .edges
                .iter()
                .map(graph_edge_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn composition_graph_node_value(
    session: &ProjectSession,
    from_document: &DocumentId,
    node: &CompositionGraphNode,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), serialized_value(node.id.0)?);
    value.insert(
        string_value("position"),
        graph_position_value(&node.position)?,
    );
    match &node.kind {
        CompositionGraphNodeKind::Layer { layer_id } => {
            value.insert(string_value("type"), Value::String("layer".to_string()));
            value.insert(string_value("layer_id"), serialized_value(layer_id.0)?);
        }
        CompositionGraphNodeKind::Operator(operator) => {
            value.insert(string_value("type"), Value::String("operator".to_string()));
            value.insert(
                string_value("operator"),
                Value::String(graph_operator_name(
                    session,
                    from_document,
                    &operator.operator,
                )?),
            );
            if !operator.params.is_empty() {
                value.insert(
                    string_value("params"),
                    Value::Mapping(
                        operator
                            .params
                            .iter()
                            .map(|(name, param)| {
                                Ok((
                                    string_value(name.as_str()),
                                    effect_param_value(session, from_document, param)?,
                                ))
                            })
                            .collect::<Result<Mapping, ExportProjectError>>()?,
                    ),
                );
            }
        }
        CompositionGraphNodeKind::Output => {
            value.insert(string_value("type"), Value::String("output".to_string()));
        }
    }
    Ok(Value::Mapping(value))
}

pub(super) fn mark_collection_value(
    collection: &MarkCollection,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("key"),
        Value::String(collection.key.name.clone()),
    );
    value.insert(string_value("name"), Value::String(collection.name.clone()));
    value.insert(
        string_value("color"),
        Value::String(collection.display_color.to_hex()),
    );
    value.insert(
        string_value("marks"),
        Value::Sequence(
            collection
                .marks
                .iter()
                .map(|time| Value::String(microseconds_string(time.as_micros_rounded())))
                .collect(),
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn write_effect_fields(
    session: &ProjectSession,
    from_document: &DocumentId,
    value: &mut Mapping,
    definition: &EffectRef,
    param_overrides: &IndexMap<Identifier, EffectParamValue>,
) -> Result<(), ExportProjectError> {
    if !param_overrides.is_empty() {
        value.insert(
            string_value("params"),
            Value::Mapping(
                param_overrides
                    .iter()
                    .map(|(name, param)| {
                        Ok((
                            string_value(name.as_str()),
                            effect_param_value(session, from_document, param)?,
                        ))
                    })
                    .collect::<Result<Mapping, ExportProjectError>>()?,
            ),
        );
    }
    let reference = crate::imports::write_effect_reference(session, from_document, definition)?;
    value.insert(string_value("effect"), Value::String(reference));
    Ok(())
}

pub(super) fn graph_position_value(
    position: &GraphNodePosition,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("x"), serialized_value(position.x)?);
    value.insert(string_value("y"), serialized_value(position.y)?);
    Ok(Value::Mapping(value))
}

pub(super) fn graph_edge_value(edge: &EffectGraphEdge) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("from"), serialized_value(edge.from.0)?);
    value.insert(
        string_value("from_port"),
        Value::String(edge.from_port.0.clone()),
    );
    value.insert(string_value("to"), serialized_value(edge.to.0)?);
    value.insert(
        string_value("to_port"),
        Value::String(edge.to_port.0.clone()),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn graph_operator_name(
    session: &ProjectSession,
    from_document: &DocumentId,
    operator: &OperatorRef,
) -> Result<String, ExportProjectError> {
    match operator {
        OperatorRef::Builtin(operator) => Ok(dawn_language::operator::builtin_operator_definition(
            *operator,
        )
        .source_name
        .clone()),
        OperatorRef::Custom(id) => write_source_reference(
            session,
            from_document,
            SourceObjectKind::OperatorDefinition,
            &id.0,
        ),
    }
}

pub(super) fn automation_clip_value(clip: &AutomationClip) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), serialized_value(clip.id.0)?);
    value.insert(
        string_value("start"),
        Value::String(microseconds_string(clip.start.as_micros_rounded())),
    );
    value.insert(
        string_value("duration"),
        Value::String(microseconds_string(clip.duration.as_micros_rounded())),
    );
    value.insert(
        string_value("anchor_lane_index"),
        serialized_value(clip.anchor_lane_index)?,
    );
    value.insert(
        string_value("lane_index"),
        serialized_value(clip.lane_index)?,
    );
    value.insert(string_value("curve"), automation_curve_value(&clip.curve)?);
    value.insert(
        string_value("bindings"),
        Value::Sequence(
            clip.bindings
                .iter()
                .map(automation_binding_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("detached_bindings"),
        Value::Sequence(
            clip.detached_bindings
                .iter()
                .map(detached_automation_binding_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn detached_automation_binding_value(
    binding: &DetachedAutomationBinding,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("target"),
        automation_target_value(&binding.target)?,
    );
    value.insert(
        string_value("mapping"),
        automation_mapping_value(&binding.mapping)?,
    );
    value.insert(
        string_value("reason"),
        Value::String(
            match binding.reason {
                AutomationDetachmentReason::TargetDeleted => "target_deleted",
                AutomationDetachmentReason::DefinitionChanged => "definition_changed",
            }
            .to_string(),
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn automation_curve_value(curve: &Curve) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("points"),
        Value::Sequence(
            curve
                .points
                .iter()
                .map(|point| {
                    let mut value = Mapping::new();
                    value.insert(string_value("position"), serialized_value(point.position)?);
                    value.insert(string_value("value"), serialized_value(point.value)?);
                    Ok(Value::Mapping(value))
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn automation_binding_value(
    binding: &AutomationBinding,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("target"),
        automation_target_value(&binding.target)?,
    );
    value.insert(
        string_value("mapping"),
        automation_mapping_value(&binding.mapping)?,
    );
    Ok(Value::Mapping(value))
}

pub(super) fn automation_target_value(
    target: &AutomationTarget,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match target {
        AutomationTarget::EffectParam { effect_id, param } => {
            value.insert(
                string_value("type"),
                Value::String("effect_param".to_string()),
            );
            value.insert(string_value("effect_id"), serialized_value(effect_id.0)?);
            value.insert(
                string_value("param"),
                Value::String(param.as_str().to_string()),
            );
        }
        AutomationTarget::CompositionNodeParam { node_id, param } => {
            value.insert(
                string_value("type"),
                Value::String("composition_node_param".to_string()),
            );
            value.insert(string_value("node_id"), serialized_value(node_id.0)?);
            value.insert(
                string_value("param"),
                Value::String(param.as_str().to_string()),
            );
        }
    }
    Ok(Value::Mapping(value))
}

pub(super) fn automation_mapping_value(
    mapping: &AutomationMapping,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match mapping {
        AutomationMapping::Float { min, max } => {
            value.insert(string_value("type"), Value::String("float".to_string()));
            value.insert(string_value("min"), serialized_value(*min)?);
            value.insert(string_value("max"), serialized_value(*max)?);
        }
        AutomationMapping::Int { min, max } => {
            value.insert(string_value("type"), Value::String("int".to_string()));
            value.insert(string_value("min"), serialized_value(*min)?);
            value.insert(string_value("max"), serialized_value(*max)?);
        }
        AutomationMapping::Bool => {
            value.insert(string_value("type"), Value::String("bool".to_string()));
        }
        AutomationMapping::Enum { values } => {
            value.insert(string_value("type"), Value::String("enum".to_string()));
            value.insert(
                string_value("values"),
                Value::Sequence(
                    values
                        .iter()
                        .map(|value| Value::String(value.as_str().to_string()))
                        .collect(),
                ),
            );
        }
        AutomationMapping::Curve { min, max } => {
            value.insert(string_value("type"), Value::String("curve".to_string()));
            value.insert(string_value("min"), serialized_value(*min)?);
            value.insert(string_value("max"), serialized_value(*max)?);
        }
    }
    Ok(Value::Mapping(value))
}

pub(super) fn control_clip_value(
    session: &ProjectSession,
    from: &DocumentId,
    clip: &ControlClip,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), serialized_value(clip.id.0)?);
    value.insert(
        string_value("start"),
        Value::String(microseconds_string(clip.start.as_micros_rounded())),
    );
    value.insert(
        string_value("duration"),
        Value::String(microseconds_string(clip.duration.as_micros_rounded())),
    );
    let (target_type, selection) = match &clip.target {
        ControlTarget::Scalar(selection) => ("scalar", selection),
        ControlTarget::Indexed(selection) => ("indexed", selection),
        ControlTarget::FixtureFunction {
            selection,
            function,
        } => {
            value.insert(string_value("function"), serialized_value(function.0)?);
            ("fixture_function", selection)
        }
    };
    value.insert(
        string_value("target_type"),
        Value::String(target_type.to_string()),
    );
    value.insert(
        string_value("selection"),
        element_selection_value(session, from, selection)?,
    );
    let mut control = Mapping::new();
    match &clip.value {
        ControlValue::ConstantNormalized(normalized) => {
            control.insert(
                string_value("type"),
                Value::String("constant_normalized".to_string()),
            );
            control.insert(string_value("value"), serialized_value(*normalized)?);
        }
        ControlValue::NormalizedCurve(curve) => {
            control.insert(
                string_value("type"),
                Value::String("normalized_curve".to_string()),
            );
            control.insert(string_value("curve"), curve_value(curve)?);
        }
        ControlValue::Indexed {
            option,
            range_curve,
        } => {
            control.insert(string_value("type"), Value::String("indexed".to_string()));
            control.insert(string_value("option"), serialized_value(option.0)?);
            if let Some(curve) = range_curve {
                control.insert(string_value("range_curve"), curve_value(curve)?);
            }
        }
        ControlValue::FixtureIndexed { entry, range_curve } => {
            control.insert(
                string_value("type"),
                Value::String("fixture_indexed".to_string()),
            );
            control.insert(string_value("entry"), serialized_value(entry.0)?);
            if let Some(curve) = range_curve {
                control.insert(string_value("range_curve"), curve_value(curve)?);
            }
        }
        ControlValue::ConstantColor(color) => {
            control.insert(
                string_value("type"),
                Value::String("constant_color".to_string()),
            );
            control.insert(string_value("color"), Value::String(color.to_hex()));
        }
        ControlValue::Gradient(gradient) => {
            control.insert(string_value("type"), Value::String("gradient".to_string()));
            control.insert(string_value("gradient"), gradient_value(gradient)?);
        }
    }
    value.insert(string_value("value"), Value::Mapping(control));
    Ok(Value::Mapping(value))
}

pub(super) fn effect_param_value(
    session: &ProjectSession,
    from_document: &DocumentId,
    param: &EffectParamValue,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match param {
        EffectParamValue::Int(inner) => {
            value.insert(string_value("type"), Value::String("integer".to_string()));
            value.insert(string_value("value"), serialized_value(*inner)?);
        }
        EffectParamValue::Float(inner) => {
            value.insert(string_value("type"), Value::String("float".to_string()));
            value.insert(string_value("value"), serialized_value(*inner)?);
        }
        EffectParamValue::Bool(inner) => {
            value.insert(string_value("type"), Value::String("bool".to_string()));
            value.insert(string_value("value"), Value::Bool(*inner));
        }
        EffectParamValue::Color(inner) => {
            value.insert(string_value("type"), Value::String("color".to_string()));
            value.insert(string_value("value"), Value::String(inner.to_hex()));
        }
        EffectParamValue::Enum(inner) => {
            value.insert(string_value("type"), Value::String("enum".to_string()));
            value.insert(
                string_value("value"),
                Value::String(inner.as_str().to_string()),
            );
        }
        EffectParamValue::Marks(inner) => {
            value.insert(string_value("type"), Value::String("marks".to_string()));
            value.insert(string_value("key"), Value::String(inner.name.clone()));
        }
        EffectParamValue::Curve(inner) => {
            value.insert(string_value("type"), Value::String("curve".to_string()));
            value.insert(
                string_value("curve"),
                curve_source_value(session, from_document, inner)?,
            );
        }
        EffectParamValue::Gradient(inner) => {
            value.insert(string_value("type"), Value::String("gradient".to_string()));
            value.insert(
                string_value("gradient"),
                gradient_source_value(session, from_document, inner)?,
            );
        }
        EffectParamValue::Array(values) => {
            value.insert(string_value("type"), Value::String("array".to_string()));
            value.insert(
                string_value("values"),
                Value::Sequence(
                    values
                        .iter()
                        .map(|item| array_item_value(session, from_document, item))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
        }
    }
    Ok(Value::Mapping(value))
}

pub(super) fn array_item_value(
    session: &ProjectSession,
    from_document: &DocumentId,
    param: &EffectParamValue,
) -> Result<Value, ExportProjectError> {
    match param {
        EffectParamValue::Curve(source) => {
            let mut value = Mapping::new();
            value.insert(
                string_value("curve"),
                curve_source_value(session, from_document, source)?,
            );
            Ok(Value::Mapping(value))
        }
        EffectParamValue::Gradient(source) => {
            let mut value = Mapping::new();
            value.insert(
                string_value("gradient"),
                gradient_source_value(session, from_document, source)?,
            );
            Ok(Value::Mapping(value))
        }
        _ => effect_param_value(session, from_document, param),
    }
}

pub(super) fn gradient_source_value(
    session: &ProjectSession,
    from_document: &DocumentId,
    source: &GradientSource,
) -> Result<Value, ExportProjectError> {
    match source {
        GradientSource::Inline(gradient) => gradient_value(gradient),
        GradientSource::Reference(id) => Ok(Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Gradient,
            &id.0,
        )?)),
    }
}

pub(super) fn curve_source_value(
    session: &ProjectSession,
    from_document: &DocumentId,
    source: &CurveSource,
) -> Result<Value, ExportProjectError> {
    match source {
        CurveSource::Inline(curve) => curve_value(curve),
        CurveSource::Reference(id) => Ok(Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Curve,
            &id.0,
        )?)),
    }
}
use dawn_language::control::{ControlClip, ControlTarget, ControlValue};
use dawn_language::dsl::Identifier;
use dawn_language::effect::{
    CurveSource, EffectInst, EffectParamValue, EffectRef, EffectScope, GradientSource,
};
use dawn_language::identity::DocumentId;
use dawn_language::operator::OperatorRef;
use dawn_language::sequence::{
    AutomationBinding, AutomationClip, AutomationDetachmentReason, AutomationMapping,
    AutomationTarget, CompositionGraphNode, CompositionGraphNodeKind, DetachedAutomationBinding,
    EffectGraphEdge, GraphNodePosition, MarkCollection, Sequence, SequenceAudio,
    SequenceCompositionGraph, SequenceLayer,
};
use dawn_language::values::Curve;
use indexmap::IndexMap;
use yaml_serde::{Mapping, Value};

use super::ProjectSession;
use super::values::{
    curve_value, element_selection_value, gradient_value, microseconds_string, serialized_value,
    string_value, typed_object, write_source_reference,
};
use crate::ExportProjectError;
use crate::source::SourceObjectKind;

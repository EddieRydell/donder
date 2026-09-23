pub(in crate::gui) fn effect_params(
    session: &ProjectSession,
    sequence: &donder_language::sequence::Sequence,
    effect: &donder_language::effect::EffectInst,
) -> Vec<SequenceEffectParam> {
    let Some(definition) = session
        .project
        .definitions
        .effects
        .resolve(&effect.definition)
    else {
        return Vec::new();
    };
    definition
        .params
        .iter()
        .filter_map(|param| {
            let kind = param_kind(&param.ty)?;
            let override_value = effect.param_overrides.get(&param.name);
            let mut value = override_value
                .map(|value| effect_param_value(session, value, &param.ty))
                .or_else(|| {
                    param
                        .default
                        .as_ref()
                        .and_then(|value| default_param_value(value, &param.ty))
                })
                .or_else(|| default_value_for_type(&param.ty))?;
            let automation = param_automation(
                sequence,
                &AutomationTarget::EffectParam {
                    effect_id: effect.id.clone(),
                    param: param.name.clone(),
                },
                &effect.start,
                &effect.duration,
                &mut value,
            );
            Some(SequenceEffectParam {
                fixed: param.fixed,
                supports_automation: param.supports_automation(),
                name: param.name.as_str().to_string(),
                kind,
                options: param_options(&param.ty),
                editable: automation.is_none(),
                automation,
                value,
            })
        })
        .collect()
}

pub(in crate::gui) fn sequence_composition_graph_node(
    session: &ProjectSession,
    sequence: &donder_language::sequence::Sequence,
    node: &CompositionGraphNode,
) -> SequenceGraphNode {
    SequenceGraphNode {
        id: graph_node_id(&node.id),
        x: node.position.x,
        y: node.position.y,
        inputs: graph_node_inputs(session, &node.kind),
        outputs: graph_node_outputs(session, &node.kind),
        kind: match &node.kind {
            CompositionGraphNodeKind::Layer { layer_id } => {
                let layer = sequence.layers.iter().find(|layer| layer.id == *layer_id);
                SequenceGraphNodeKind::Layer {
                    layer_id: layer_id.0,
                    layer_name: layer
                        .map(|layer| layer.name.clone())
                        .unwrap_or_else(|| format!("Layer {}", layer_id.0)),
                    layer_color: layer
                        .map(|layer| layer.color.to_hex())
                        .unwrap_or_else(|| "#808080".to_string()),
                    enabled: layer.map(|layer| layer.enabled).unwrap_or(false),
                }
            }
            CompositionGraphNodeKind::Operator(operator) => SequenceGraphNodeKind::Operator {
                operator: graph_operator_to_gui(&operator.operator),
                params: graph_operator_params(session, sequence, &node.id, operator),
            },
            CompositionGraphNodeKind::Output => SequenceGraphNodeKind::Output,
        },
    }
}

pub(in crate::gui) fn graph_node_id(node_id: &CompositionGraphNodeId) -> String {
    format!("node:{}", node_id.0)
}

fn graph_operator_params(
    session: &ProjectSession,
    sequence: &donder_language::sequence::Sequence,
    node_id: &CompositionGraphNodeId,
    operator: &GraphOperatorNode,
) -> Vec<SequenceEffectParam> {
    let Some(definition) = session
        .project
        .definitions
        .operators
        .resolve(&operator.operator)
    else {
        return Vec::new();
    };
    definition
        .params
        .iter()
        .filter_map(|declaration| {
            let kind = param_kind(&declaration.ty)?;
            let override_value = operator.params.get(&declaration.name);
            let mut value = override_value
                .map(|value| effect_param_value(session, value, &declaration.ty))
                .or_else(|| {
                    declaration
                        .default
                        .as_ref()
                        .and_then(|value| default_param_value(value, &declaration.ty))
                })?;
            let automation = param_automation(
                sequence,
                &AutomationTarget::CompositionNodeParam {
                    node_id: node_id.clone(),
                    param: declaration.name.clone(),
                },
                &donder_language::values::DonderTime::from_nanos(0),
                &sequence.duration,
                &mut value,
            );
            Some(SequenceEffectParam {
                fixed: declaration.fixed,
                supports_automation: declaration.supports_automation(),
                name: declaration.name.as_str().to_string(),
                kind,
                options: param_options(&declaration.ty),
                editable: automation.is_none(),
                value,
                automation,
            })
        })
        .collect()
}

pub(in crate::gui) fn graph_operator_definition_to_gui(
    operator: OperatorRef,
    definition: &OperatorDefinition,
) -> SequenceGraphOperatorDefinition {
    SequenceGraphOperatorDefinition {
        operator: graph_operator_to_gui(&operator),
        source_name: definition.source_name.clone(),
        display_name: definition.display_name.clone(),
        inputs: definition.inputs.iter().map(graph_port_to_gui).collect(),
        outputs: vec![graph_port_to_gui(&definition.output)],
        params: definition
            .params
            .iter()
            .filter_map(|param| {
                Some(crate::dto::SequenceEffectDefinitionParam {
                    fixed: param.fixed,
                    supports_automation: param.supports_automation(),
                    name: param.name.as_str().to_string(),
                    kind: param_kind(&param.ty)?,
                })
            })
            .collect(),
    }
}

fn graph_port_to_gui(port: &OperatorPortDefinition) -> SequenceGraphPortDefinition {
    SequenceGraphPortDefinition {
        source_name: port.source_name.to_string(),
        display_name: port.display_name.to_string(),
        cardinality: match port.cardinality {
            OperatorPortCardinality::One => SequenceGraphPortCardinality::One,
            OperatorPortCardinality::Many => SequenceGraphPortCardinality::Many,
        },
    }
}

fn graph_node_inputs(
    session: &ProjectSession,
    kind: &CompositionGraphNodeKind,
) -> Vec<SequenceGraphPortDefinition> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => vec![],
        CompositionGraphNodeKind::Operator(operator) => session
            .project
            .definitions
            .operators
            .resolve(&operator.operator)
            .into_iter()
            .flat_map(|definition| definition.inputs.iter())
            .map(graph_port_to_gui)
            .collect(),
        CompositionGraphNodeKind::Output => vec![SequenceGraphPortDefinition {
            source_name: "input".to_string(),
            display_name: "Input".to_string(),
            cardinality: SequenceGraphPortCardinality::Many,
        }],
    }
}

fn graph_node_outputs(
    session: &ProjectSession,
    kind: &CompositionGraphNodeKind,
) -> Vec<SequenceGraphPortDefinition> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => {
            vec![SequenceGraphPortDefinition {
                source_name: "output".to_string(),
                display_name: "Output".to_string(),
                cardinality: SequenceGraphPortCardinality::Many,
            }]
        }
        CompositionGraphNodeKind::Operator(operator) => session
            .project
            .definitions
            .operators
            .resolve(&operator.operator)
            .map(|definition| vec![graph_port_to_gui(&definition.output)])
            .unwrap_or_default(),
        CompositionGraphNodeKind::Output => vec![],
    }
}

fn graph_operator_to_gui(operator: &OperatorRef) -> SequenceGraphOperator {
    match operator {
        OperatorRef::Builtin(operator) => SequenceGraphOperator::Builtin {
            operator: match operator {
                BuiltinOperator::Max => SequenceBuiltinOperator::Max,
                BuiltinOperator::Add => SequenceBuiltinOperator::Add,
                BuiltinOperator::Multiply => SequenceBuiltinOperator::Multiply,
                BuiltinOperator::IntensityModulate => SequenceBuiltinOperator::IntensityModulate,
                BuiltinOperator::Dim => SequenceBuiltinOperator::Dim,
                BuiltinOperator::Invert => SequenceBuiltinOperator::Invert,
                BuiltinOperator::Colorize => SequenceBuiltinOperator::Colorize,
                BuiltinOperator::Delay => SequenceBuiltinOperator::Delay,
                BuiltinOperator::Echo => SequenceBuiltinOperator::Echo,
            },
        },
        OperatorRef::Custom(id) => SequenceGraphOperator::Custom {
            module_id: id.0.module_id().to_string(),
            path: id.0.document().to_string(),
            object_key: id.0.object().to_string(),
        },
    }
}

fn param_automation(
    sequence: &donder_language::sequence::Sequence,
    target: &AutomationTarget,
    start: &donder_language::values::DonderTime,
    duration: &donder_language::values::DonderDuration,
    value: &mut SequenceEffectParamValue,
) -> Option<SequenceParamAutomation> {
    sequence.automation_clips.iter().find_map(|clip| {
        let binding = clip
            .bindings
            .iter()
            .find(|binding| &binding.target == target)?;
        if let SequenceEffectParamValue::Curve { value } = value {
            value.points = curve_points(&clip.curve_in_range(start, duration));
        }
        Some(SequenceParamAutomation {
            clip_id: clip.id.0,
            mapping: automation_mapping_to_gui(&binding.mapping),
        })
    })
}

pub(in crate::gui) fn automation_mapping_to_gui(
    mapping: &AutomationMapping,
) -> SequenceAutomationMapping {
    match mapping {
        AutomationMapping::Float { min, max } => SequenceAutomationMapping::Float {
            min: *min,
            max: *max,
        },
        AutomationMapping::Int { min, max } => SequenceAutomationMapping::Int {
            min: *min as f32,
            max: *max as f32,
        },
        AutomationMapping::Bool => SequenceAutomationMapping::Bool,
        AutomationMapping::Enum { values } => SequenceAutomationMapping::Enum {
            values: values
                .iter()
                .map(|value| value.as_str().to_string())
                .collect(),
        },
        AutomationMapping::Curve { min, max } => SequenceAutomationMapping::Curve {
            min: *min,
            max: *max,
        },
    }
}

pub(in crate::gui) fn curve_library(session: &ProjectSession) -> Vec<SequenceCurveLibraryItem> {
    session
        .project
        .definitions
        .curves
        .definitions
        .iter()
        .map(|(id, definition)| SequenceCurveLibraryItem {
            module_id: id.0.module_id().to_string(),
            path: id.0.document().to_string(),
            object_key: id.0.object().to_string(),
            display_name: id.0.object().to_string(),
            points: curve_points(&definition.curve),
        })
        .collect()
}

pub(in crate::gui) fn gradient_library(
    session: &ProjectSession,
) -> Vec<SequenceGradientLibraryItem> {
    session
        .project
        .definitions
        .gradients
        .definitions
        .iter()
        .map(|(id, definition)| SequenceGradientLibraryItem {
            module_id: id.0.module_id().to_string(),
            path: id.0.document().to_string(),
            object_key: id.0.object().to_string(),
            display_name: id.0.object().to_string(),
            stops: gradient_stops(&definition.gradient),
        })
        .collect()
}

pub(in crate::gui) fn param_kind(ty: &Type) -> Option<SequenceEffectParamKind> {
    Some(match ty {
        Type::Int => SequenceEffectParamKind::Int,
        Type::Float => SequenceEffectParamKind::Float,
        Type::Bool => SequenceEffectParamKind::Bool,
        Type::Color => SequenceEffectParamKind::Color,
        Type::Enum(_) => SequenceEffectParamKind::Enum,
        Type::Marks => SequenceEffectParamKind::Marks,
        Type::Curve => SequenceEffectParamKind::Curve,
        Type::Gradient => SequenceEffectParamKind::Gradient,
        Type::Array(inner) => match inner.as_ref() {
            Type::Int => SequenceEffectParamKind::IntArray,
            Type::Float => SequenceEffectParamKind::FloatArray,
            Type::Bool => SequenceEffectParamKind::BoolArray,
            Type::Color => SequenceEffectParamKind::ColorArray,
            Type::Curve => SequenceEffectParamKind::CurveArray,
            Type::Gradient => SequenceEffectParamKind::GradientArray,
            _ => SequenceEffectParamKind::FloatArray,
        },
        Type::Void
        | Type::Signal
        | Type::Timeline
        | Type::Target
        | Type::TargetItems
        | Type::TargetItem => {
            return None;
        }
    })
}

fn param_options(ty: &Type) -> Vec<String> {
    match ty {
        Type::Enum(options) => options
            .iter()
            .map(|option| option.as_str().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

pub(in crate::gui) fn effect_param_value(
    session: &ProjectSession,
    value: &EffectParamValue,
    ty: &Type,
) -> SequenceEffectParamValue {
    match value {
        EffectParamValue::Int(value) => SequenceEffectParamValue::Int {
            value: *value as f32,
        },
        EffectParamValue::Float(value) => SequenceEffectParamValue::Float { value: *value },
        EffectParamValue::Bool(value) => SequenceEffectParamValue::Bool { value: *value },
        EffectParamValue::Color(value) => SequenceEffectParamValue::Color {
            value: value.to_hex(),
        },
        EffectParamValue::Enum(value) => SequenceEffectParamValue::Enum {
            value: value.as_str().to_string(),
        },
        EffectParamValue::Marks(value) => SequenceEffectParamValue::Marks {
            key: value.name.clone(),
        },
        EffectParamValue::Curve(source) => SequenceEffectParamValue::Curve {
            value: SequenceCurveValue {
                points: curve_points(match source {
                    CurveSource::Inline(curve) => curve,
                    CurveSource::Reference(id) => {
                        &session.project.definitions.curves.definitions[id].curve
                    }
                }),
                source: match source {
                    CurveSource::Inline(_) => SequenceLibrarySource::Inline,
                    CurveSource::Reference(id) => library_source(&id.0),
                },
            },
        },
        EffectParamValue::Gradient(source) => SequenceEffectParamValue::Gradient {
            value: SequenceGradientValue {
                stops: gradient_stops(match source {
                    GradientSource::Inline(gradient) => gradient,
                    GradientSource::Reference(id) => {
                        &session.project.definitions.gradients.definitions[id].gradient
                    }
                }),
                source: match source {
                    GradientSource::Inline(_) => SequenceLibrarySource::Inline,
                    GradientSource::Reference(id) => library_source(&id.0),
                },
            },
        },
        EffectParamValue::Array(values) => array_param_value(session, values, ty),
    }
}

pub(in crate::gui) fn default_param_value(
    value: &EffectValue,
    ty: &Type,
) -> Option<SequenceEffectParamValue> {
    Some(match value {
        EffectValue::Int(value) => SequenceEffectParamValue::Int {
            value: *value as f32,
        },
        EffectValue::Float(value) => SequenceEffectParamValue::Float { value: *value },
        EffectValue::Bool(value) => SequenceEffectParamValue::Bool { value: *value },
        EffectValue::Color(value) => SequenceEffectParamValue::Color {
            value: value.to_hex(),
        },
        EffectValue::Enum(value) => SequenceEffectParamValue::Enum {
            value: value.as_str().to_string(),
        },
        EffectValue::Marks(_) => SequenceEffectParamValue::Marks { key: String::new() },
        EffectValue::Curve(curve) => SequenceEffectParamValue::Curve {
            value: SequenceCurveValue {
                points: curve_points(curve),
                source: SequenceLibrarySource::Inline,
            },
        },
        EffectValue::Gradient(gradient) => SequenceEffectParamValue::Gradient {
            value: SequenceGradientValue {
                stops: gradient_stops(gradient),
                source: SequenceLibrarySource::Inline,
            },
        },
        EffectValue::Array(values) => {
            let Type::Array(inner) = ty else {
                return None;
            };
            let converted = values
                .iter()
                .map(|value| default_param_value(value, inner))
                .collect::<Option<Vec<_>>>()?;
            array_param_from_sequence_values(&converted, inner)
        }
        EffectValue::Void
        | EffectValue::Target(_)
        | EffectValue::TargetItems(_)
        | EffectValue::TargetItem(_) => return None,
    })
}

fn default_value_for_type(ty: &Type) -> Option<SequenceEffectParamValue> {
    default_param_value(&ty.default_value(), ty)
}

fn library_source(id: &donder_language::identity::SourceIdentity) -> SequenceLibrarySource {
    SequenceLibrarySource::Library {
        module_id: id.module_id().to_string(),
        path: id.document().to_string(),
        object_key: id.object().to_string(),
        display_name: id.object().to_string(),
    }
}

use crate::gui::model::{curve_points, gradient_stops};

fn array_param_value(
    session: &ProjectSession,
    values: &[EffectParamValue],
    ty: &Type,
) -> SequenceEffectParamValue {
    let Type::Array(inner) = ty else {
        unreachable!("validated array parameter type");
    };
    let converted = values
        .iter()
        .map(|value| effect_param_value(session, value, inner))
        .collect::<Vec<_>>();
    array_param_from_sequence_values(&converted, inner)
}

fn array_param_from_sequence_values(
    values: &[SequenceEffectParamValue],
    inner: &Type,
) -> SequenceEffectParamValue {
    match inner {
        Type::Int => SequenceEffectParamValue::IntArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Int { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
        Type::Bool => SequenceEffectParamValue::BoolArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Bool { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
        Type::Color => SequenceEffectParamValue::ColorArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Color { value } => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        },
        Type::Gradient => SequenceEffectParamValue::GradientArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Gradient { value } => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        },
        Type::Curve => SequenceEffectParamValue::CurveArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Curve { value } => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        },
        _ => SequenceEffectParamValue::FloatArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Float { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
    }
}
use donder_language::dsl::{Type, Value as EffectValue};
use donder_language::effect::{CurveSource, EffectParamValue, GradientSource};
use donder_language::operator::{
    BuiltinOperator, GraphOperatorNode, OperatorDefinition, OperatorPortCardinality,
    OperatorPortDefinition, OperatorRef,
};
use donder_language::sequence::{
    AutomationMapping, AutomationTarget, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind,
};
use donder_project_io::ProjectSession;

use crate::dto::{
    SequenceAutomationMapping, SequenceBuiltinOperator, SequenceCurveLibraryItem,
    SequenceCurveValue, SequenceEffectParam, SequenceEffectParamKind, SequenceEffectParamValue,
    SequenceGradientLibraryItem, SequenceGradientValue, SequenceGraphNode, SequenceGraphNodeKind,
    SequenceGraphOperator, SequenceGraphOperatorDefinition, SequenceGraphPortCardinality,
    SequenceGraphPortDefinition, SequenceLibrarySource, SequenceParamAutomation,
};

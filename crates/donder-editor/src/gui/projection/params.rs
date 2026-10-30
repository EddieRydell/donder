pub(in crate::gui) fn clip_params(
    session: &ProjectSession,
    sequence: &donder_model::Sequence,
    clip: &donder_model::Clip,
) -> Vec<SequenceClipParam> {
    invocation_params(
        session,
        &clip.definition,
        &clip.param_overrides,
        |name, value| {
            param_automation(
                sequence,
                &AutomationTarget::ClipParam {
                    clip_id: clip.id.clone(),
                    param: name.clone(),
                },
                &clip.start,
                &clip.duration,
                value,
            )
        },
    )
}

/// A clip invocation's parameters: each declared parameter with its
/// override, else its default, and the automation `automation` finds for it.
pub(in crate::gui) fn invocation_params(
    session: &ProjectSession,
    definition: &donder_model::EffectDefinitionId,
    overrides: &indexmap::IndexMap<Identifier, ParamValue>,
    mut automation: impl FnMut(&Identifier, &mut SequenceParamValue) -> Option<SequenceParamAutomation>,
) -> Vec<SequenceClipParam> {
    let Some(definition) = session
        .project
        .get::<donder_model::EffectDefinition>(*definition)
    else {
        return Vec::new();
    };
    definition
        .params()
        .iter()
        .filter_map(|param| {
            let kind = param_kind(&param.ty)?;
            let override_value = overrides.get(&param.name);
            let mut value = override_value
                .map(|value| param_value_to_gui(session, value, &param.ty))
                .or_else(|| {
                    param
                        .default
                        .as_ref()
                        .and_then(|value| default_param_value(value, &param.ty))
                })
                .or_else(|| default_value_for_type(&param.ty))?;
            let automation = automation(&param.name, &mut value);
            Some(SequenceClipParam {
                supports_automation: param.supports_automation(),
                name: param.name.as_str().to_string(),
                description: param.description.clone(),
                kind,
                options: param_options(&param.ty),
                range: param_range(param),
                editable: automation.is_none(),
                automation,
                value,
            })
        })
        .collect()
}

/// A composition's graph as the graph editor shows it, with what
/// `automation` finds automating each operator parameter.
pub(in crate::gui) fn composition_graph(
    session: &ProjectSession,
    layers: &[donder_model::SequenceLayer],
    graph: &donder_model::SequenceCompositionGraph,
    automation: impl Fn(
        &CompositionGraphNodeId,
        &Identifier,
        &mut SequenceParamValue,
    ) -> Option<SequenceParamAutomation>,
) -> SequenceCompositionGraph {
    SequenceCompositionGraph {
        id: 0,
        operator_catalog: session
            .project
            .reusable::<donder_model::OperatorDefinition>()
            .iter()
            .map(|(id, definition)| graph_operator_definition_to_gui(session, *id, definition))
            .collect(),
        nodes: graph
            .nodes
            .iter()
            .map(|node| composition_graph_node(session, layers, &automation, node))
            .collect(),
        edges: graph
            .edges
            .iter()
            .map(|edge| SequenceGraphEdge {
                from_node: graph_node_id(&edge.from),
                from_port: edge.from_port.0.clone(),
                to_node: graph_node_id(&edge.to),
                to_port: edge.to_port.0.clone(),
            })
            .collect(),
    }
}

pub(in crate::gui) fn layers_to_gui(layers: &[donder_model::SequenceLayer]) -> Vec<SequenceLayer> {
    layers
        .iter()
        .map(|layer| SequenceLayer {
            id: layer.id.0,
            name: layer.name.as_str().to_string(),
            description: layer.description.clone(),
            color: layer.color.to_hex(),
            enabled: layer.enabled,
            is_default: layer.id.0 == 0,
        })
        .collect()
}

fn composition_graph_node(
    session: &ProjectSession,
    layers: &[donder_model::SequenceLayer],
    automation: &impl Fn(
        &CompositionGraphNodeId,
        &Identifier,
        &mut SequenceParamValue,
    ) -> Option<SequenceParamAutomation>,
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
                let layer = layers.iter().find(|layer| layer.id == *layer_id);
                SequenceGraphNodeKind::Layer {
                    layer_id: layer_id.0,
                    layer_name: layer
                        .map(|layer| layer.name.as_str().to_string())
                        .unwrap_or_else(|| format!("Layer {}", layer_id.0)),
                    layer_color: layer
                        .map(|layer| layer.color.to_hex())
                        .unwrap_or_else(|| "#808080".to_string()),
                    enabled: layer.map(|layer| layer.enabled).unwrap_or(false),
                }
            }
            CompositionGraphNodeKind::Operator(operator) => SequenceGraphNodeKind::Operator {
                name: operator.name.as_str().to_string(),
                operator: graph_operator_to_gui(session, &operator.operator),
                params: graph_operator_params(session, automation, &node.id, operator),
            },
            CompositionGraphNodeKind::Root => SequenceGraphNodeKind::Root,
        },
    }
}

pub(in crate::gui) fn graph_node_id(node_id: &CompositionGraphNodeId) -> String {
    format!("node:{}", node_id.0)
}

fn graph_operator_params(
    session: &ProjectSession,
    automation: &impl Fn(
        &CompositionGraphNodeId,
        &Identifier,
        &mut SequenceParamValue,
    ) -> Option<SequenceParamAutomation>,
    node_id: &CompositionGraphNodeId,
    operator: &GraphOperatorNode,
) -> Vec<SequenceClipParam> {
    let Some(definition) = session
        .project
        .get::<donder_model::OperatorDefinition>(operator.operator)
    else {
        return Vec::new();
    };
    definition
        .params()
        .iter()
        .filter_map(|declaration| {
            let kind = param_kind(&declaration.ty)?;
            let override_value = operator.params.get(&declaration.name);
            let mut value = override_value
                .map(|value| param_value_to_gui(session, value, &declaration.ty))
                .or_else(|| {
                    declaration
                        .default
                        .as_ref()
                        .and_then(|value| default_param_value(value, &declaration.ty))
                })?;
            let automation = automation(node_id, &declaration.name, &mut value);
            Some(SequenceClipParam {
                supports_automation: declaration.supports_automation(),
                name: declaration.name.as_str().to_string(),
                description: declaration.description.clone(),
                kind,
                options: param_options(&declaration.ty),
                range: param_range(declaration),
                editable: automation.is_none(),
                value,
                automation,
            })
        })
        .collect()
}

fn graph_operator_definition_to_gui(
    session: &ProjectSession,
    operator: OperatorDefinitionId,
    definition: &OperatorDefinition,
) -> SequenceGraphOperatorDefinition {
    SequenceGraphOperatorDefinition {
        operator: graph_operator_to_gui(session, &operator),
        source_name: definition.name().to_string(),
        display_name: definition.name().to_string(),
        description: definition.description().map(str::to_string),
        inputs: definition.inputs().iter().map(graph_port_to_gui).collect(),
        outputs: vec![graph_port_to_gui(definition.output())],
        params: definition_params_to_gui(definition.params()),
    }
}

/// The parameters a definition declares, as the inspector lists them.
pub(in crate::gui) fn definition_params_to_gui(
    params: &[donder_language::compiler::ParamDecl],
) -> Vec<crate::dto::SequenceEffectDefinitionParam> {
    params
        .iter()
        .filter_map(|param| {
            Some(crate::dto::SequenceEffectDefinitionParam {
                supports_automation: param.supports_automation(),
                name: param.name.as_str().to_string(),
                description: param.description.clone(),
                kind: param_kind(&param.ty)?,
            })
        })
        .collect()
}

fn graph_port_to_gui(port: &OperatorPortDefinition) -> SequenceGraphPortDefinition {
    SequenceGraphPortDefinition {
        source_name: port.source_name.to_string(),
        display_name: port.display_name.to_string(),
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
            .get::<donder_model::OperatorDefinition>(operator.operator)
            .into_iter()
            .flat_map(|definition| definition.inputs().iter())
            .map(graph_port_to_gui)
            .collect(),
        CompositionGraphNodeKind::Root => vec![SequenceGraphPortDefinition {
            source_name: "input".to_string(),
            display_name: "Input".to_string(),
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
            }]
        }
        CompositionGraphNodeKind::Operator(operator) => session
            .project
            .get::<donder_model::OperatorDefinition>(operator.operator)
            .map(|definition| vec![graph_port_to_gui(definition.output())])
            .unwrap_or_default(),
        CompositionGraphNodeKind::Root => vec![],
    }
}

fn graph_operator_to_gui(
    session: &ProjectSession,
    id: &OperatorDefinitionId,
) -> SequenceGraphOperator {
    let identity = crate::gui::model::declared_identity(session, *id);
    SequenceGraphOperator {
        path: identity.document().to_string(),
        object_key: identity.object().to_string(),
    }
}

/// The automation of a sequence's operator node parameter, over the
/// sequence.
pub(super) fn sequence_node_automation(
    sequence: &donder_model::Sequence,
    node_id: &CompositionGraphNodeId,
    param: &Identifier,
    value: &mut SequenceParamValue,
) -> Option<SequenceParamAutomation> {
    param_automation(
        sequence,
        &AutomationTarget::CompositionNodeParam {
            node_id: node_id.clone(),
            param: param.clone(),
        },
        &donder_language::DonderTime::from_nanos(0),
        &sequence.duration,
        value,
    )
}

fn param_automation(
    sequence: &donder_model::Sequence,
    target: &AutomationTarget,
    start: &donder_language::DonderTime,
    duration: &donder_language::DonderDuration,
    value: &mut SequenceParamValue,
) -> Option<SequenceParamAutomation> {
    let envelope = sequence.automation_envelope(target)?;
    let clip = sequence.automation_clips.iter().find(|clip| {
        clip.bindings
            .iter()
            .any(|binding| &binding.target == target)
    })?;
    if let SequenceParamValue::Curve { value } = value {
        value.points = curve_points(&envelope.curve_in_range(start, duration));
    }
    Some(SequenceParamAutomation {
        automation_clip_id: clip.id.0,
    })
}

pub(in crate::gui) fn curve_library(session: &ProjectSession) -> Vec<SequenceCurveLibraryItem> {
    let available =
        donder_project_io::available_declarations(session, &[donder_model::ObjectKind::Curve]);
    session
        .project
        .reusable::<donder_model::CurveDefinition>()
        .iter()
        .filter(|(id, _)| available.contains(&(**id).into()))
        .map(|(id, definition)| {
            let identity = crate::gui::model::declared_identity(session, *id);
            (identity, definition)
        })
        .map(|(identity, definition)| SequenceCurveLibraryItem {
            path: identity.document().to_string(),
            object_key: identity.object().to_string(),
            display_name: identity.object().to_string(),
            points: curve_points(&definition.curve),
        })
        .collect()
}

pub(in crate::gui) fn gradient_library(
    session: &ProjectSession,
) -> Vec<SequenceGradientLibraryItem> {
    let available =
        donder_project_io::available_declarations(session, &[donder_model::ObjectKind::Gradient]);
    session
        .project
        .reusable::<donder_model::GradientDefinition>()
        .iter()
        .filter(|(id, _)| available.contains(&(**id).into()))
        .map(|(id, definition)| {
            let identity = crate::gui::model::declared_identity(session, *id);
            (identity, definition)
        })
        .map(|(identity, definition)| SequenceGradientLibraryItem {
            path: identity.document().to_string(),
            object_key: identity.object().to_string(),
            display_name: identity.object().to_string(),
            stops: gradient_stops(&definition.gradient),
        })
        .collect()
}

fn param_kind(ty: &Type) -> Option<SequenceParamKind> {
    Some(match ty {
        Type::Int => SequenceParamKind::Int,
        Type::Float => SequenceParamKind::Float,
        Type::Bool => SequenceParamKind::Bool,
        Type::Color => SequenceParamKind::Color,
        Type::Enum(_) => SequenceParamKind::Enum,
        Type::Marks => SequenceParamKind::Marks,
        Type::Curve => SequenceParamKind::Curve,
        Type::Gradient => SequenceParamKind::Gradient,
        Type::Array(inner) => match inner.as_ref() {
            Type::Int => SequenceParamKind::IntArray,
            Type::Float => SequenceParamKind::FloatArray,
            Type::Bool => SequenceParamKind::BoolArray,
            Type::Color => SequenceParamKind::ColorArray,
            Type::Curve => SequenceParamKind::CurveArray,
            Type::Gradient => SequenceParamKind::GradientArray,
            _ => SequenceParamKind::FloatArray,
        },
        Type::Void => {
            return None;
        }
    })
}

fn param_range(param: &ParamDecl) -> Option<SequenceParamRange> {
    param.range.map(|range| match range {
        ParamRange::Int { min, max } => SequenceParamRange {
            min: f64::from(min),
            max: f64::from(max),
        },
        ParamRange::Float { min, max } => SequenceParamRange {
            min: f64::from(min),
            max: f64::from(max),
        },
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

fn param_value_to_gui(
    session: &ProjectSession,
    value: &ParamValue,
    ty: &Type,
) -> SequenceParamValue {
    match value {
        ParamValue::Int(value) => SequenceParamValue::Int {
            value: *value as f32,
        },
        ParamValue::Float(value) => SequenceParamValue::Float { value: *value },
        ParamValue::Bool(value) => SequenceParamValue::Bool { value: *value },
        ParamValue::Color(value) => SequenceParamValue::Color {
            value: value.to_hex(),
        },
        ParamValue::Enum(value) => SequenceParamValue::Enum {
            value: value.as_str().to_string(),
        },
        ParamValue::Marks(value) => SequenceParamValue::Marks {
            key: value
                .as_ref()
                .map(|collection| collection.name.as_str().to_string()),
        },
        ParamValue::Curve(source) => SequenceParamValue::Curve {
            value: SequenceCurveValue {
                points: curve_points(match source {
                    OwnedCurve::Inline(curve) => curve,
                    OwnedCurve::Reference(id) => {
                        &session.project.reusable::<donder_model::CurveDefinition>()[id].curve
                    }
                }),
                link: match source {
                    OwnedCurve::Inline(_) => SequenceLibraryLink::Inline,
                    OwnedCurve::Reference(id) => {
                        library_link(&crate::gui::model::declared_identity(session, *id))
                    }
                },
            },
        },
        ParamValue::Gradient(source) => SequenceParamValue::Gradient {
            value: SequenceGradientValue {
                stops: gradient_stops(match source {
                    OwnedGradient::Inline(gradient) => gradient,
                    OwnedGradient::Reference(id) => {
                        &session
                            .project
                            .reusable::<donder_model::GradientDefinition>()[id]
                            .gradient
                    }
                }),
                link: match source {
                    OwnedGradient::Inline(_) => SequenceLibraryLink::Inline,
                    OwnedGradient::Reference(id) => {
                        library_link(&crate::gui::model::declared_identity(session, *id))
                    }
                },
            },
        },
        ParamValue::Array(values) => array_param_value(session, values, ty),
    }
}

fn default_param_value(value: &EffectValue, ty: &Type) -> Option<SequenceParamValue> {
    Some(match value {
        EffectValue::Int(value) => SequenceParamValue::Int {
            value: *value as f32,
        },
        EffectValue::Float(value) => SequenceParamValue::Float { value: *value },
        EffectValue::Bool(value) => SequenceParamValue::Bool { value: *value },
        EffectValue::Color(value) => SequenceParamValue::Color {
            value: value.to_hex(),
        },
        EffectValue::Enum(value) => SequenceParamValue::Enum {
            value: value.as_str().to_string(),
        },
        EffectValue::Marks(_) => SequenceParamValue::Marks { key: None },
        EffectValue::Curve(curve) => SequenceParamValue::Curve {
            value: SequenceCurveValue {
                points: curve_points(curve),
                link: SequenceLibraryLink::Inline,
            },
        },
        EffectValue::Gradient(gradient) => SequenceParamValue::Gradient {
            value: SequenceGradientValue {
                stops: gradient_stops(gradient),
                link: SequenceLibraryLink::Inline,
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
        EffectValue::Void => return None,
    })
}

fn default_value_for_type(ty: &Type) -> Option<SequenceParamValue> {
    default_param_value(&ty.default_value(), ty)
}

fn library_link(id: &donder_project_io::SourceIdentity) -> SequenceLibraryLink {
    SequenceLibraryLink::Library {
        path: id.document().to_string(),
        object_key: id.object().to_string(),
        display_name: id.object().to_string(),
    }
}

use crate::gui::model::{curve_points, gradient_stops};

fn array_param_value(
    session: &ProjectSession,
    values: &[ParamValue],
    ty: &Type,
) -> SequenceParamValue {
    let Type::Array(inner) = ty else {
        unreachable!("validated array parameter type");
    };
    let converted = values
        .iter()
        .map(|value| param_value_to_gui(session, value, inner))
        .collect::<Vec<_>>();
    array_param_from_sequence_values(&converted, inner)
}

fn array_param_from_sequence_values(
    values: &[SequenceParamValue],
    inner: &Type,
) -> SequenceParamValue {
    match inner {
        Type::Int => SequenceParamValue::IntArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceParamValue::Int { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
        Type::Bool => SequenceParamValue::BoolArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceParamValue::Bool { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
        Type::Color => SequenceParamValue::ColorArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceParamValue::Color { value } => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        },
        Type::Gradient => SequenceParamValue::GradientArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceParamValue::Gradient { value } => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        },
        Type::Curve => SequenceParamValue::CurveArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceParamValue::Curve { value } => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        },
        _ => SequenceParamValue::FloatArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceParamValue::Float { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
    }
}
use donder_language::compiler::{ParamDecl, ParamRange};
use donder_model::{
    AutomationTarget, CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind,
};
use donder_model::{
    GraphOperatorNode, OperatorDefinition, OperatorDefinitionId, OperatorPortDefinition,
};
use donder_model::{OwnedCurve, OwnedGradient, ParamValue};
use donder_project_io::ProjectSession;
use donder_runtime_types::{Identifier, Type, Value as EffectValue};

use crate::dto::{
    SequenceClipParam, SequenceCompositionGraph, SequenceCurveLibraryItem, SequenceCurveValue,
    SequenceGradientLibraryItem, SequenceGradientValue, SequenceGraphEdge, SequenceGraphNode,
    SequenceGraphNodeKind, SequenceGraphOperator, SequenceGraphOperatorDefinition,
    SequenceGraphPortDefinition, SequenceLayer, SequenceLibraryLink, SequenceParamAutomation,
    SequenceParamKind, SequenceParamRange, SequenceParamValue,
};

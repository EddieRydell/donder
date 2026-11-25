pub(in crate::gui) fn clip_params(
    session: &ProjectSession,
    sequence: &donder_model::Sequence,
    clip: &donder_model::Clip,
    effect: &donder_model::Effect,
) -> Vec<SequenceClipParam> {
    invocation_params(
        session,
        &effect.definition,
        &effect.params,
        |name, value| {
            let (start, duration) = clip.timing()?;
            param_automation(
                sequence,
                &AutomationTarget::ClipParam {
                    clip_id: clip.id,
                    param: name.clone(),
                },
                start,
                duration,
                value,
            )
        },
    )
}

/// A clip invocation's parameters: each declared parameter with its
/// override, else its default, and the automation `automation` finds for it.
pub(in crate::gui) fn invocation_params(
    session: &ProjectSession,
    definition: &donder_model::ScriptId,
    overrides: &indexmap::IndexMap<Identifier, ParamValue>,
    mut automation: impl FnMut(&Identifier, &mut SequenceParamValue) -> Option<SequenceParamAutomation>,
) -> Vec<SequenceClipParam> {
    let definition = &session
        .project()
        .reusable::<donder_model::ScriptDefinition>()[definition];
    definition
        .params()
        .iter()
        .filter_map(|param| {
            let mut value = match overrides.get(&param.name) {
                Some(value) => param_value_to_gui(session, value, &param.ty, &param.option_fields),
                None => declared_value(session, param)?,
            };
            let automation = automation(&param.name, &mut value);
            param_to_gui(session, param, value, automation)
        })
        .collect()
}

/// A composition's graph as the graph editor shows it, with what
/// `automation` finds automating each operator parameter.
pub(in crate::gui) fn composition_graph(
    session: &ProjectSession,
    layers: &[donder_model::Layer],
    graph: &donder_model::CompositionGraph,
    automation: impl Fn(
        &CompositionGraphNodeId,
        &Identifier,
        &mut SequenceParamValue,
    ) -> Option<SequenceParamAutomation>,
) -> SequenceCompositionGraph {
    SequenceCompositionGraph {
        operator_catalog: session
            .project()
            .reusable::<donder_model::ScriptDefinition>()
            .iter()
            .filter(|(_, definition)| definition.kind() == DeclarationKind::Operator)
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
                from_node: edge.from.0,
                to_node: edge.to.0,
                input: edge.input.as_ref().map(|input| input.as_str().to_string()),
            })
            .collect(),
    }
}

pub(in crate::gui) fn layers_to_gui(layers: &[donder_model::Layer]) -> Vec<SequenceLayer> {
    layers
        .iter()
        .map(|layer| SequenceLayer {
            id: layer.id.0,
            name: layer.name.as_str().to_string(),
            description: layer.description.clone(),
            color: layer.color.to_hex(),
            enabled: layer.enabled,
        })
        .collect()
}

fn composition_graph_node(
    session: &ProjectSession,
    layers: &[donder_model::Layer],
    automation: &impl Fn(
        &CompositionGraphNodeId,
        &Identifier,
        &mut SequenceParamValue,
    ) -> Option<SequenceParamAutomation>,
    node: &CompositionGraphNode,
) -> SequenceGraphNode {
    SequenceGraphNode {
        id: node.id.0,
        x: node.position.x,
        y: node.position.y,
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
    let definition = &session
        .project()
        .reusable::<donder_model::ScriptDefinition>()[&operator.operator];
    definition
        .params()
        .iter()
        .filter_map(|declaration| {
            let mut value = match operator.params.get(&declaration.name) {
                Some(value) => {
                    param_value_to_gui(session, value, &declaration.ty, &declaration.option_fields)
                }
                None => declared_value(session, declaration)?,
            };
            let automation = automation(node_id, &declaration.name, &mut value);
            param_to_gui(session, declaration, value, automation)
        })
        .collect()
}

fn graph_operator_definition_to_gui(
    session: &ProjectSession,
    operator: ScriptId,
    definition: &ScriptDefinition,
) -> SequenceGraphOperatorDefinition {
    SequenceGraphOperatorDefinition {
        operator: graph_operator_to_gui(session, &operator),
        display_name: definition.name().to_string(),
        description: definition.description().map(str::to_string),
        inputs: definition
            .inputs()
            .map(|input| input.as_str().to_string())
            .collect(),
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

fn graph_operator_to_gui(session: &ProjectSession, id: &ScriptId) -> SequenceGraphOperator {
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
    let composition = &sequence.composition;
    let envelope = composition.automation_envelope(target)?;
    let (clip, binding) = composition
        .clips
        .iter()
        .find_map(|clip| Some((clip, clip.envelope()?.binding(target)?)))?;
    if let SequenceParamValue::Curve { value } = value {
        value.points = curve_points(&envelope.curve_in_range(start, duration));
    }
    Some(SequenceParamAutomation {
        clip_id: clip.id.0,
        window: binding.window.into(),
    })
}

pub(in crate::gui) fn curve_library(session: &ProjectSession) -> Vec<SequenceCurveLibraryItem> {
    session
        .project()
        .reusable::<donder_model::CurveDefinition>()
        .iter()
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
    session
        .project()
        .reusable::<donder_model::GradientDefinition>()
        .iter()
        .map(|(id, definition)| {
            let identity = crate::gui::model::declared_identity(session, *id);
            (identity, definition)
        })
        .map(|(identity, definition)| SequenceGradientLibraryItem {
            path: identity.document().to_string(),
            object_key: identity.object().to_string(),
            display_name: identity.object().to_string(),
            gradient: gradient_dto(&definition.gradient),
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
        Type::Text => SequenceParamKind::Text,
        Type::Font => SequenceParamKind::Font,
        Type::Image => SequenceParamKind::Image,
        Type::Track => return None,
        Type::Curve => SequenceParamKind::Curve,
        Type::Gradient => SequenceParamKind::Gradient,
        Type::Array(element) => SequenceParamKind::Array {
            element: Box::new(param_kind(element)?),
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

/// A declared parameter, valued at `value`, as the inspector shows it.
fn param_to_gui(
    session: &ProjectSession,
    param: &ParamDecl,
    value: SequenceParamValue,
    automation: Option<SequenceParamAutomation>,
) -> Option<SequenceClipParam> {
    let new_item = match (
        &param.ty,
        ParamValue::initial_for_param(param, NEW_ITEM_COLOR),
    ) {
        (Type::Array(item), Some(ParamValue::Array(items))) => items
            .first()
            .map(|value| param_value_to_gui(session, value, item, &[])),
        _ => None,
    };
    Some(SequenceClipParam {
        supports_automation: param.supports_automation(),
        name: param.name.as_str().to_string(),
        description: param.description.clone(),
        kind: param_kind(&param.ty)?,
        options: param_options(session, param),
        range: param_range(param),
        editable: automation.is_none(),
        automation,
        value,
        new_item,
    })
}

/// The color a new item of a color or gradient array starts with.
const NEW_ITEM_COLOR: donder_runtime_types::Color = donder_runtime_types::Color {
    red: 1.0,
    green: 1.0,
    blue: 1.0,
};

/// A parameter's value when nothing overrides it: its default, else its
/// type's. An enum option starts with its fields' defaults.
fn declared_value(session: &ProjectSession, param: &ParamDecl) -> Option<SequenceParamValue> {
    match param
        .default
        .clone()
        .unwrap_or_else(|| param.ty.default_value())
    {
        EffectValue::Enum(option) => Some(param_value_to_gui(
            session,
            &ParamValue::option(param, option),
            &param.ty,
            &param.option_fields,
        )),
        value => default_param_value(&value, &param.ty),
    }
}

fn param_options(session: &ProjectSession, param: &ParamDecl) -> Vec<SequenceParamOption> {
    match &param.ty {
        Type::Enum(options) => options
            .iter()
            .map(|option| SequenceParamOption {
                name: option.as_str().to_string(),
                fields: param
                    .fields_of(option)
                    .iter()
                    .filter_map(|field| {
                        param_to_gui(session, field, declared_value(session, field)?, None)
                    })
                    .collect(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `value` of type `ty`, whose enum options carry `option_fields`.
fn param_value_to_gui(
    session: &ProjectSession,
    value: &ParamValue,
    ty: &Type,
    option_fields: &[Vec<ParamDecl>],
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
        ParamValue::Enum { option, fields } => {
            let declared = match ty {
                Type::Enum(options) => options
                    .iter()
                    .position(|candidate| candidate == option)
                    .and_then(|index| option_fields.get(index))
                    .map_or(&[][..], Vec::as_slice),
                _ => &[],
            };
            SequenceParamValue::Enum {
                value: option.as_str().to_string(),
                fields: fields
                    .iter()
                    .zip(declared)
                    .map(|((name, value), field)| SequenceParamField {
                        name: name.as_str().to_string(),
                        value: param_value_to_gui(session, value, &field.ty, &field.option_fields),
                    })
                    .collect(),
            }
        }
        ParamValue::Marks(value) => SequenceParamValue::Marks {
            id: value.map(|collection| collection.0),
        },
        ParamValue::Curve(source) => SequenceParamValue::Curve {
            value: SequenceCurveValue {
                points: curve_points(match source {
                    CurveValue::Literal(curve) => curve,
                    CurveValue::Linked(id) => {
                        &session
                            .project()
                            .reusable::<donder_model::CurveDefinition>()[id]
                            .curve
                    }
                }),
                link: match source {
                    CurveValue::Literal(_) => SequenceLibraryLink::Inline,
                    CurveValue::Linked(id) => {
                        library_link(&crate::gui::model::declared_identity(session, *id))
                    }
                },
            },
        },
        ParamValue::Gradient(source) => SequenceParamValue::Gradient {
            value: SequenceGradientValue {
                gradient: gradient_dto(match source {
                    GradientValue::Literal(gradient) => gradient,
                    GradientValue::Linked(id) => {
                        &session
                            .project()
                            .reusable::<donder_model::GradientDefinition>()[id]
                            .gradient
                    }
                }),
                link: match source {
                    GradientValue::Literal(_) => SequenceLibraryLink::Inline,
                    GradientValue::Linked(id) => {
                        library_link(&crate::gui::model::declared_identity(session, *id))
                    }
                },
            },
        },
        ParamValue::Array(values) => array_param_value(session, values, ty),
        ParamValue::Text(value) => SequenceParamValue::Text {
            value: value.clone(),
        },
        ParamValue::Font(face) => SequenceParamValue::Font {
            value: face.as_ref().map(|face| crate::dto::SequenceFont {
                file: session
                    .asset(&face.file)
                    .map(|asset| asset.relative_path.to_string())
                    .unwrap_or_else(|| unreachable!("validated font assets are referenced")),
                height: face.height,
            }),
        },
        ParamValue::Image(image) => SequenceParamValue::Image {
            value: image.as_ref().map(|image| crate::dto::SequenceImage {
                file: session
                    .asset(&image.file)
                    .map(|asset| asset.relative_path.to_string())
                    .unwrap_or_else(|| unreachable!("validated image assets are referenced")),
                size: image.size,
            }),
        },
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
            fields: Vec::new(),
        },
        EffectValue::Marks(_) => SequenceParamValue::Marks { id: None },
        EffectValue::Font(_) => SequenceParamValue::Font { value: None },
        EffectValue::Image(_) => SequenceParamValue::Image { value: None },
        EffectValue::Array(codes) if *ty == Type::Text => SequenceParamValue::Text {
            value: codes
                .iter()
                .filter_map(|code| match code {
                    EffectValue::Int(code) => char::from_u32(*code as u32),
                    _ => None,
                })
                .collect(),
        },
        EffectValue::Track(_) => return None,
        EffectValue::Curve(curve) => SequenceParamValue::Curve {
            value: SequenceCurveValue {
                points: curve_points(curve),
                link: SequenceLibraryLink::Inline,
            },
        },
        EffectValue::Gradient(gradient) => SequenceParamValue::Gradient {
            value: SequenceGradientValue {
                gradient: gradient_dto(gradient),
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
            SequenceParamValue::Array { values: converted }
        }
        EffectValue::Void => return None,
    })
}

fn library_link(id: &donder_project_io::SourceIdentity) -> SequenceLibraryLink {
    SequenceLibraryLink::Library {
        path: id.document().to_string(),
        object_key: id.object().to_string(),
        display_name: id.object().to_string(),
    }
}

use crate::gui::model::{curve_points, gradient_dto};

fn array_param_value(
    session: &ProjectSession,
    values: &[ParamValue],
    ty: &Type,
) -> SequenceParamValue {
    let Type::Array(inner) = ty else {
        unreachable!("validated array parameter type");
    };
    SequenceParamValue::Array {
        values: values
            .iter()
            .map(|value| param_value_to_gui(session, value, inner, &[]))
            .collect(),
    }
}

use donder_language::compiler::{DeclarationKind, ParamDecl, ParamRange};
use donder_model::{
    AutomationTarget, CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind,
};
use donder_model::{CurveValue, GradientValue, ParamValue};
use donder_model::{GraphOperatorNode, ScriptDefinition, ScriptId};
use donder_project_io::ProjectSession;
use donder_runtime_types::{Identifier, Type, Value as EffectValue};

use crate::dto::{
    SequenceClipParam, SequenceCompositionGraph, SequenceCurveLibraryItem, SequenceCurveValue,
    SequenceGradientLibraryItem, SequenceGradientValue, SequenceGraphEdge, SequenceGraphNode,
    SequenceGraphNodeKind, SequenceGraphOperator, SequenceGraphOperatorDefinition, SequenceLayer,
    SequenceLibraryLink, SequenceParamAutomation, SequenceParamField, SequenceParamKind,
    SequenceParamOption, SequenceParamRange, SequenceParamValue,
};

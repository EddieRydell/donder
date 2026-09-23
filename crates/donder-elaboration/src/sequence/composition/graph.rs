use donder_language::dsl::{BoundParams, BytecodeProgram, ParamDecl};
use donder_language::operator::{OperatorImplementation, validate_composition_graph};
use donder_language::sequence::{
    AutomationTarget, CompositionGraphNodeId, CompositionGraphNodeKind, GraphPortId, Sequence,
    SequenceCompositionGraph,
};
use donder_runtime::signal::{
    PreparedOperator, PreparedOperatorNode, PreparedSignalKind, PreparedSignalNode, SignalPlan,
};
use indexmap::{IndexMap, IndexSet};
use std::sync::Arc;

use crate::sequence::effects::parameters::prepare_operator_params;
use crate::sequence::effects::preparation::prepare_automation;
use crate::sequence::targets::PreparedTargetCache;
use crate::sequence::targets::full_rig_target_pixels;
use crate::{EffectParamTiming, PreparedAutomation, PreparedFixture, RenderError};
use donder_language::model::DonderProject;

pub(crate) fn automation_for_composition_node(
    sequence: &Sequence,
    node_id: &CompositionGraphNodeId,
    params: &[ParamDecl],
) -> Result<Vec<PreparedAutomation>, RenderError> {
    sequence
        .automation_clips
        .iter()
        .flat_map(|clip| {
            clip.bindings
                .iter()
                .filter(move |binding| {
                    matches!(
                        &binding.target,
                        AutomationTarget::CompositionNodeParam {
                            node_id: target_node_id,
                            ..
                        } if target_node_id == node_id
                    )
                })
                .map(move |binding| prepare_automation(clip, binding, params))
        })
        .collect()
}

pub(crate) struct PrepareGraphContext<'a> {
    pub(crate) project: &'a DonderProject,
    pub(crate) sequence: &'a Sequence,
    pub(crate) fixtures: &'a [PreparedFixture],
    pub(crate) programs: &'a mut Vec<BytecodeProgram>,
    pub(crate) targets: &'a mut PreparedTargetCache,
}

pub(crate) fn prepare_signal_graph(
    context: PrepareGraphContext<'_>,
    graph: &SequenceCompositionGraph,
) -> Result<SignalPlan, RenderError> {
    let full_target = context
        .targets
        .sample_target(Arc::from(full_rig_target_pixels(context.fixtures)?))?;
    validate_composition_graph(graph, &context.project.definitions.operators).map_err(|error| {
        RenderError::BadGraph {
            message: error.message,
        }
    })?;
    validate_composition_graph_layers(context.sequence, graph)?;
    let node_ids = composition_graph_node_ids(graph)?;
    let node_indexes = node_ids
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, node_id)| (node_id, index))
        .collect::<IndexMap<_, _>>();
    let node_order = topological_composition_graph_order(&node_ids, &node_indexes, graph)?;
    let mut incoming = vec![Vec::<(GraphPortId, usize)>::new(); node_ids.len()];
    for edge in &graph.edges {
        let from = node_index(&node_indexes, &edge.from)?;
        let to = node_index(&node_indexes, &edge.to)?;
        incoming[to].push((edge.to_port.clone(), from));
    }

    let mut prepared_nodes = Vec::<PreparedSignalNode>::new();
    let mut operator_programs = Vec::new();
    let mut automation_count = 0usize;
    let mut prepared_index_by_node = vec![usize::MAX; node_ids.len()];
    for node_index in &node_order {
        let node_id = &node_ids[*node_index];
        let node = graph_node(graph, node_id)?;
        let prepared = match &node.kind {
            CompositionGraphNodeKind::Layer { layer_id } => {
                let layer_index = context
                    .sequence
                    .layers
                    .iter()
                    .position(|layer| layer.id == *layer_id)
                    .ok_or_else(|| RenderError::BadGraph {
                        message: format!(
                            "composition graph references missing layer {}",
                            layer_id.0
                        ),
                    })?;
                PreparedSignalNode {
                    kind: PreparedSignalKind::Layer { layer_index },
                }
            }
            CompositionGraphNodeKind::Operator(operator_node) => {
                let definition = context
                    .project
                    .definitions
                    .operators
                    .resolve(&operator_node.operator)
                    .ok_or_else(|| RenderError::BadGraph {
                        message: "missing operator definition".to_string(),
                    })?;
                let inputs = definition
                    .inputs
                    .iter()
                    .map(|port| {
                        incoming[*node_index]
                            .iter()
                            .find_map(|(input_port, node)| {
                                (input_port.0 == port.source_name).then_some(*node)
                            })
                            .ok_or_else(|| RenderError::BadGraph {
                                message: format!(
                                    "composition graph input port `{}` is not connected",
                                    port.source_name
                                ),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|input| {
                        let prepared_index = prepared_index_by_node[input];
                        (prepared_index != usize::MAX)
                            .then_some(prepared_index)
                            .ok_or_else(|| RenderError::BadGraph {
                                message: "composition graph order did not prepare an input first"
                                    .to_string(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let params = prepare_operator_params(
                    context.project,
                    context.sequence,
                    definition,
                    &operator_node.params,
                    EffectParamTiming {
                        start: donder_language::values::SampleTime::from_ticks(0),
                        duration: donder_language::values::sample_duration_from_donder_duration(
                            &context.sequence.duration,
                        )
                        .map_err(|_| RenderError::InvalidTiming {
                            reason: "sequence duration exceeds the runtime clock range".to_string(),
                        })?,
                    },
                )?;
                let automation = automation_for_composition_node(
                    context.sequence,
                    &node.id,
                    &definition.params,
                )?;
                let implementation = match &definition.implementation {
                    OperatorImplementation::Dsl(compiled) => {
                        let program = match operator_programs
                            .iter()
                            .find(|(operator, _)| operator == &operator_node.operator)
                        {
                            Some((_, index)) => *index,
                            None => {
                                let index =
                                    u32::try_from(context.programs.len()).map_err(|_| {
                                        RenderError::BadGraph {
                                            message:
                                                "prepared sequence has too many bytecode programs"
                                                    .to_string(),
                                        }
                                    })?;
                                context.programs.push(compiled.bytecode.clone());
                                operator_programs.push((operator_node.operator.clone(), index));
                                index
                            }
                        };
                        PreparedOperator::Dsl(program)
                    }
                    OperatorImplementation::Native(builtin) => PreparedOperator::Native(*builtin),
                };
                let operator = PreparedOperatorNode {
                    automation_slot: u32::try_from(automation_count).map_err(|_| {
                        RenderError::BadGraph {
                            message: "too many automated operators".to_string(),
                        }
                    })?,
                    implementation,
                    params: BoundParams::bind(&definition.params, &params)?,
                };
                automation_count += usize::from(!automation.is_empty());
                PreparedSignalNode {
                    kind: PreparedSignalKind::Operator {
                        operator,
                        inputs: inputs.into_boxed_slice(),
                        automation: automation.into_boxed_slice(),
                        vm_slot: 0,
                    },
                }
            }
            CompositionGraphNodeKind::Output => {
                let inputs = incoming[*node_index]
                    .iter()
                    .map(|(_, input)| {
                        let prepared_index = prepared_index_by_node[*input];
                        (prepared_index != usize::MAX)
                            .then_some(prepared_index)
                            .ok_or_else(|| RenderError::BadGraph {
                                message:
                                    "composition graph order did not prepare output input first"
                                        .to_string(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                PreparedSignalNode {
                    kind: PreparedSignalKind::Output {
                        inputs: inputs.into_boxed_slice(),
                    },
                }
            }
        };
        prepared_index_by_node[*node_index] = prepared_nodes.len();
        prepared_nodes.push(prepared);
    }

    let output_candidates = node_order
        .iter()
        .filter(|index| {
            matches!(
                graph_node(graph, &node_ids[**index]).map(|node| &node.kind),
                Ok(CompositionGraphNodeKind::Output)
            )
        })
        .copied()
        .collect::<Vec<_>>();
    let [output_source_index] = output_candidates.as_slice() else {
        return Err(RenderError::BadGraph {
            message: "composition graph must have exactly one output node".to_string(),
        });
    };
    let output_index = prepared_index_by_node[*output_source_index];
    if output_index == usize::MAX {
        return Err(RenderError::BadGraph {
            message: "composition graph output node is not in render order".to_string(),
        });
    }

    finish_signal_plan(prepared_nodes, output_index, full_target)
}

pub(crate) fn finish_signal_plan(
    mut prepared_nodes: Vec<PreparedSignalNode>,
    output_index: usize,
    target: u32,
) -> Result<SignalPlan, RenderError> {
    let mut vm_depths = Vec::with_capacity(prepared_nodes.len());
    let mut vm_workspace_count = 0;
    for node in &mut prepared_nodes {
        let (inputs, is_dsl, vm_slot) = match &mut node.kind {
            PreparedSignalKind::Layer { .. } => {
                vm_depths.push(0);
                continue;
            }
            PreparedSignalKind::Operator {
                operator,
                inputs,
                vm_slot,
                ..
            } => (
                &inputs[..],
                matches!(operator.implementation, PreparedOperator::Dsl(_)),
                Some(vm_slot),
            ),
            PreparedSignalKind::Output { inputs } => (&inputs[..], false, None),
        };
        let input_depth = inputs
            .iter()
            .filter_map(|input| vm_depths.get(*input))
            .copied()
            .max()
            .unwrap_or(0);
        if is_dsl {
            let slot = u16::try_from(input_depth).map_err(|_| RenderError::BadGraph {
                message: "composition graph DSL nesting exceeds the runtime range".to_string(),
            })?;
            let Some(vm_slot) = vm_slot else {
                return Err(RenderError::BadGraph {
                    message: "DSL operator is missing its VM workspace slot".to_string(),
                });
            };
            *vm_slot = slot;
            let depth = input_depth + 1;
            vm_workspace_count = vm_workspace_count.max(depth);
            vm_depths.push(depth);
        } else {
            vm_depths.push(input_depth);
        }
    }

    let (frame_nodes, frame_slots, frame_buffer_count) =
        prepare_frame_plan(&prepared_nodes, output_index)?;

    Ok(SignalPlan {
        output_index,
        target,
        nodes: prepared_nodes.into_boxed_slice(),
        vm_workspace_count,
        frame_nodes,
        frame_slots,
        frame_buffer_count,
    })
}

#[allow(clippy::type_complexity)]
fn prepare_frame_plan(
    nodes: &[PreparedSignalNode],
    output_index: usize,
) -> Result<(Box<[usize]>, Box<[u16]>, u16), RenderError> {
    let mut required = vec![false; nodes.len()];
    mark_frame_nodes(nodes, output_index, &mut required);
    let mut consumers = vec![0u16; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        if !required[index] {
            continue;
        }
        for input in frame_inputs(node) {
            consumers[*input] =
                consumers[*input]
                    .checked_add(1)
                    .ok_or_else(|| RenderError::BadGraph {
                        message: "composition graph has too many consumers for one signal"
                            .to_string(),
                    })?;
        }
    }
    let mut frame_nodes = Vec::new();
    let mut frame_slots = vec![u16::MAX; nodes.len()];
    let mut available = Vec::new();
    let mut frame_buffer_count = 0u16;
    for (index, node) in nodes.iter().enumerate() {
        if !required[index] {
            continue;
        }
        // A terminal single-input output is an alias, not another frame pass.
        // Keep the input slot live through evaluation's final output copy.
        if index == output_index
            && let PreparedSignalKind::Output { inputs } = &node.kind
            && let [input] = inputs.as_ref()
        {
            frame_slots[index] = frame_slots[*input];
            continue;
        }
        let slot = if let Some(slot) = available.pop() {
            slot
        } else {
            let slot = frame_buffer_count;
            frame_buffer_count =
                frame_buffer_count
                    .checked_add(1)
                    .ok_or_else(|| RenderError::BadGraph {
                        message: "composition graph needs too many frame buffers".to_string(),
                    })?;
            slot
        };
        frame_slots[index] = slot;
        frame_nodes.push(index);
        for input in frame_inputs(node) {
            consumers[*input] = consumers[*input].saturating_sub(1);
            if consumers[*input] == 0 {
                available.push(frame_slots[*input]);
            }
        }
    }
    Ok((
        frame_nodes.into_boxed_slice(),
        frame_slots.into_boxed_slice(),
        frame_buffer_count,
    ))
}

fn mark_frame_nodes(nodes: &[PreparedSignalNode], index: usize, required: &mut [bool]) {
    if required.get(index).copied().unwrap_or(false) {
        return;
    }
    let Some(node) = nodes.get(index) else {
        return;
    };
    required[index] = true;
    for input in frame_inputs(node) {
        mark_frame_nodes(nodes, *input, required);
    }
}

fn frame_inputs(node: &PreparedSignalNode) -> &[usize] {
    match &node.kind {
        PreparedSignalKind::Layer { .. } => &[],
        PreparedSignalKind::Operator {
            operator, inputs, ..
        } => match operator.implementation {
            PreparedOperator::Native(builtin) if builtin.resamples_time() => &[],
            PreparedOperator::Dsl(_) => &[],
            PreparedOperator::Native(_) => inputs,
        },
        PreparedSignalKind::Output { inputs } => inputs,
    }
}

fn composition_graph_node_ids(
    graph: &SequenceCompositionGraph,
) -> Result<Vec<CompositionGraphNodeId>, RenderError> {
    let mut ids = IndexSet::new();
    for node in &graph.nodes {
        if !ids.insert(node.id.clone()) {
            return Err(RenderError::BadGraph {
                message: format!("duplicate composition graph node {}", node.id.0),
            });
        }
    }
    Ok(ids.into_iter().collect())
}

fn validate_composition_graph_layers(
    sequence: &Sequence,
    graph: &SequenceCompositionGraph,
) -> Result<(), RenderError> {
    let mut graph_layer_ids = IndexSet::new();
    for node in &graph.nodes {
        let CompositionGraphNodeKind::Layer { layer_id } = &node.kind else {
            continue;
        };
        if !sequence.layers.iter().any(|layer| layer.id == *layer_id) {
            return Err(RenderError::BadGraph {
                message: format!(
                    "composition graph layer node references missing layer {}",
                    layer_id.0
                ),
            });
        }
        if !graph_layer_ids.insert(layer_id.clone()) {
            return Err(RenderError::BadGraph {
                message: format!(
                    "composition graph has duplicate layer node for layer {}",
                    layer_id.0
                ),
            });
        }
    }
    for layer in &sequence.layers {
        if !graph_layer_ids.contains(&layer.id) {
            return Err(RenderError::BadGraph {
                message: format!(
                    "composition graph is missing layer node for layer {}",
                    layer.id.0
                ),
            });
        }
    }
    Ok(())
}

fn node_index(
    indexes: &IndexMap<CompositionGraphNodeId, usize>,
    node_id: &CompositionGraphNodeId,
) -> Result<usize, RenderError> {
    indexes
        .get(node_id)
        .copied()
        .ok_or_else(|| RenderError::BadGraph {
            message: format!(
                "edge references missing composition graph node {}",
                node_id.0
            ),
        })
}

fn graph_node<'a>(
    graph: &'a SequenceCompositionGraph,
    id: &CompositionGraphNodeId,
) -> Result<&'a donder_language::sequence::CompositionGraphNode, RenderError> {
    graph
        .nodes
        .iter()
        .find(|node| node.id == *id)
        .ok_or_else(|| RenderError::BadGraph {
            message: format!("missing composition graph node {}", id.0),
        })
}

fn topological_composition_graph_order(
    node_ids: &[CompositionGraphNodeId],
    node_indexes: &IndexMap<CompositionGraphNodeId, usize>,
    graph: &SequenceCompositionGraph,
) -> Result<Vec<usize>, RenderError> {
    let mut indegree = vec![0usize; node_ids.len()];
    let mut outgoing = vec![Vec::<usize>::new(); node_ids.len()];
    for edge in &graph.edges {
        let from = node_index(node_indexes, &edge.from)?;
        let to = node_index(node_indexes, &edge.to)?;
        outgoing[from].push(to);
        indegree[to] += 1;
    }
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut order = Vec::with_capacity(node_ids.len());
    while let Some(index) = ready.pop() {
        order.push(index);
        for next in &outgoing[index] {
            indegree[*next] = indegree[*next].saturating_sub(1);
            if indegree[*next] == 0 {
                ready.push(*next);
            }
        }
    }
    if order.len() != node_ids.len() {
        return Err(RenderError::BadGraph {
            message: "composition graph contains a cycle".to_string(),
        });
    }
    Ok(order)
}

#[cfg(test)]
mod frame_plan_tests {
    use super::*;

    #[test]
    fn terminal_output_aliases_one_input_but_composes_multiple_inputs() {
        let mut nodes = vec![
            PreparedSignalNode {
                kind: PreparedSignalKind::Layer { layer_index: 0 },
            },
            PreparedSignalNode {
                kind: PreparedSignalKind::Output {
                    inputs: vec![0].into(),
                },
            },
        ];
        let (frames, slots, count) = prepare_frame_plan(&nodes, 1).unwrap();
        assert_eq!(&*frames, &[0]);
        assert_eq!(&*slots, &[0, 0]);
        assert_eq!(count, 1);

        nodes[1].kind = PreparedSignalKind::Layer { layer_index: 1 };
        nodes.push(PreparedSignalNode {
            kind: PreparedSignalKind::Output {
                inputs: vec![0, 1].into(),
            },
        });
        let (frames, slots, count) = prepare_frame_plan(&nodes, 2).unwrap();
        assert_eq!(&*frames, &[0, 1, 2]);
        assert_eq!(&*slots, &[0, 1, 2]);
        assert_eq!(count, 3);
    }
}

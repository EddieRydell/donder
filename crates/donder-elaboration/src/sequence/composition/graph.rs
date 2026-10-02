use donder_language::dsl::{BoundParams, BytecodeProgram, ParamDecl};
use donder_language::operator::{
    OperatorImplementation, OperatorRef, composition_graph_output_dependencies,
};
use donder_language::sequence::{
    AutomationTarget, CompositionGraphNodeId, CompositionGraphNodeKind, GraphPortId, Sequence,
    SequenceCompositionGraph,
};
use donder_runtime::signal::{
    PreparedAutomation, PreparedOperator, PreparedOperatorNode, PreparedSignalKind,
    PreparedSignalNode, SignalPlan,
};
use indexmap::IndexMap;
use std::sync::Arc;

use crate::sequence::effects::parameters::{EffectParamTiming, prepare_params};
use crate::sequence::effects::preparation::prepare_automation;
use crate::sequence::fixtures::PreparedFixture;
use crate::sequence::targets::PreparedTargetCache;
use crate::sequence::targets::full_rig_target_pixels;
use donder_language::model::DonderProject;

pub(crate) fn automation_for_composition_node(
    sequence: &Sequence,
    node_id: &CompositionGraphNodeId,
    params: &[ParamDecl],
) -> Vec<PreparedAutomation> {
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
) -> SignalPlan {
    let full_target = context
        .targets
        .sample_target(Arc::from(full_rig_target_pixels(context.fixtures)));
    // Validation already established unique IDs, connected required inputs, and
    // an acyclic graph with one output. Lower those references to vector indices.
    let node_indexes = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect::<IndexMap<_, _>>();
    let node_order = topological_composition_graph_order(&node_indexes, graph);
    let layer_indexes = context
        .sequence
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| (&layer.id, index))
        .collect::<IndexMap<_, _>>();
    let output_dependencies = composition_graph_output_dependencies(graph);
    let mut incoming = vec![Vec::<(GraphPortId, usize)>::new(); graph.nodes.len()];
    for edge in &graph.edges {
        let from = node_indexes[&edge.from];
        let to = node_indexes[&edge.to];
        incoming[to].push((edge.to_port.clone(), from));
    }

    let mut prepared_nodes = Vec::<PreparedSignalNode>::new();
    let mut operator_programs = Vec::new();
    let mut automation_count = 0usize;
    let mut prepared_index_by_node = vec![usize::MAX; graph.nodes.len()];
    let mut output_index = 0;
    for node_index in &node_order {
        let node = &graph.nodes[*node_index];
        if !output_dependencies.contains(&node.id) {
            continue;
        }
        let prepared = match &node.kind {
            CompositionGraphNodeKind::Layer { layer_id } => {
                let layer_index = layer_indexes[layer_id];
                PreparedSignalNode {
                    kind: PreparedSignalKind::Layer { layer_index },
                }
            }
            CompositionGraphNodeKind::Operator(operator_node) => {
                let OperatorRef::Custom(id) = &operator_node.operator;
                let definition = &context.project.definitions.operators.definitions[id];
                let ports = incoming[*node_index]
                    .iter()
                    .map(|(port, input)| (port.0.as_str(), *input))
                    .collect::<IndexMap<_, _>>();
                let inputs = definition
                    .inputs
                    .iter()
                    .map(|port| prepared_index_by_node[ports[port.source_name.as_str()]])
                    .collect::<Vec<_>>();
                let params = prepare_params(
                    context.project,
                    context.sequence,
                    &definition.params,
                    &operator_node.params,
                    EffectParamTiming {
                        start: donder_language::values::SampleTime::from_ticks(0),
                        duration: donder_language::values::SampleDuration::from_ticks(
                            context.sequence.duration.as_micros_rounded() as u32,
                        ),
                    },
                );
                let automation =
                    automation_for_composition_node(context.sequence, &node.id, &definition.params);
                let implementation = match &definition.implementation {
                    OperatorImplementation::Dsl(compiled) => {
                        let program = match operator_programs
                            .iter()
                            .find(|(operator, _)| operator == &operator_node.operator)
                        {
                            Some((_, index)) => *index,
                            None => {
                                let index = context.programs.len();
                                context.programs.push(compiled.bytecode.clone());
                                operator_programs.push((operator_node.operator.clone(), index));
                                index
                            }
                        };
                        PreparedOperator::Dsl(program)
                    }
                };
                let operator = PreparedOperatorNode {
                    automation_slot: automation_count,
                    implementation,
                    params: BoundParams::from_values(
                        definition
                            .params
                            .iter()
                            .map(|param| (&param.ty, params[&param.name].clone())),
                        &mut Default::default(),
                    ),
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
                output_index = prepared_nodes.len();
                let inputs = incoming[*node_index]
                    .iter()
                    .map(|(_, input)| prepared_index_by_node[*input])
                    .collect::<Vec<_>>();
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

    finish_signal_plan(prepared_nodes, output_index, full_target)
}

pub(crate) fn finish_signal_plan(
    mut prepared_nodes: Vec<PreparedSignalNode>,
    output_index: usize,
    target: usize,
) -> SignalPlan {
    let mut vm_depths = Vec::with_capacity(prepared_nodes.len());
    let mut vm_workspace_count = 0;
    for node in &mut prepared_nodes {
        let (inputs, vm_slot) = match &mut node.kind {
            PreparedSignalKind::Layer { .. } => {
                vm_depths.push(0);
                continue;
            }
            PreparedSignalKind::Operator {
                inputs, vm_slot, ..
            } => (&inputs[..], Some(vm_slot)),
            PreparedSignalKind::Output { inputs } => (&inputs[..], None),
        };
        let input_depth = inputs
            .iter()
            .filter_map(|input| vm_depths.get(*input))
            .copied()
            .max()
            .unwrap_or(0);
        if let Some(vm_slot) = vm_slot {
            *vm_slot = input_depth;
            let depth = input_depth + 1;
            vm_workspace_count = vm_workspace_count.max(depth);
            vm_depths.push(depth);
        } else {
            vm_depths.push(input_depth);
        }
    }

    let (frame_nodes, frame_slots, frame_buffer_count) =
        prepare_frame_plan(&prepared_nodes, output_index);

    SignalPlan {
        output_index,
        target,
        nodes: prepared_nodes.into_boxed_slice(),
        vm_workspace_count,
        frame_nodes,
        frame_slots,
        frame_buffer_count,
    }
}

#[allow(clippy::type_complexity)]
fn prepare_frame_plan(
    nodes: &[PreparedSignalNode],
    output_index: usize,
) -> (Box<[usize]>, Box<[usize]>, usize) {
    let mut required = vec![false; nodes.len()];
    required[output_index] = true;
    // Lowering orders dependencies before consumers, so one backwards pass
    // discovers frame dependencies without recursive traversal.
    for index in (0..nodes.len()).rev() {
        if required[index] {
            for &input in frame_inputs(&nodes[index]) {
                required[input] = true;
            }
        }
    }
    let mut consumers = vec![0usize; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        if !required[index] {
            continue;
        }
        for input in frame_inputs(node) {
            consumers[*input] += 1;
        }
    }
    let mut frame_nodes = Vec::new();
    let mut frame_slots = vec![usize::MAX; nodes.len()];
    let mut available = Vec::new();
    let mut frame_buffer_count = 0usize;
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
            frame_buffer_count += 1;
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
    (
        frame_nodes.into_boxed_slice(),
        frame_slots.into_boxed_slice(),
        frame_buffer_count,
    )
}

fn frame_inputs(node: &PreparedSignalNode) -> &[usize] {
    match &node.kind {
        PreparedSignalKind::Layer { .. } => &[],
        PreparedSignalKind::Operator { .. } => &[],
        PreparedSignalKind::Output { inputs } => inputs,
    }
}

/// Order dependencies before their consumers. The loaded graph is already acyclic.
fn topological_composition_graph_order(
    node_indexes: &IndexMap<CompositionGraphNodeId, usize>,
    graph: &SequenceCompositionGraph,
) -> Vec<usize> {
    let mut indegree = vec![0usize; graph.nodes.len()];
    let mut outgoing = vec![Vec::<usize>::new(); graph.nodes.len()];
    for edge in &graph.edges {
        let from = node_indexes[&edge.from];
        let to = node_indexes[&edge.to];
        outgoing[from].push(to);
        indegree[to] += 1;
    }
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut order = Vec::with_capacity(graph.nodes.len());
    while let Some(index) = ready.pop() {
        order.push(index);
        for next in &outgoing[index] {
            indegree[*next] -= 1;
            if indegree[*next] == 0 {
                ready.push(*next);
            }
        }
    }
    order
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
        let (frames, slots, count) = prepare_frame_plan(&nodes, 1);
        assert_eq!(&*frames, &[0]);
        assert_eq!(&*slots, &[0, 0]);
        assert_eq!(count, 1);

        nodes[1].kind = PreparedSignalKind::Layer { layer_index: 1 };
        nodes.push(PreparedSignalNode {
            kind: PreparedSignalKind::Output {
                inputs: vec![0, 1].into(),
            },
        });
        let (frames, slots, count) = prepare_frame_plan(&nodes, 2);
        assert_eq!(&*frames, &[0, 1, 2]);
        assert_eq!(&*slots, &[0, 1, 2]);
        assert_eq!(count, 3);
    }
}

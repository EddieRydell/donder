//! The global signal graph of one sequence: layers, operator instances and the
//! output. Several sources into one port, the output's included, combine by
//! their native channel-wise maximum. Before lowering, black signals are
//! folded into their consumers and an operator consumed once is fused into its
//! consumer, so the boundary between them disappears from the prepared
//! programs.
use donder_language::compiler::Instance;
use donder_model::composition_graph_output_dependencies;
use donder_model::{AcceptedOperatorInputs, SequenceCompositionGraph};
use donder_model::{CompositionGraphNodeKind, SequenceLayerId};
use donder_runtime::{SequenceBuilder, SequenceRoot, SignalHandle};
use donder_runtime_types::Color;
use indexmap::IndexMap;
use std::collections::HashSet;

enum Pending<'id> {
    /// Black at every time and pixel.
    Black,
    Layer(SignalHandle<'id>),
    Operator {
        instance: Box<Instance>,
        inputs: Vec<usize>,
    },
    /// Several signals into one port.
    Mix(Vec<usize>),
}

pub(crate) fn prepare<'id, 'a>(
    builder: &mut SequenceBuilder<'id>,
    graph: &SequenceCompositionGraph,
    operators: impl Iterator<Item = AcceptedOperatorInputs<'a>>,
    duration: donder_runtime_types::SampleDuration,
    layers: &IndexMap<&SequenceLayerId, SignalHandle<'id>>,
    black: &HashSet<&SequenceLayerId>,
    programs: &mut super::programs::Programs,
) -> SequenceRoot<'id> {
    let indexes = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (&node.id, index))
        .collect::<IndexMap<_, _>>();
    let operators = operators
        .map(|operator| (&operator.node().id, operator))
        .collect::<IndexMap<_, _>>();
    let dependencies = composition_graph_output_dependencies(graph);
    let mut incoming = vec![Vec::new(); graph.nodes.len()];
    let mut outgoing = vec![Vec::new(); graph.nodes.len()];
    let mut indegree = vec![0usize; graph.nodes.len()];
    let mut consumers = vec![0usize; graph.nodes.len()];
    for edge in &graph.edges {
        let from = indexes[&edge.from];
        let to = indexes[&edge.to];
        incoming[to].push((&edge.to_port, from));
        outgoing[from].push(to);
        indegree[to] += 1;
        if dependencies.contains(&graph.nodes[to].id) {
            consumers[from] += 1;
        }
    }
    let mut output = None;
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, &count)| (count == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut pending: Vec<Option<Pending<'id>>> = (0..graph.nodes.len()).map(|_| None).collect();
    let mut order = Vec::new();
    let duration = donder_runtime_types::sample_duration_seconds_f32(duration);
    // Admission guarantees one output, connected operator ports and an acyclic graph.
    // Mixes follow the graph's nodes in `pending`.
    while let Some(index) = ready.pop() {
        let node = &graph.nodes[index];
        if dependencies.contains(&node.id) {
            pending[index] = match &node.kind {
                CompositionGraphNodeKind::Layer { layer_id } if black.contains(layer_id) => {
                    Some(Pending::Black)
                }
                CompositionGraphNodeKind::Layer { layer_id } => {
                    Some(Pending::Layer(layers[layer_id]))
                }
                CompositionGraphNodeKind::Operator(_) => {
                    let operator = operators[&node.id];
                    let instance = operator.invocation().instance(
                        donder_language::compiler::ProgramConstants {
                            duration_seconds: Some(duration),
                            ..Default::default()
                        },
                    );
                    let inputs = operator
                        .definition()
                        .inputs()
                        .iter()
                        .map(|input| {
                            let sources = incoming[index]
                                .iter()
                                .filter(|(port, _)| port.0 == input.source_name)
                                .map(|(_, source)| *source)
                                .collect();
                            fan_in(sources, &mut pending, &mut order)
                        })
                        .collect::<Vec<_>>();
                    Some(simplify(instance, inputs, &mut pending, &consumers))
                }
                CompositionGraphNodeKind::Output => {
                    let sources = incoming[index].iter().map(|(_, source)| *source).collect();
                    output = Some(fan_in(sources, &mut pending, &mut order));
                    None
                }
            };
            order.push(index);
        }
        for &next in &outgoing[index] {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                ready.push(next);
            }
        }
    }
    // Assemble only retained nodes/programs. Building an upstream node and then
    // bypassing it would leave its bindings and workspace requirements resident.
    let mut signals = IndexMap::new();
    for index in order {
        match pending[index].take() {
            Some(Pending::Layer(signal)) => {
                signals.insert(index, signal);
            }
            Some(Pending::Operator { instance, inputs }) => {
                let invocation = programs.operator(&instance);
                let signal = builder.operator(&invocation, |input| signals[&inputs[input]]);
                signals.insert(index, signal);
            }
            Some(Pending::Mix(inputs)) => {
                let signal = builder.mix(inputs.iter().map(|input| signals[input]));
                signals.insert(index, signal);
            }
            Some(Pending::Black) | None => {}
        }
    }
    builder.output(output.and_then(|output| signals.get(&output).copied()))
}

/// The one signal the `sources` into a port make: their maximum, without
/// the black ones.
fn fan_in(
    sources: Vec<usize>,
    pending: &mut Vec<Option<Pending<'_>>>,
    order: &mut Vec<usize>,
) -> usize {
    let mut lit = sources
        .into_iter()
        .filter(|&source| !matches!(pending[source], Some(Pending::Black)))
        .collect::<Vec<_>>();
    if lit.len() == 1 {
        return lit.remove(0);
    }
    pending.push(Some(if lit.is_empty() {
        Pending::Black
    } else {
        Pending::Mix(lit)
    }));
    order.push(pending.len() - 1);
    pending.len() - 1
}

/// Fold black inputs into an operator and fuse sources consumed only by it.
fn simplify<'id>(
    mut instance: Instance,
    mut inputs: Vec<usize>,
    pending: &mut [Option<Pending<'id>>],
    consumers: &[usize],
) -> Pending<'id> {
    let mut input = 0;
    while input < inputs.len() {
        let source = inputs[input];
        match &pending[source] {
            Some(Pending::Black) => {
                instance = instance.with_black_input(input);
                inputs.remove(input);
                continue;
            }
            Some(Pending::Operator {
                instance: upstream,
                inputs: upstream_inputs,
            }) if consumers[source] == 1 => {
                if let Some(fused) = instance.fuse_input(input, upstream) {
                    let upstream_inputs = upstream_inputs.clone();
                    instance = fused;
                    inputs.remove(input);
                    inputs.extend(upstream_inputs);
                    pending[source] = None;
                    continue;
                }
            }
            _ => {}
        }
        input += 1;
    }
    if instance.constant_color() == Some(Color::BLACK)
        || (inputs.is_empty() && instance.stays_black())
    {
        Pending::Black
    } else {
        Pending::Operator {
            instance: Box::new(instance),
            inputs,
        }
    }
}

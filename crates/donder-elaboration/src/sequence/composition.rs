//! The global signal graph of one sequence: layers, operator instances and the
//! output. Before lowering, black signals are folded into their consumers and
//! an operator consumed once is fused into its consumer, so the boundary
//! between them disappears from the prepared programs.
use donder_language::dsl::Instance;
use donder_language::model::AcceptedSequence;
use donder_language::operator::composition_graph_output_dependencies;
use donder_language::sequence::{CompositionGraphNodeKind, SequenceLayerId};
use donder_language::values::Color;
use donder_runtime::{SequenceBuilder, SequenceRoot, SignalHandle};
use indexmap::IndexMap;
use std::collections::HashSet;

enum Pending<'id> {
    /// Black at every time and pixel.
    Black,
    Layer(SignalHandle<'id>),
    Operator {
        instance: Instance,
        inputs: Vec<usize>,
    },
    Output(Vec<usize>),
}

pub(super) fn prepare<'id>(
    builder: &mut SequenceBuilder<'id>,
    accepted: AcceptedSequence<'_>,
    layers: &IndexMap<&SequenceLayerId, SignalHandle<'id>>,
    black: &HashSet<&SequenceLayerId>,
    programs: &mut super::programs::Programs,
) -> SequenceRoot<'id> {
    let graph = &accepted.sequence().composition_graph;
    let indexes = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (&node.id, index))
        .collect::<IndexMap<_, _>>();
    let operators = accepted
        .operators()
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
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, &count)| (count == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut pending: Vec<Option<Pending<'id>>> = (0..graph.nodes.len()).map(|_| None).collect();
    let mut order = Vec::new();
    let duration =
        donder_language::values::sample_duration_seconds_f32(accepted.timing().duration());
    // Admission guarantees one output, connected operator ports and an acyclic graph.
    while let Some(index) = ready.pop() {
        let node = &graph.nodes[index];
        if dependencies.contains(&node.id) {
            pending[index] = Some(match &node.kind {
                CompositionGraphNodeKind::Layer { layer_id } if black.contains(layer_id) => {
                    Pending::Black
                }
                CompositionGraphNodeKind::Layer { layer_id } => Pending::Layer(layers[layer_id]),
                CompositionGraphNodeKind::Operator(_) => {
                    let operator = operators[&node.id];
                    let ports = incoming[index]
                        .iter()
                        .map(|(port, source)| (port.0.as_str(), *source))
                        .collect::<IndexMap<_, _>>();
                    let instance =
                        operator
                            .invocation()
                            .instance(donder_language::dsl::ProgramConstants {
                                duration_seconds: Some(duration),
                                ..Default::default()
                            });
                    let inputs = operator
                        .definition()
                        .inputs()
                        .iter()
                        .map(|input| ports[input.source_name.as_str()])
                        .collect::<Vec<_>>();
                    simplify(instance, inputs, &mut pending, &consumers)
                }
                CompositionGraphNodeKind::Output => {
                    Pending::Output(incoming[index].iter().map(|(_, input)| *input).collect())
                }
            });
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
    let mut output_inputs = Vec::new();
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
            Some(Pending::Output(inputs)) => output_inputs.extend(
                inputs
                    .iter()
                    .filter_map(|input| signals.get(input).copied()),
            ),
            Some(Pending::Black) | None => {}
        }
    }
    builder.output(output_inputs)
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
    if instance.constant_color() == Some(Color::BLACK) {
        Pending::Black
    } else {
        Pending::Operator { instance, inputs }
    }
}

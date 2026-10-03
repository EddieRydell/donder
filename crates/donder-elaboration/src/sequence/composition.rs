use donder_language::model::AcceptedSequence;
use donder_language::operator::composition_graph_output_dependencies;
use donder_language::sequence::{CompositionGraphNodeKind, SequenceLayerId};
use donder_runtime::{SequenceBuilder, SequenceRoot, SignalHandle};
use indexmap::IndexMap;

enum Pending<'id> {
    Layer(SignalHandle<'id>),
    Operator {
        invocation: donder_language::dsl::OperatorInvocation,
        inputs: Vec<usize>,
    },
    Output(Vec<usize>),
}

pub(super) fn prepare<'id>(
    builder: &mut SequenceBuilder<'id>,
    accepted: AcceptedSequence<'_>,
    layers: &IndexMap<&SequenceLayerId, SignalHandle<'id>>,
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
    // Admission guarantees one output, connected operator ports and an acyclic graph.
    while let Some(index) = ready.pop() {
        let node = &graph.nodes[index];
        if dependencies.contains(&node.id) {
            match &node.kind {
                CompositionGraphNodeKind::Layer { layer_id } => {
                    pending[index] = Some(Pending::Layer(layers[layer_id]));
                }
                CompositionGraphNodeKind::Operator(_) => {
                    let operator = operators[&node.id];
                    let ports = incoming[index]
                        .iter()
                        .map(|(port, source)| (port.0.as_str(), *source))
                        .collect::<IndexMap<_, _>>();
                    let mut invocation = programs.operator(
                        operator.invocation(),
                        donder_language::dsl::ProgramConstants {
                            duration_seconds: Some(
                                donder_language::values::sample_duration_seconds_f32(
                                    accepted.timing().duration(),
                                ),
                            ),
                            ..Default::default()
                        },
                    );
                    let mut inputs = operator
                        .definition()
                        .inputs()
                        .iter()
                        .map(|input| ports[input.source_name.as_str()])
                        .collect::<Vec<_>>();
                    let mut input = 0;
                    while input < inputs.len() {
                        let source = inputs[input];
                        let merged = if consumers[source] == 1
                            && let Some(Pending::Operator {
                                invocation: upstream,
                                inputs: upstream_inputs,
                            }) = &pending[source]
                        {
                            invocation
                                .fuse_input(input, upstream)
                                .map(|invocation| (invocation, upstream_inputs.clone()))
                        } else {
                            None
                        };
                        if let Some((merged, upstream_inputs)) = merged {
                            invocation = programs.operator(&merged, Default::default());
                            inputs.remove(input);
                            inputs.extend(upstream_inputs);
                            pending[source] = None;
                        } else {
                            input += 1;
                        }
                    }
                    pending[index] = Some(Pending::Operator { invocation, inputs });
                }
                CompositionGraphNodeKind::Output => {
                    pending[index] = Some(Pending::Output(
                        incoming[index].iter().map(|(_, input)| *input).collect(),
                    ));
                }
            }
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
            Some(Pending::Operator { invocation, inputs }) => {
                let signal = builder.operator(&invocation, |input| signals[&inputs[input]]);
                signals.insert(index, signal);
            }
            Some(Pending::Output(inputs)) => {
                output_inputs.extend(inputs.iter().map(|input| signals[input]))
            }
            None => {}
        }
    }
    builder.output(output_inputs)
}

use donder_language::model::AcceptedSequence;
use donder_language::operator::composition_graph_output_dependencies;
use donder_language::sequence::{CompositionGraphNodeKind, SequenceLayerId};
use donder_runtime::{SequenceBuilder, SequenceRoot, SignalHandle};
use indexmap::IndexMap;

pub(super) fn prepare<'id>(
    builder: &mut SequenceBuilder<'id>,
    accepted: AcceptedSequence<'_>,
    layers: &IndexMap<&SequenceLayerId, SignalHandle<'id>>,
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
    for edge in &graph.edges {
        let from = indexes[&edge.from];
        let to = indexes[&edge.to];
        incoming[to].push((&edge.to_port, from));
        outgoing[from].push(to);
        indegree[to] += 1;
    }
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, &count)| (count == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut signals = IndexMap::new();
    let mut output_inputs = Vec::new();
    // Admission guarantees one output, connected operator ports and an acyclic graph.
    while let Some(index) = ready.pop() {
        let node = &graph.nodes[index];
        if dependencies.contains(&node.id) {
            match &node.kind {
                CompositionGraphNodeKind::Layer { layer_id } => {
                    signals.insert(index, layers[layer_id]);
                }
                CompositionGraphNodeKind::Operator(_) => {
                    let operator = operators[&node.id];
                    let ports = incoming[index]
                        .iter()
                        .map(|(port, source)| (port.0.as_str(), *source))
                        .collect::<IndexMap<_, _>>();
                    let signal = builder.operator(operator.invocation(), |input| {
                        let name = &operator.definition().inputs()[input].source_name;
                        signals[&ports[name.as_str()]]
                    });
                    signals.insert(index, signal);
                }
                CompositionGraphNodeKind::Output => {
                    output_inputs.extend(incoming[index].iter().map(|(_, input)| signals[input]));
                }
            }
        }
        for &next in &outgoing[index] {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                ready.push(next);
            }
        }
    }
    builder.output(output_inputs)
}

use donder_language::compiler::{CompiledDefinition, compile_script};
use donder_model::{
    CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
    GraphNodePosition, GraphPortId, SequenceId,
};
use donder_model::{DonderProject, ProjectEdit};
use donder_model::{GraphOperatorNode, OperatorDefinition, OperatorDefinitionId};

pub fn append_operator(project: &mut DonderProject, id: &SequenceId, compiled: CompiledDefinition) {
    let definition = OperatorDefinitionId(project.new_id());
    let input = compiled.inputs()[0].name.as_str().to_owned();
    let mut sequence = project.get::<donder_model::Sequence>(*id).unwrap().clone();
    let graph = &mut sequence.composition_graph;
    let output = graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Root))
        .unwrap()
        .id
        .clone();
    let next = graph.nodes.iter().map(|node| node.id.0).max().unwrap() + 1;
    let count = graph.edges.iter().filter(|edge| edge.to == output).count();
    assert!(count > 0);
    let declarations = (0..count)
        .map(|index| format!("input s{index};"))
        .collect::<String>();
    let expression = (1..count).fold("s0".to_owned(), |value, index| {
        format!("max({value}, s{index})")
    });
    let mixer = compile_script(&format!(
        "operator TestMix {{ {declarations} sample {{ {expression} }} }}"
    ))
    .unwrap()
    .remove(0);
    let mixer_id = OperatorDefinitionId(project.new_id());
    // Preserve the existing output mix as the new operator's sole input.
    let mix = CompositionGraphNodeId(next);
    let operator = CompositionGraphNodeId(next + 1);
    for node in &mut graph.nodes {
        if node.id == output {
            node.id = mix.clone();
            node.kind = CompositionGraphNodeKind::Operator(GraphOperatorNode {
                name: donder_language::object_name("mix"),
                operator: mixer_id,
                params: Default::default(),
            });
        }
    }
    for (index, edge) in graph
        .edges
        .iter_mut()
        .filter(|edge| edge.to == output)
        .enumerate()
    {
        edge.to = mix.clone();
        edge.to_port = GraphPortId(format!("s{index}"));
    }
    graph.nodes.extend([
        CompositionGraphNode {
            id: operator.clone(),
            position: GraphNodePosition { x: 0.0, y: 0.0 },
            kind: CompositionGraphNodeKind::Operator(GraphOperatorNode {
                name: donder_language::object_name("operator"),
                operator: definition,
                params: Default::default(),
            }),
        },
        CompositionGraphNode {
            id: output.clone(),
            position: GraphNodePosition { x: 0.0, y: 0.0 },
            kind: CompositionGraphNodeKind::Root,
        },
    ]);
    graph.edges.extend([
        EffectGraphEdge {
            from: mix,
            from_port: GraphPortId("output".into()),
            to: operator.clone(),
            to_port: GraphPortId(input),
        },
        EffectGraphEdge {
            from: operator,
            from_port: GraphPortId("output".into()),
            to: output,
            to_port: GraphPortId("input".into()),
        },
    ]);
    project
        .apply_edits([
            ProjectEdit::Put(donder_model::Object::OperatorDefinition(
                OperatorDefinition::new(mixer_id, mixer),
            )),
            ProjectEdit::Put(donder_model::Object::OperatorDefinition(
                OperatorDefinition::new(definition, compiled),
            )),
            ProjectEdit::Put(sequence.into()),
        ])
        .unwrap();
}

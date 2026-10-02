use donder_language::dsl::{CompiledOperator, compile_operators};
use donder_language::identity::SourceIdentity;
use donder_language::model::{DonderProject, ProjectEdit};
use donder_language::operator::{
    GraphOperatorNode, OperatorDefinitionId, OperatorRef, custom_operator_definition,
};
use donder_language::sequence::{
    CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
    GraphNodePosition, GraphPortId, SequenceId,
};

pub fn append_operator(project: &mut DonderProject, id: &SequenceId, compiled: CompiledOperator) {
    let definition = OperatorDefinitionId(SourceIdentity::from_document(
        id.0.root_source().document_id().clone(),
        compiled.name().as_str().into(),
    ));
    let input = compiled.inputs()[0].name.as_str().to_owned();
    let mut sequence = project.sequence(id).unwrap().clone();
    let graph = &mut sequence.composition_graph;
    let output = graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Output))
        .unwrap()
        .id
        .clone();
    let next = graph.nodes.iter().map(|node| node.id.0).max().unwrap() + 1;
    let count = graph.edges.iter().filter(|edge| edge.to == output).count();
    assert!(count > 0);
    let declarations = (0..count)
        .map(|index| format!("input Signal s{index};"))
        .collect::<String>();
    let expression = (1..count).fold("s0.at(seconds())".to_owned(), |value, index| {
        format!("max({value}, s{index}.at(seconds()))")
    });
    let mixer = compile_operators(&format!(
        "operator TestMix {{ {declarations} color sample() {{ return {expression}; }} }}"
    ))
    .unwrap()
    .remove(0);
    let mixer_id = OperatorDefinitionId(SourceIdentity::from_document(
        id.0.root_source().document_id().clone(),
        "TestMix".into(),
    ));
    // Preserve the existing output mix as the new operator's sole input.
    let mix = CompositionGraphNodeId(next);
    let operator = CompositionGraphNodeId(next + 1);
    for node in &mut graph.nodes {
        if node.id == output {
            node.id = mix.clone();
            node.kind = CompositionGraphNodeKind::Operator(GraphOperatorNode {
                operator: OperatorRef::Custom(mixer_id.clone()),
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
                operator: OperatorRef::Custom(definition.clone()),
                params: Default::default(),
            }),
        },
        CompositionGraphNode {
            id: output.clone(),
            position: GraphNodePosition { x: 0.0, y: 0.0 },
            kind: CompositionGraphNodeKind::Output,
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
            ProjectEdit::SetOperatorDefinition {
                id: mixer_id.clone(),
                value: custom_operator_definition(mixer_id, mixer),
            },
            ProjectEdit::SetOperatorDefinition {
                id: definition.clone(),
                value: custom_operator_definition(definition, compiled),
            },
            ProjectEdit::ReplaceSequence {
                id: id.clone(),
                value: sequence,
            },
        ])
        .unwrap();
}

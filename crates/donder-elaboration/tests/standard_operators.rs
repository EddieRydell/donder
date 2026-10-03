use donder_language::values::SampleTime;

#[test]
fn fusion_preserves_shared_sources_and_query_clocks() {
    use donder_language::dsl::compile_operators;
    use donder_language::identity::SourceIdentity;
    use donder_language::model::ProjectEdit;
    use donder_language::operator::{
        GraphOperatorNode, OperatorDefinitionId, OperatorRef, custom_operator_definition,
    };
    use donder_language::sequence::{
        CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
        GraphNodePosition, GraphPortId, SequenceCompositionGraph,
    };

    let root = camino::Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut project = donder_project_io::load_project(&root).unwrap().project;
    let id = project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
        .find(|id| id.0.root_source().object() == "layer_test")
        .unwrap()
        .clone();
    let definitions = compile_operators(
        "operator Clock { input Signal source;
        color sample() { return max(source.at(seconds()), rgb(progress(), 0.05, 0.1)); }
    }
    operator Early { input Signal source;
        color sample() { return source.at(seconds() - 0.125) * 0.4; }
    }
    operator Late { input Signal source;
        color sample() { return source.at(seconds() + 0.25) * 0.8; }
    }",
    )
    .unwrap();
    let mut refs = Vec::new();
    for compiled in definitions {
        let definition = OperatorDefinitionId(SourceIdentity::from_document(
            id.0.root_source().document_id().clone(),
            compiled.name().as_str().into(),
        ));
        refs.push(OperatorRef::Custom(definition.clone()));
        project
            .apply_edits([ProjectEdit::SetOperatorDefinition {
                id: definition.clone(),
                value: custom_operator_definition(definition, compiled),
            }])
            .unwrap();
    }
    let mut sequence = project.sequence(&id).unwrap().clone();
    sequence.automation_clips.clear();
    let node = |id, kind| CompositionGraphNode {
        id: CompositionGraphNodeId(id),
        position: GraphNodePosition { x: 0.0, y: 0.0 },
        kind,
    };
    let operator = |index: usize| {
        CompositionGraphNodeKind::Operator(GraphOperatorNode {
            operator: refs[index].clone(),
            params: Default::default(),
        })
    };
    let edge = |from, to, port: &str| EffectGraphEdge {
        from: CompositionGraphNodeId(from),
        from_port: GraphPortId("output".into()),
        to: CompositionGraphNodeId(to),
        to_port: GraphPortId(port.into()),
    };
    sequence.composition_graph = SequenceCompositionGraph {
        nodes: vec![
            node(
                0,
                CompositionGraphNodeKind::Layer {
                    layer_id: sequence.layers[0].id.clone(),
                },
            ),
            node(1, operator(0)),
            node(2, operator(1)),
            node(3, operator(2)),
            node(4, CompositionGraphNodeKind::Output),
        ],
        edges: vec![
            edge(0, 1, "source"),
            edge(1, 2, "source"),
            edge(1, 3, "source"),
            edge(2, 4, "input"),
            edge(3, 4, "input"),
        ],
    };
    project.replace_sequence(&id, sequence.clone()).unwrap();
    let shared =
        donder_elaboration::prepare(&project, &id, donder_elaboration::PrepareOutputs::All)
            .unwrap();
    // The shared clock stays a graph node. Duplicating it gives each consumer a
    // private source that can be fused, while preserving the authored behavior.
    sequence.composition_graph.nodes.push(node(5, operator(0)));
    sequence
        .composition_graph
        .edges
        .retain(|edge| !(edge.from.0 == 1 && edge.to.0 == 3));
    sequence
        .composition_graph
        .edges
        .extend([edge(0, 5, "source"), edge(5, 3, "source")]);
    project.replace_sequence(&id, sequence).unwrap();
    let duplicated =
        donder_elaboration::prepare(&project, &id, donder_elaboration::PrepareOutputs::All)
            .unwrap();
    let mut shared = shared.into_playback();
    let mut duplicated = duplicated.into_playback();
    for ticks in [
        0, 124_999, 125_000, 125_001, 1_000_000, 3_000_000, 0, 999_999,
    ] {
        let time = SampleTime::from_ticks(ticks);
        assert_eq!(
            shared.evaluate(time).colors(),
            duplicated.evaluate(time).colors()
        );
    }
}

#[test]
fn disconnected_operator_branches_are_preserved_but_not_prepared() {
    use donder_language::operator::{GraphOperatorNode, validate_composition_graph};
    use donder_language::sequence::{
        CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
        GraphNodePosition, GraphPortId,
    };
    let root = camino::Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut project = donder_project_io::load_project(&root).unwrap().project;
    let id = project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
        .find(|id| id.0.root_source().object() == "layer_test")
        .unwrap()
        .clone();
    let baseline =
        donder_elaboration::prepare(&project, &id, donder_elaboration::PrepareOutputs::All)
            .unwrap();
    let definitions = &project.definitions().operators;
    let make_node = |id, name| CompositionGraphNode {
        id: CompositionGraphNodeId(id),
        position: GraphNodePosition { x: 100.0, y: 100.0 },
        kind: CompositionGraphNodeKind::Operator(GraphOperatorNode {
            operator: definitions
                .definitions
                .values()
                .find(|d| d.declaration_name == name)
                .unwrap()
                .id()
                .clone(),
            params: Default::default(),
        }),
    };
    let edge = |from, to, port: &str| EffectGraphEdge {
        from: CompositionGraphNodeId(from),
        from_port: GraphPortId("output".into()),
        to: CompositionGraphNodeId(to),
        to_port: GraphPortId(port.into()),
    };
    let mut sequence = project.sequence(&id).unwrap().clone();
    let graph = &mut sequence.composition_graph;
    graph
        .nodes
        .extend([make_node(8000, "Add"), make_node(8001, "HueShift")]);
    graph.edges.push(edge(8000, 8001, "source"));
    let output = graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Output))
        .unwrap()
        .id
        .0;
    validate_composition_graph(graph, definitions).unwrap();
    let mut invalid = graph.clone();
    invalid.edges.push(edge(8001, 8000, "a"));
    assert!(
        validate_composition_graph(&invalid, definitions).is_err(),
        "disconnected cycle accepted"
    );
    let mut invalid = graph.clone();
    invalid.edges.push(edge(8000, 8001, "unknown"));
    assert!(
        validate_composition_graph(&invalid, definitions).is_err(),
        "unknown port accepted"
    );
    project.replace_sequence(&id, sequence.clone()).unwrap();
    let disconnected =
        donder_elaboration::prepare(&project, &id, donder_elaboration::PrepareOutputs::All)
            .unwrap();
    assert_eq!(
        donder_runtime::encode_sequence(&baseline).unwrap(),
        donder_runtime::encode_sequence(&disconnected).unwrap(),
    );
    let mut before = baseline.into_playback();
    let mut after = disconnected.into_playback();
    for ticks in [0, 1_000_000, 3_000_000] {
        let expected = before.evaluate(SampleTime::from_ticks(ticks)).colors();
        let actual = after.evaluate(SampleTime::from_ticks(ticks)).colors();
        assert_eq!(actual, expected);
    }
    sequence
        .composition_graph
        .edges
        .push(edge(8001, output, "input"));
    assert!(
        project.replace_sequence(&id, sequence).is_err(),
        "incomplete branch contributing to output accepted"
    );
}

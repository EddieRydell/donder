use donder_language::values::SampleTime;

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

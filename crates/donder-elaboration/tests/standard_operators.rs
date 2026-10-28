use donder_elaboration::{PreparedShow, Retain, prepare};
use donder_model::DonderProject;
use donder_model::GraphOperatorNode;
use donder_model::{
    CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
    GraphNodePosition, GraphPortId, Sequence, SequenceCompositionGraph, SequenceId,
};
use donder_runtime_types::SampleTime;

/// The starter's `layer_test`: layer 0 lights the output directly and layer 1
/// feeds an operator.
fn layer_test() -> (DonderProject, SequenceId) {
    let root = camino::Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let session =
        donder_project_io::check_project(&root, &Default::default(), &mut Default::default())
            .into_session()
            .unwrap();
    let id = session
        .project
        .root()
        .sequences
        .iter()
        .map(|source| *source.id())
        .find(|id| {
            session
                .source
                .declared_name((*id).into())
                .is_some_and(|name| name.as_str() == "layer_test")
        })
        .unwrap();
    (session.project, id)
}

fn node(id: u32, kind: CompositionGraphNodeKind) -> CompositionGraphNode {
    CompositionGraphNode {
        id: CompositionGraphNodeId(id),
        position: GraphNodePosition { x: 0.0, y: 0.0 },
        kind,
    }
}

fn edge(from: u32, to: u32, port: &str) -> EffectGraphEdge {
    EffectGraphEdge {
        from: CompositionGraphNodeId(from),
        from_port: GraphPortId("output".into()),
        to: CompositionGraphNodeId(to),
        to_port: GraphPortId(port.into()),
    }
}

/// Node `id` for one of the project's operators, with default parameters.
fn project_operator(project: &DonderProject, id: u32, name: &str) -> CompositionGraphNodeKind {
    CompositionGraphNodeKind::Operator(GraphOperatorNode {
        name: donder_language::object_name(&format!("operator_{id}")),
        operator: *project
            .reusable::<donder_model::OperatorDefinition>()
            .values()
            .find(|definition| definition.name() == name)
            .unwrap()
            .id(),
        params: Default::default(),
    })
}

#[test]
fn fusion_preserves_shared_sources_and_addressed_reads() {
    use donder_language::compiler::compile_script;
    use donder_model::ProjectEdit;
    use donder_model::{OperatorDefinition, OperatorDefinitionId};

    let (mut project, id) = layer_test();
    let definitions = compile_script(
        "operator Clock { input source;
        sample { max(source, rgb(progress, 0.05, 0.1)) }
    }
    operator Early { input source;
        sample { source.at(pixel.index - 1) * 0.4 }
    }
    operator Late { input source;
        sample { source.at(pixel.index + 2) * 0.8 }
    }",
    )
    .unwrap();
    let mut refs = Vec::new();
    for compiled in definitions {
        let definition = OperatorDefinitionId(project.new_id());
        refs.push(definition);
        project
            .apply_edits([ProjectEdit::Put(donder_model::Object::OperatorDefinition(
                OperatorDefinition::new(definition, compiled),
            ))])
            .unwrap();
    }
    let mut sequence = project.get::<donder_model::Sequence>(id).unwrap().clone();
    sequence.automation_clips.clear();
    // Node `id` runs operator `index`.
    let operator = |id: u32, index: usize| {
        CompositionGraphNodeKind::Operator(GraphOperatorNode {
            name: donder_language::object_name(&format!("operator_{id}")),
            operator: refs[index],
            params: Default::default(),
        })
    };
    sequence.composition_graph = SequenceCompositionGraph {
        nodes: vec![
            node(
                0,
                CompositionGraphNodeKind::Layer {
                    layer_id: sequence.layers[0].id.clone(),
                },
            ),
            node(1, operator(1, 0)),
            node(2, operator(2, 1)),
            node(3, operator(3, 2)),
            node(4, CompositionGraphNodeKind::Root),
        ],
        edges: vec![
            edge(0, 1, "source"),
            edge(1, 2, "source"),
            edge(1, 3, "source"),
            edge(2, 4, "input"),
            edge(3, 4, "input"),
        ],
    };
    project.put(sequence.clone()).unwrap();
    let shared = prepare(&project, &id, Retain::All).unwrap();
    // The shared source stays a graph node. Duplicating it gives each consumer
    // a private source, which stays a boundary for the consumers' addressed
    // reads, while preserving the authored behavior.
    sequence
        .composition_graph
        .nodes
        .push(node(5, operator(5, 0)));
    sequence
        .composition_graph
        .edges
        .retain(|edge| !(edge.from.0 == 1 && edge.to.0 == 3));
    sequence
        .composition_graph
        .edges
        .extend([edge(0, 5, "source"), edge(5, 3, "source")]);
    project.put(sequence).unwrap();
    let duplicated = prepare(&project, &id, Retain::All).unwrap();
    let mut shared = shared.into_playback();
    let mut duplicated = duplicated.into_playback();
    for ticks in [
        0, 124_999, 125_000, 125_001, 1_000_000, 3_000_000, 0, 999_999,
    ] {
        let time = SampleTime::from_ticks(ticks);
        donder_test_support::playback::assert_frames_close(
            shared.evaluate(time).colors(),
            duplicated.evaluate(time).colors(),
            &format!("ticks={ticks}"),
        );
    }
}

/// `layer_test` with layer 1 made black by `blacken`, feeding the output
/// through a chain of `(operator, input port)`, beside the lit layer 0.
fn black_layer_through(blacken: fn(&mut Sequence), operators: &[(&str, &str)]) -> PreparedShow {
    let (mut project, id) = layer_test();
    let mut sequence = project.get::<donder_model::Sequence>(id).unwrap().clone();
    sequence.automation_clips.clear();
    blacken(&mut sequence);
    let layer = |index: usize| CompositionGraphNodeKind::Layer {
        layer_id: sequence.layers[index].id.clone(),
    };
    let mut nodes = vec![
        node(0, layer(0)),
        node(1, layer(1)),
        node(2, CompositionGraphNodeKind::Root),
    ];
    let mut edges = vec![edge(0, 2, "input")];
    let mut previous = 1;
    for (id, (name, port)) in (10..).zip(operators) {
        nodes.push(node(id, project_operator(&project, id, name)));
        edges.push(edge(previous, id, port));
        previous = id;
    }
    edges.push(edge(previous, 2, "input"));
    sequence.composition_graph = SequenceCompositionGraph { nodes, edges };
    project.put(sequence).unwrap();
    prepare(&project, &id, Retain::All).unwrap()
}

/// The archive bytes, which record every prepared signal and program.
fn encoded(sequence: &PreparedShow) -> Vec<u8> {
    sequence.encode()
}

fn disable_layer(sequence: &mut Sequence) {
    sequence.layers[1].enabled = false;
}

fn empty_layer(sequence: &mut Sequence) {
    let layer = sequence.layers[1].id.clone();
    sequence.clips.retain(|clip| clip.layer_id != layer);
}

#[test]
fn operators_that_keep_black_inputs_black_are_not_prepared() {
    for blacken in [disable_layer, empty_layer] {
        let direct = encoded(&black_layer_through(blacken, &[]));
        // Scaling, recoloring, trailing and reducing operators map black to
        // black, so each folds away, including along a chain.
        for operators in [
            &[("Dim", "input")][..],
            &[("Afterglow", "input")][..],
            &[("Afterglow", "input"), ("Dim", "input")][..],
            &[("Blur", "input"), ("Gain", "source")][..],
            &[("HueShift", "source")][..],
            &[("Colorize", "input")][..],
        ] {
            assert!(
                encoded(&black_layer_through(blacken, operators)) == direct,
                "{operators:?} over a black layer was prepared"
            );
        }
    }
}

#[test]
fn operators_that_light_black_inputs_are_prepared() {
    for blacken in [disable_layer, empty_layer] {
        let direct = encoded(&black_layer_through(blacken, &[]));
        let inverted = black_layer_through(blacken, &[("Dim", "input"), ("Invert", "input")]);
        assert!(
            encoded(&inverted) != direct,
            "inverted black was folded away"
        );
        // Inverted black is white, whatever layer 0 renders.
        let mut playback = inverted.into_playback();
        for ticks in [0, 4_500_000, 59_000_000] {
            let frame = playback.evaluate(SampleTime::from_ticks(ticks));
            assert!(
                frame
                    .outputs()
                    .all(|output| output.bytes.iter().all(|&value| value == u8::MAX)),
                "inverted black is not white at {ticks}"
            );
        }
    }
}

#[test]
fn disconnected_operator_branches_are_preserved_but_not_prepared() {
    use donder_model::validate_composition_graph;
    let (mut project, id) = layer_test();
    let baseline = prepare(&project, &id, Retain::All).unwrap();
    let definitions = project.reusable::<donder_model::OperatorDefinition>();
    let make_node = |id, name| CompositionGraphNode {
        position: GraphNodePosition { x: 100.0, y: 100.0 },
        ..node(id, project_operator(&project, id, name))
    };
    let mut sequence = project.get::<donder_model::Sequence>(id).unwrap().clone();
    let graph = &mut sequence.composition_graph;
    graph
        .nodes
        .extend([make_node(8000, "Add"), make_node(8001, "HueShift")]);
    graph.edges.push(edge(8000, 8001, "source"));
    let output = graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Root))
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
    project.put(sequence.clone()).unwrap();
    let disconnected = prepare(&project, &id, Retain::All).unwrap();
    assert!(encoded(&baseline) == encoded(&disconnected));
    sequence
        .composition_graph
        .edges
        .push(edge(8001, output, "input"));
    assert!(
        project.put(sequence).is_err(),
        "incomplete branch contributing to output accepted"
    );
}

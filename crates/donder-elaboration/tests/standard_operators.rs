const SPATIAL: donder_runtime::SpatialContext = donder_runtime::SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

use donder_language::dsl::compile_operators;
use donder_runtime::{Color, SampleDuration, SampleTime};
use donder_runtime::{
    CompiledOperator, Identifier, OperatorRunContext, RuntimeError, SignalPixel, SignalSampler,
    Value, VmWorkspace,
};

fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color { red, green, blue }
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

struct Inputs<F> {
    sample: F,
    times: Vec<u32>,
}

impl<F: Fn(usize, u32) -> Color> SignalSampler for Inputs<F> {
    fn sample_signal(
        &mut self,
        input: usize,
        time: SampleTime,
        _: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Result<Color, RuntimeError> {
        self.times.push(time.as_ticks());
        Ok((self.sample)(input, time.as_ticks()))
    }
}

fn library() -> Vec<CompiledOperator> {
    compile_operators(include_str!(
        "../../../examples/starter/operators/standard.operator.donder"
    ))
    .unwrap()
}

fn sample(
    operators: &[CompiledOperator],
    name: &str,
    time: u32,
    overrides: &[(&str, Value)],
    source: impl Fn(usize, u32) -> Color,
) -> (Color, Vec<u32>) {
    let operator = operators
        .iter()
        .find(|operator| operator.name().as_str() == name)
        .unwrap();
    let overrides = overrides
        .iter()
        .map(|(name, value)| (Identifier::new((*name).into()).unwrap(), value.clone()))
        .collect::<Vec<_>>();
    let invocation = operator
        .bind(
            overrides.iter().map(|(name, value)| (name, value)),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    let mut inputs = Inputs {
        sample: source,
        times: Vec::new(),
    };
    let color = invocation
        .evaluate(
            &OperatorRunContext {
                progress: time as f32 / 10_000_000.0,
                time: SampleDuration::from_ticks(time),
                duration: SampleDuration::from_ticks(10_000_000),
                pixel_index: 0,
                pixel_count: 1,
                pixel_fraction: 0.0,
            },
            &SPATIAL,
            &mut inputs,
            &mut VmWorkspace::default(),
        )
        .unwrap();
    (color, inputs.times)
}

#[test]
fn standard_color_operators_match_expected_channels() {
    let operators = library();
    let inputs = |index, _| [rgb(200, 100, 50), rgb(128, 255, 0)][index];
    for (name, params, expected) in [
        ("Max", vec![], rgb(200, 255, 50)),
        ("Add", vec![], rgb(255, 255, 50)),
        ("Multiply", vec![], rgb(100, 100, 0)),
        ("IntensityModulate", vec![], rgb(200, 100, 50)),
        ("Dim", vec![("amount", Value::Float(0.5))], rgb(100, 50, 25)),
        ("Dim", vec![("amount", Value::Float(-1.0))], Color::BLACK),
        (
            "Dim",
            vec![("amount", Value::Float(2.0))],
            rgb(200, 100, 50),
        ),
        ("Invert", vec![], rgb(55, 155, 205)),
        (
            "Colorize",
            vec![("tint", Value::Color(rgb(255, 128, 0)))],
            rgb(200, 100, 0),
        ),
    ] {
        assert_eq!(
            sample(&operators, name, 0, &params, inputs).0,
            expected,
            "{name}"
        );
    }
}

#[test]
fn delay_rejects_before_zero_and_clamps_negative_offsets() {
    let operators = library();
    let color = rgb(123, 45, 67);
    for (time, delay, expected_time) in [
        (0, 0.25, None),
        (249_999, 0.25, None),
        (250_000, 0.25, Some(0)),
        (500_000, 0.25, Some(250_000)),
        (0, 0.0, Some(0)),
        (500_000, 0.0, Some(500_000)),
        (500_000, -1.0, Some(500_000)),
    ] {
        let (actual, times) = sample(
            &operators,
            "Delay",
            time,
            &[("seconds", Value::Float(delay))],
            |_, _| color,
        );
        assert_eq!(actual, expected_time.map_or(Color::BLACK, |_| color));
        assert_eq!(times, expected_time.into_iter().collect::<Vec<_>>());
    }
}

#[test]
fn echo_boundaries_repeat_limits_and_decay_match_expected_output() {
    let operators = library();
    for (time, repeats, decay, expected, expected_times) in [
        (0, 3, 0.5, rgb(200, 100, 50), vec![0]),
        (249_999, 3, 0.5, Color::BLACK, vec![249_999]),
        (250_000, 3, 0.5, rgb(100, 50, 25), vec![250_000, 0]),
        (500_000, 3, 0.5, rgb(50, 25, 13), vec![500_000, 250_000, 0]),
        (250_000, -10, 0.5, rgb(100, 50, 25), vec![250_000, 0]),
        (500_000, 0, 0.5, Color::BLACK, vec![500_000, 250_000]),
        (250_000, 3, -1.0, Color::BLACK, vec![250_000, 0]),
        (250_000, 3, 2.0, rgb(200, 100, 50), vec![250_000, 0]),
    ] {
        let params = [
            ("seconds", Value::Float(0.25)),
            ("repeats", Value::Int(repeats)),
            ("decay", Value::Float(decay)),
        ];
        let (actual, times) = sample(&operators, "Echo", time, &params, |_, time| {
            if time == 0 {
                rgb(200, 100, 50)
            } else {
                Color::BLACK
            }
        });
        assert_eq!(
            actual, expected,
            "time={time} repeats={repeats} decay={decay}"
        );
        assert_eq!(times, expected_times);
    }
    for repeats in [32, 33, 100] {
        let params = [
            ("seconds", Value::Float(0.25)),
            ("repeats", Value::Int(repeats)),
            ("decay", Value::Float(1.0)),
        ];
        let (color, times) = sample(&operators, "Echo", 8_000_000, &params, |_, time| {
            if time == 0 {
                rgb(200, 100, 50)
            } else {
                Color::BLACK
            }
        });
        assert_eq!(color, rgb(200, 100, 50));
        assert_eq!(
            times,
            (0..=32)
                .rev()
                .map(|index| index * 250_000)
                .collect::<Vec<_>>()
        );
    }
    for delay in [0.0, -1.0] {
        let params = [
            ("seconds", Value::Float(delay)),
            ("repeats", Value::Int(3)),
            ("decay", Value::Float(0.5)),
        ];
        let (color, times) = sample(&operators, "Echo", 500_000, &params, |_, _| {
            rgb(200, 100, 50)
        });
        assert_eq!(color, rgb(200, 100, 50));
        assert_eq!(times, vec![500_000; 4]);
    }
}

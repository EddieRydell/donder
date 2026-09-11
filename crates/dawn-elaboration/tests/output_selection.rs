use camino::Utf8PathBuf;
use dawn_elaboration::{PreparedSequenceOutput, SequenceOutputPrepareError};
use dawn_language::controller::{ControllerId, ControllerPortId};
use dawn_language::layout::FixtureInstanceId;
use dawn_language::model::DawnProject;
use dawn_language::patch::PixelSpan;
use dawn_language::sequence::SequenceId;
use dawn_project_io::load_package;
use dawn_runtime::sequence::PreparedSequence;
use dawn_runtime::values::{Color, SampleTime, sample_time_from_frame};

fn starter() -> DawnProject {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    load_package(&root).unwrap().session.project
}

#[test]
fn controller_fragments_retain_nested_generator_parameter_dependencies() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut sources = dawn_project_io::project_source_texts(&root).unwrap();
    sources.insert(
        "effects/mark-impact-burst.effect.dawn".into(),
        r#"
        effect MarkImpactBurst {
            void generate() {
                timeline.emit Inner { start: 0.0, duration: duration(), target: target, value: progress() };
            }
        }
        effect Inner {
            param float value;
            void generate() {
                timeline.emit Leaf { start: 0.0, duration: duration(), target: target, value: value * 0.5 + progress() * 0.5 };
            }
        }
        effect Leaf {
            param float value;
            color sample() { return rgb(value, 0.0, 0.0); }
        }
        "#.into(),
    );
    let report = dawn_project_io::check_package_with_overrides(&root, &sources);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let mut project = report.session.unwrap().project;
    let generator = project
        .definitions
        .effects
        .definitions
        .keys()
        .find(|id| id.0.object() == "MarkImpactBurst")
        .unwrap()
        .clone();
    for sequence in project.sequences.values_mut() {
        sequence.automation_clips.clear();
        for effect in &mut sequence.effects {
            effect.definition = dawn_language::effect::EffectRef::Custom(generator.clone());
            effect.param_overrides.clear();
        }
    }
    let ports = ports(&project);
    let mut retained = false;
    for id in project.sequences.keys() {
        let fragment = compare(&project, id, &ports[..1]);
        retained |= !fragment.signals.parameter_environments.is_empty();
        let bytes = dawn_runtime::wire::encode_sequence(&fragment).unwrap();
        dawn_runtime::wire::decode_sequence(&bytes, dawn_runtime::wire::LoadLimits::default())
            .unwrap();
    }
    assert!(retained);
}

fn ports(project: &DawnProject) -> Vec<(ControllerId, ControllerPortId)> {
    project.setups[&project.root.setup]
        .controllers
        .iter()
        .flat_map(|id| {
            project.controllers[id]
                .ports
                .iter()
                .map(|port| (id.clone(), port.id))
        })
        .collect()
}

fn compare(
    project: &DawnProject,
    id: &SequenceId,
    selected: &[(ControllerId, ControllerPortId)],
) -> PreparedSequence {
    let full = PreparedSequenceOutput::prepare(project, &project.root.setup, id).unwrap();
    let fragment =
        PreparedSequenceOutput::prepare_selected(project, &project.root.setup, id, selected)
            .unwrap();
    let mut full_workspace = full.workspace();
    let mut workspace = fragment.workspace();
    let mut times = [9504, 8450, 0, 8494, 8398, 7150, 7151, 2000, 15000]
        .map(|frame| sample_time_from_frame(frame, full.frame_rate()).unwrap())
        .to_vec();
    times.extend(full.sequence.signals.effects.iter().flat_map(|effect| {
        [
            Some(effect.start_time),
            effect.start_time.checked_add_duration(effect.duration),
        ]
        .into_iter()
        .flatten()
    }));
    times.extend([
        SampleTime::from_ticks(full.sequence.signals.duration.as_ticks()),
        SampleTime::from_ticks(0),
    ]);
    for time in times {
        let expected = full.sample_into(time, &mut full_workspace).unwrap();
        let actual = fragment.sample_into(time, &mut workspace).unwrap();
        assert_eq!(actual.len(), selected.len());
        for (frame, (controller, port)) in actual.iter().zip(selected) {
            assert_eq!((&frame.controller, frame.port), (controller, *port));
            let expected = expected
                .iter()
                .find(|frame| frame.controller == *controller && frame.port == *port)
                .unwrap();
            assert_eq!(
                frame,
                expected,
                "{} at {time:?}, port {port:?}",
                id.0.object()
            );
        }
    }
    fragment.sequence
}

#[test]
fn every_starter_port_matches_the_full_sequence_across_seeks() {
    let project = starter();
    let ports = ports(&project);
    for id in &project.root.sequences {
        let full = PreparedSequenceOutput::prepare(&project, &project.root.setup, id).unwrap();
        for port in &ports {
            let fragment = compare(&project, id, std::slice::from_ref(port));
            assert_eq!(fragment.signals.fixtures.len(), 1);
            assert_eq!(fragment.signals.pixel_count, 113);
            assert!(fragment.signals.effects.len() <= full.sequence.signals.effects.len());
            assert!(fragment.signals.programs.len() <= full.sequence.signals.programs.len());
            assert!(
                fragment.signals.target_pixels.len() < full.sequence.signals.target_pixels.len()
            );
        }
        let fragment = compare(&project, id, &ports[0..1]);
        let frame_bytes = |sequence: &PreparedSequence| {
            sequence.signals.pixel_count
                * usize::from(sequence.signals.plan.frame_buffer_count)
                * size_of::<Color>()
        };
        println!(
            "{}: pixels {} -> {}; target records {} -> {}; effects {} -> {}; programs {} -> {}; pixel routes {} -> {}; graph buffer bytes {} -> {}",
            id.0.object(),
            full.sequence.signals.pixel_count,
            fragment.signals.pixel_count,
            full.sequence.signals.target_pixels.len(),
            fragment.signals.target_pixels.len(),
            full.sequence.signals.effects.len(),
            fragment.signals.effects.len(),
            full.sequence.signals.programs.len(),
            fragment.signals.programs.len(),
            full.sequence.patch.routes.len(),
            fragment.patch.routes.len(),
            frame_bytes(&full.sequence),
            frame_bytes(&fragment)
        );
        let reversed = ports.iter().rev().cloned().collect::<Vec<_>>();
        compare(&project, id, &reversed);
    }
}

#[test]
fn split_fixture_keeps_original_context_and_compacts_disjoint_pixels() {
    let mut project = starter();
    let patch_id = project.setups[&project.root.setup].patch.clone();
    let patch = project.patches.get_mut(&patch_id).unwrap();
    // Two ports wire disjoint spans of one fixture, preserving authored effect coordinates.
    for (index, start) in [(0, 0), (1, 76)] {
        let route = &mut patch.routes[index];
        route.target.fixture = FixtureInstanceId(1);
        route.pixels = Some(PixelSpan { start, count: 37 });
        route.start_slot = 7;
    }
    let ports = ports(&project);
    for id in &project.root.sequences {
        let fragment = compare(&project, id, &[ports[1].clone(), ports[0].clone()]);
        assert_eq!(fragment.signals.fixtures.len(), 1);
        assert_eq!(fragment.signals.pixel_count, 74);
        let target = fragment.signals.target(fragment.signals.plan.target);
        assert_eq!(target[37].fixture_pixel_index, 37);
        assert_eq!(target[37].pixel_index, 76);
        assert_eq!(target[37].pixel_count, 113);
        compare(&project, id, &ports[1..2]);
    }
    // Whole-target effects use different context from per-fixture effects.
    for sequence in project.sequences.values_mut() {
        for effect in &mut sequence.effects {
            effect.scope = dawn_language::effect::EffectScope::WholeTarget;
        }
        for collection in &mut sequence.mark_collections {
            collection.marks = [58_000_000, 59_000_000, 60_000_000]
                .map(dawn_language::values::DawnTime::from_micros)
                .to_vec();
        }
    }
    for id in &project.root.sequences {
        compare(&project, id, &[ports[1].clone(), ports[0].clone()]);
    }
    // Spatial reads must not see the compacted 37-pixel output domain. Local
    // reads need the whole original fixture; global reads can reach unpatched
    // fixtures. This also exercises nested temporal/spatial operator sampling.
    for (query, expected_pixels) in [
        (
            "source.at(seconds() + offset_seconds, pixel_count() - 1 - pixel_index())",
            113,
        ),
        (
            "source.at_global(seconds() + offset_seconds, 226 + pixel_index())",
            3390,
        ),
    ] {
        let compiled = dawn_language::dsl::compile_operators(&format!(
            "operator TimeWarp {{ input Signal source; param float offset_seconds = 0.0; color sample() {{ return {query}; }} }}"
        )).unwrap().remove(0);
        let definition = project
            .definitions
            .operators
            .definitions
            .values_mut()
            .find(|definition| definition.declaration_name == "TimeWarp")
            .unwrap();
        definition.implementation =
            dawn_language::operator::OperatorImplementation::Dsl(Box::new(compiled));
        let id = project
            .root
            .sequences
            .iter()
            .find(|id| id.0.object() == "layer_test")
            .unwrap();
        let fragment = compare(&project, id, &ports[1..2]);
        assert_eq!(fragment.signals.pixel_count, expected_pixels, "{query}");
        // Exercise the serialized representation as well as live preparation.
        let encoded = dawn_runtime::wire::encode_sequence(&fragment).unwrap();
        let decoded = dawn_runtime::wire::decode_sequence(
            &encoded,
            dawn_runtime::wire::LoadLimits {
                workspace_bytes: 4 * 1024 * 1024,
                ..Default::default()
            },
        )
        .unwrap();
        let mut original_workspace = fragment.workspace();
        let mut decoded_workspace = decoded.workspace();
        let mut original = vec![vec![0; fragment.output_widths[0] as usize]];
        let mut restored = original.clone();
        let time = SampleTime::from_ticks(59_000_000);
        fragment
            .evaluate(time, &mut original, &mut original_workspace)
            .unwrap();
        decoded
            .evaluate(time, &mut restored, &mut decoded_workspace)
            .unwrap();
        assert_eq!(restored, original);
    }
}

#[test]
fn shared_pixels_and_multiple_controllers_keep_output_order() {
    use dawn_language::identity::SourceIdentity;
    let mut project = starter();
    let selected = ports(&project);
    let original_id = selected[0].0.clone();
    let other_id = ControllerId(SourceIdentity::from_document(
        original_id.0.document_id().clone(),
        "other_controller".into(),
    ));
    let other = project.controllers[&original_id].clone();
    project.controllers.insert(other_id.clone(), other);
    let setup = project.setups.get_mut(&project.root.setup).unwrap();
    setup.controllers.push(other_id.clone());
    let patch = project.patches.get_mut(&setup.patch).unwrap();
    patch.routes[1].controller = other_id.clone();
    patch.routes[1].target = patch.routes[0].target.clone();
    for id in &project.root.sequences {
        let fragment = compare(
            &project,
            id,
            &[(other_id.clone(), selected[1].1), selected[0].clone()],
        );
        assert_eq!(fragment.signals.fixtures.len(), 1);
        assert_eq!(fragment.patch.routes.len(), 2);
        assert_eq!(fragment.signals.pixel_count, 113);
        let unpatched = compare(&project, id, &selected[1..2]);
        assert!(unpatched.signals.fixtures.is_empty());
        assert!(unpatched.patch.routes.is_empty());
    }
}

#[test]
fn operators_keep_empty_inputs_and_unused_programs_are_removed() {
    use dawn_language::operator::{GraphOperatorNode, OperatorRef};
    use dawn_language::sequence::{
        CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
        GraphNodePosition, GraphPortId,
    };
    use dawn_runtime::BuiltinOperator;
    let mut project = starter();
    let ports = ports(&project);
    let id = project
        .root
        .sequences
        .iter()
        .find(|id| id.0.object() == "layer_test")
        .unwrap()
        .clone();
    let sequence = project.sequences.get_mut(&id).unwrap();
    sequence.automation_clips.clear();
    // Effects target the second port; the first must still receive inverted black.
    for effect in &mut sequence.effects {
        effect.target.fixture = FixtureInstanceId(2);
    }
    let output = sequence
        .composition_graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Output))
        .unwrap()
        .id
        .clone();
    // Invert one layer; the other disconnected layer must also be pruned.
    let layer = sequence
        .composition_graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Layer { .. }))
        .unwrap()
        .id
        .clone();
    sequence.composition_graph.edges = vec![
        EffectGraphEdge {
            from: layer,
            from_port: GraphPortId("output".into()),
            to: CompositionGraphNodeId(10000),
            to_port: GraphPortId("input".into()),
        },
        EffectGraphEdge {
            from: CompositionGraphNodeId(10000),
            from_port: GraphPortId("output".into()),
            to: output,
            to_port: GraphPortId("input".into()),
        },
    ];
    sequence
        .composition_graph
        .nodes
        .retain(|node| !matches!(node.kind, CompositionGraphNodeKind::Operator(_)));
    sequence.composition_graph.nodes.push(CompositionGraphNode {
        id: CompositionGraphNodeId(10000),
        position: GraphNodePosition { x: 0.0, y: 0.0 },
        kind: CompositionGraphNodeKind::Operator(GraphOperatorNode {
            operator: OperatorRef::Builtin(BuiltinOperator::Invert),
            params: Default::default(),
        }),
    });
    let fragment = compare(&project, &id, &ports[0..1]);
    assert!(fragment.signals.effects.is_empty());
    assert!(fragment.signals.programs.is_empty());
    assert!(fragment.signals.plan.nodes.iter().any(|node| matches!(
        node.kind,
        dawn_runtime::signal::PreparedSignalKind::Operator { .. }
    )));
    let mut workspace = fragment.workspace();
    let mut buffers = vec![vec![0; fragment.output_widths[0] as usize]];
    fragment
        .evaluate(SampleTime::from_ticks(0), &mut buffers, &mut workspace)
        .unwrap();
    assert!(buffers[0].iter().all(|&value| value == u8::MAX));
}

#[test]
fn empty_and_unknown_selections_are_explicit() {
    let project = starter();
    let ports = ports(&project);
    let id = &project.root.sequences[0];
    let empty = compare(&project, id, &[]);
    assert!(empty.signals.fixtures.is_empty());
    assert!(empty.signals.effects.is_empty());
    assert!(empty.signals.programs.is_empty());
    assert!(empty.signals.target_pixels.is_empty());
    assert!(empty.patch.routes.is_empty());
    let duplicate = PreparedSequenceOutput::prepare_selected(
        &project,
        &project.root.setup,
        id,
        &[ports[0].clone(), ports[0].clone()],
    );
    assert!(matches!(
        duplicate,
        Err(SequenceOutputPrepareError::DuplicateOutput { .. })
    ));
    let unknown = PreparedSequenceOutput::prepare_selected(
        &project,
        &project.root.setup,
        id,
        &[(ports[0].0.clone(), ControllerPortId(u32::MAX))],
    );
    assert!(matches!(
        unknown,
        Err(SequenceOutputPrepareError::UnknownOutput { .. })
    ));
}

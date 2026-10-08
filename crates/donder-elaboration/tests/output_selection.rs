use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_model::FixtureInstanceId;
use donder_model::PixelSpan;
use donder_model::SequenceId;
use donder_model::{ControllerId, ControllerPortId};
use donder_model::{DonderProject, ProjectEdit};
use donder_project_io::load_project;
use donder_runtime::PreparedSequence;
use donder_runtime_types::SampleTime;
use donder_runtime_types::sample_time_from_frame;

fn starter() -> DonderProject {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    load_project(&root).unwrap().project
}

fn ports(project: &DonderProject) -> Vec<(ControllerId, ControllerPortId)> {
    project.reusable_setups()[project.root().setup.id()]
        .controllers
        .iter()
        .flat_map(|source| {
            let id = source.id();
            project.reusable_controllers()[id]
                .ports
                .iter()
                .map(|port| (id.clone(), port.id))
        })
        .collect()
}

struct Reference {
    sequence: PreparedSequence,
    times: Vec<SampleTime>,
    frames: Vec<Vec<Vec<u8>>>,
}

fn prepare_reference(project: &DonderProject, id: &SequenceId) -> Reference {
    let sequence = prepare(project, id, PrepareOutputs::All).unwrap();
    let mut times = [9504, 8450, 0, 8494, 8398, 7150, 7151, 2000, 15000]
        .map(|frame| sample_time_from_frame(frame, sequence.frame_rate()).unwrap())
        .to_vec();
    times.extend(sequence.effect_windows().flat_map(|(start, duration)| {
        [Some(start), start.checked_add_duration(duration)]
            .into_iter()
            .flatten()
    }));
    times.extend([
        SampleTime::from_ticks(sequence.duration().as_ticks()),
        SampleTime::from_ticks(0),
    ]);
    let mut workspace = sequence.clone().into_playback();
    let frames = times
        .iter()
        .map(|&time| {
            workspace
                .evaluate(time)
                .outputs()
                .map(|output| output.bytes.to_vec())
                .collect()
        })
        .collect();
    Reference {
        sequence,
        times,
        frames,
    }
}

fn compare(
    project: &DonderProject,
    id: &SequenceId,
    reference: &Reference,
    selected: &[(ControllerId, ControllerPortId)],
) -> PreparedSequence {
    let full = &reference.sequence;
    let fragment = prepare(project, id, PrepareOutputs::Ports(selected)).unwrap();
    let mut workspace = fragment.clone().into_playback();
    let setup = project.setup(project.root().setup.id()).unwrap();
    let identity = |output: &donder_runtime::PreparedOutput| {
        (
            setup.controllers[output.controller_index as usize]
                .id()
                .clone(),
            ControllerPortId(output.port),
        )
    };
    let indices = selected
        .iter()
        .map(|selected| {
            full.outputs()
                .iter()
                .position(|output| identity(output) == *selected)
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(fragment.outputs().len(), selected.len());
    for (output, selected) in fragment.outputs().iter().zip(selected) {
        assert_eq!(&identity(output), selected);
    }
    for (&time, expected) in reference.times.iter().zip(&reference.frames) {
        for ((output, &index), selected) in workspace
            .evaluate(time)
            .outputs()
            .zip(&indices)
            .zip(selected)
        {
            assert_eq!(
                output.bytes,
                expected[index].as_slice(),
                "{} at {time:?}, port {:?}",
                id.0.root_source().object(),
                selected.1
            );
        }
    }
    fragment
}

#[test]
fn every_starter_port_matches_the_full_sequence_across_seeks() {
    let project = starter();
    let ports = ports(&project);
    for id in project.root().sequences.iter().map(|source| source.id()) {
        let reference = prepare_reference(&project, id);
        let reversed = ports.iter().rev().cloned().collect::<Vec<_>>();
        compare(&project, id, &reference, &reversed);
        for port in [ports.first().unwrap(), ports.last().unwrap()] {
            let fragment = compare(&project, id, &reference, std::slice::from_ref(port));
            assert_eq!(fragment.fixtures().len(), 1);
            assert_eq!(fragment.pixel_count(), 113);
            assert!(fragment.effect_count() <= reference.sequence.effect_count());
        }
    }
}

#[test]
fn split_fixture_keeps_original_context_and_compacts_disjoint_pixels() {
    let mut project = starter();
    let patch_id = project.reusable_setups()[project.root().setup.id()]
        .patch
        .id()
        .clone();
    let mut patch = project.patch(&patch_id).unwrap().clone();
    // Two ports wire disjoint spans of one fixture, preserving authored effect coordinates.
    for (index, start) in [(0, 0), (1, 76)] {
        let route = &mut patch.routes[index];
        route.target.fixture = FixtureInstanceId(2);
        route.pixels = Some(PixelSpan { start, count: 37 });
        route.start_slot = 7;
    }
    project.replace_patch(&patch_id, patch).unwrap();
    let ports = ports(&project);
    for id in project.root().sequences.iter().map(|source| source.id()) {
        let reference = prepare_reference(&project, id);
        let fragment = compare(
            &project,
            id,
            &reference,
            &[ports[1].clone(), ports[0].clone()],
        );
        assert_eq!(fragment.fixtures().len(), 1);
        assert_eq!(fragment.pixel_count(), 74);
        compare(&project, id, &reference, &ports[1..2]);
    }
    // Whole-target effects use different context from per-fixture effects.
    let edits = project
        .reusable_sequences()
        .values()
        .cloned()
        .map(|mut sequence| {
            for effect in &mut sequence.effects {
                effect.scope = donder_model::EffectScope::WholeTarget;
            }
            for collection in &mut sequence.mark_collections {
                collection.marks = [58_000_000, 59_000_000, 60_000_000]
                    .map(donder_language::DonderTime::from_micros)
                    .to_vec();
            }
            ProjectEdit::ReplaceSequence {
                id: sequence.id.clone(),
                value: sequence,
            }
        })
        .collect::<Vec<_>>();
    project.apply_edits(edits).unwrap();
    for id in project.root().sequences.iter().map(|source| source.id()) {
        let reference = prepare_reference(&project, id);
        compare(
            &project,
            id,
            &reference,
            &[ports[1].clone(), ports[0].clone()],
        );
    }
    // Spatial reads must not see the compacted 37-pixel output domain. Local
    // reads need the whole original fixture; global reads can reach unpatched
    // fixtures. This also exercises nested temporal/spatial operator sampling.
    for (query, expected_pixels) in [
        (
            "source.at(time + offset_seconds, target.count - 1 - pixel.index)",
            113,
        ),
        (
            "source.at_global(time + offset_seconds, 226 + pixel.index)",
            3390,
        ),
    ] {
        let compiled = donder_language::compiler::compile_operators(&format!(
            "operator TimeWarp {{ input source; param offset_seconds: float in -1.0..1.0 = 0.0; sample {{ {query} }} }}"
        )).unwrap().remove(0);
        let definition_id = project
            .definitions()
            .operators
            .definitions
            .iter()
            .find(|(_, definition)| definition.declaration_name == "TimeWarp")
            .unwrap()
            .0
            .clone();
        let definition = donder_model::custom_operator_definition(definition_id.clone(), compiled);
        project
            .apply_edits([donder_model::ProjectEdit::SetOperatorDefinition {
                id: definition_id,
                value: definition,
            }])
            .unwrap();
        let id = project
            .root()
            .sequences
            .iter()
            .map(|source| source.id())
            .find(|id| id.0.root_source().object() == "layer_test")
            .unwrap();
        let reference = prepare_reference(&project, id);
        let fragment = compare(&project, id, &reference, &ports[1..2]);
        assert_eq!(fragment.pixel_count(), expected_pixels, "{query}");
        // Exercise the serialized representation as well as live preparation.
        let encoded = donder_runtime::encode_sequence(&fragment).unwrap();
        let decoded = donder_runtime::decode_sequence(
            &encoded,
            donder_runtime::LoadLimits {
                // This tests fragment round-tripping, not the device upload size limit.
                payload_bytes: encoded.len() - donder_runtime::HEADER_BYTES,
                workspace_bytes: 4 * 1024 * 1024,
                ..Default::default()
            },
        )
        .unwrap();
        let mut original_workspace = fragment.clone().into_playback();
        let mut decoded_workspace = decoded.into_playback();
        let mut original = vec![vec![0; fragment.outputs()[0].width]];
        let mut restored = original.clone();
        let time = SampleTime::from_ticks(59_000_000);
        for (snapshot, output) in original
            .iter_mut()
            .zip(original_workspace.evaluate(time).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        for (snapshot, output) in restored
            .iter_mut()
            .zip(decoded_workspace.evaluate(time).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        assert_eq!(restored, original);
    }
}

#[test]
fn shared_pixels_and_multiple_controllers_keep_output_order() {
    use donder_model::SourceIdentity;
    let mut project = starter();
    let selected = ports(&project);
    let original_id = selected[0].0.clone();
    let other_id = ControllerId(
        SourceIdentity::from_document(
            original_id.0.document_id().clone(),
            "other_controller".into(),
        )
        .into(),
    );
    let mut other = project.reusable_controllers()[&original_id].clone();
    other.id = other_id.clone();
    let mut setup = project.setup(project.root().setup.id()).unwrap().clone();
    setup
        .controllers
        .push(donder_model::ValueSource::Reference(other_id.clone()));
    let mut patch = project.patch(setup.patch.id()).unwrap().clone();
    patch.routes[1].controller = other_id.clone();
    patch.routes[1].target = patch.routes[0].target.clone();
    project
        .apply_edits([
            ProjectEdit::InsertController(other),
            ProjectEdit::ReplaceSetup {
                id: setup.id.clone(),
                value: setup,
            },
            ProjectEdit::ReplacePatch {
                id: patch.id.clone(),
                value: patch,
            },
        ])
        .unwrap();
    for id in project.root().sequences.iter().map(|source| source.id()) {
        let reference = prepare_reference(&project, id);
        let fragment = compare(
            &project,
            id,
            &reference,
            &[(other_id.clone(), selected[1].1), selected[0].clone()],
        );
        assert_eq!(fragment.fixtures().len(), 1);
        assert_eq!(fragment.pixel_count(), 113);
        let unpatched = compare(&project, id, &reference, &selected[1..2]);
        assert!(unpatched.fixtures().is_empty());
    }
}

#[test]
fn operators_keep_empty_inputs_when_upstream_effects_are_pruned() {
    use donder_model::GraphOperatorNode;
    use donder_model::{
        CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
        GraphNodePosition, GraphPortId,
    };
    let mut project = starter();
    let invert = project
        .definitions()
        .operators
        .definitions
        .values()
        .find(|definition| definition.declaration_name == "Invert")
        .unwrap()
        .id()
        .clone();
    let ports = ports(&project);
    let id = project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
        .find(|id| id.0.root_source().object() == "layer_test")
        .unwrap()
        .clone();
    let mut sequence = project.sequence(&id).unwrap().clone();
    sequence.automation_clips.clear();
    // Effects target the second port; the first must still receive inverted black.
    let second = project
        .layout(&sequence.effects[0].target.layout)
        .unwrap()
        .iter_fixtures()
        .find(|fixture| fixture.name.as_str() == "output_02")
        .unwrap()
        .id;
    for effect in &mut sequence.effects {
        effect.target.fixture = second;
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
            name: donder_language::object_name("operator"),
            operator: invert,
            params: Default::default(),
        }),
    });
    project.replace_sequence(&id, sequence).unwrap();
    let reference = prepare_reference(&project, &id);
    let fragment = compare(&project, &id, &reference, &ports[0..1]);
    assert!(fragment.effect_count() == 0);
    let mut workspace = fragment.into_playback();
    let frame = workspace.evaluate(SampleTime::from_ticks(0));
    let output = frame.outputs().next().unwrap();
    assert!(output.bytes.iter().all(|&value| value == u8::MAX));
}

#[test]
fn empty_selection_prepares_an_empty_sequence() {
    let project = starter();
    let id = project.root().sequences[0].id();
    let empty = prepare(&project, id, PrepareOutputs::Ports(&[])).unwrap();
    assert!(empty.fixtures().is_empty());
    assert!(empty.effect_count() == 0);
    assert_eq!(empty.pixel_count(), 0);
    assert!(empty.outputs().is_empty());
}

mod support;

use camino::Utf8PathBuf;
use donder_elaboration::{Retain, prepare};
use donder_model::FixtureInstanceId;
use donder_model::PixelSpan;
use donder_model::SequenceId;
use donder_model::{ControllerId, ControllerPortId};
use donder_model::{DonderProject, ProjectEdit};
use donder_project_io::check_project;
use donder_runtime::PreparedShow;
use donder_runtime_types::SampleTime;
use donder_runtime_types::sample_time_from_frame;

fn starter() -> DonderProject {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    check_project(&root, &Default::default(), &mut Default::default())
        .into_session()
        .unwrap()
        .project
}

/// The starter and its `layer_test` sequence.
fn starter_with_layer_test() -> (DonderProject, SequenceId) {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let session = check_project(&root, &Default::default(), &mut Default::default())
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

fn ports(project: &DonderProject) -> Vec<(ControllerId, ControllerPortId)> {
    project.reusable::<donder_model::Setup>()[project.root().setup.id()]
        .controllers
        .iter()
        .flat_map(|source| {
            let id = source.id();
            project.reusable::<donder_model::Controller>()[id]
                .ports
                .iter()
                .map(|port| (*id, port.id))
        })
        .collect()
}

struct Reference {
    sequence: PreparedShow,
    times: Vec<SampleTime>,
    frames: Vec<Vec<Vec<u8>>>,
}

fn prepare_reference(project: &DonderProject, id: &SequenceId) -> Reference {
    let sequence = prepare(project, id, Retain::All).unwrap();
    let mut times = [9504, 8450, 0, 8494, 8398, 7150, 7151, 2000, 15000]
        .map(|frame| sample_time_from_frame(frame, sequence.frame_rate()).unwrap())
        .to_vec();
    times.extend(sequence.clip_windows().flat_map(|(start, duration)| {
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
) -> PreparedShow {
    let full = &reference.sequence;
    let fragment = prepare(project, id, Retain::Ports(selected)).unwrap();
    let mut workspace = fragment.clone().into_playback();
    let setup = project
        .get::<donder_model::Setup>(*project.root().setup.id())
        .unwrap();
    let identity = |output: &donder_runtime::Output| {
        (
            *setup.controllers[output.controller_index as usize].id(),
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
                "{id:?} at {time:?}, port {:?}",
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
            assert!(fragment.clip_count() <= reference.sequence.clip_count());
        }
    }
}

#[test]
fn split_fixture_keeps_original_context_and_compacts_disjoint_pixels() {
    let (mut project, layer_test) = starter_with_layer_test();
    let patch_id = *project.reusable::<donder_model::Setup>()[project.root().setup.id()]
        .patch
        .id();
    let mut patch = project
        .get::<donder_model::Patch>(patch_id)
        .unwrap()
        .clone();
    // Two ports wire disjoint spans of one fixture, preserving authored effect coordinates.
    for (index, start) in [(0, 0), (1, 76)] {
        let route = &mut patch.routes[index];
        route.target.fixture = FixtureInstanceId(2);
        route.pixels = Some(PixelSpan { start, count: 37 });
        route.start_slot = 7;
    }
    project.put(patch).unwrap();
    let ports = ports(&project);
    for id in project.root().sequences.iter().map(|source| source.id()) {
        let reference = prepare_reference(&project, id);
        let fragment = compare(&project, id, &reference, &[ports[1], ports[0]]);
        assert_eq!(fragment.fixtures().len(), 1);
        assert_eq!(fragment.pixel_count(), 74);
        compare(&project, id, &reference, &ports[1..2]);
    }
    // Whole-target effects use different context from per-fixture effects.
    let edits = project
        .reusable::<donder_model::Sequence>()
        .values()
        .cloned()
        .map(|mut sequence| {
            for effect in &mut sequence.clips {
                std::sync::Arc::make_mut(effect).scope = donder_model::ClipScope::WholeTarget;
            }
            for collection in &mut sequence.mark_collections {
                collection.marks = [58_000_000, 59_000_000, 60_000_000]
                    .map(|micros| {
                        donder_model::Mark::at(donder_language::DonderTime::from_micros(micros))
                    })
                    .to_vec();
            }
            ProjectEdit::Put(sequence.into())
        })
        .collect::<Vec<_>>();
    project.apply_edits(edits).unwrap();
    for id in project.root().sequences.iter().map(|source| source.id()) {
        let reference = prepare_reference(&project, id);
        compare(&project, id, &reference, &[ports[1], ports[0]]);
    }
    // Spatial reads must not see the compacted 37-pixel output domain: local
    // reads need the whole original fixture.
    for (query, expected_pixels) in [("source.at(target.count - 1 - pixel.index)", 113)] {
        let id = layer_test;
        let compiled = donder_language::compiler::compile_script(&format!(
            "operator Mirror {{ input source; sample {{ {query} }} }}"
        ))
        .unwrap()
        .remove(0);
        support::append_operator(&mut project, &id, compiled);
        let reference = prepare_reference(&project, &id);
        let fragment = compare(&project, &id, &reference, &ports[1..2]);
        assert_eq!(fragment.pixel_count(), expected_pixels, "{query}");
        // Exercise the serialized representation as well as live preparation.
        let encoded = fragment.encode();
        let decoded = donder_runtime::PreparedShow::decode(&encoded).unwrap();
        let mut original_workspace = fragment.clone().into_playback();
        let mut decoded_workspace = decoded.into_playback();
        let mut original = vec![vec![0; fragment.outputs()[0].width as usize]];
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
    let mut project = starter();
    let selected = ports(&project);
    let original_id = selected[0].0;
    let other_id = ControllerId(project.new_id());
    let mut other = project.reusable::<donder_model::Controller>()[&original_id].clone();
    other.id = other_id;
    let mut setup = project
        .get::<donder_model::Setup>(*project.root().setup.id())
        .unwrap()
        .clone();
    setup
        .controllers
        .push(donder_model::OwnedMember::Reference(other_id));
    let mut patch = project
        .get::<donder_model::Patch>(*setup.patch.id())
        .unwrap()
        .clone();
    patch.routes[1].controller = other_id;
    patch.routes[1].target = patch.routes[0].target.clone();
    project
        .apply_edits([
            ProjectEdit::Put(other.into()),
            ProjectEdit::Put(setup.into()),
            ProjectEdit::Put(patch.into()),
        ])
        .unwrap();
    for id in project.root().sequences.iter().map(|source| source.id()) {
        let reference = prepare_reference(&project, id);
        let fragment = compare(
            &project,
            id,
            &reference,
            &[(other_id, selected[1].1), selected[0]],
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
    let (mut project, layer_test) = starter_with_layer_test();
    let invert = *project
        .reusable::<donder_model::OperatorDefinition>()
        .values()
        .find(|definition| definition.name() == "Invert")
        .unwrap()
        .id();
    let ports = ports(&project);
    let id = layer_test;
    let mut sequence = project.get::<donder_model::Sequence>(id).unwrap().clone();
    sequence.automation_clips.clear();
    // Effects target the second port; the first must still receive inverted black.
    let second = project
        .get::<donder_model::Layout>(sequence.clips[0].target.layout)
        .unwrap()
        .iter_fixtures()
        .find(|fixture| fixture.name.as_str() == "output_02")
        .unwrap()
        .id;
    for effect in &mut sequence.clips {
        std::sync::Arc::make_mut(effect).target.fixture = second;
    }
    let output = sequence
        .composition_graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Root))
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
    project.put(sequence).unwrap();
    let reference = prepare_reference(&project, &id);
    let fragment = compare(&project, &id, &reference, &ports[0..1]);
    assert!(fragment.clip_count() == 0);
    let mut workspace = fragment.into_playback();
    let frame = workspace.evaluate(SampleTime::from_ticks(0));
    let output = frame.outputs().next().unwrap();
    assert!(output.bytes.iter().all(|&value| value == u8::MAX));
}

#[test]
fn empty_selection_prepares_an_empty_sequence() {
    let project = starter();
    let id = project.root().sequences[0].id();
    let empty = prepare(&project, id, Retain::Ports(&[])).unwrap();
    assert!(empty.fixtures().is_empty());
    assert!(empty.clip_count() == 0);
    assert_eq!(empty.pixel_count(), 0);
    assert!(empty.outputs().is_empty());
}

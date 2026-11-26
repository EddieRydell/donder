mod support;

use camino::Utf8PathBuf;
use donder_elaboration::{Retain, prepare};
use donder_model::FixtureId;
use donder_model::PixelSpan;
use donder_model::SequenceId;
use donder_model::{ControllerId, ControllerOutput, ControllerPortId};
use donder_model::{DonderProject, ProjectEdit};
use donder_project_io::check_project;
use donder_runtime::PreparedShow;
use donder_runtime_types::SampleTime;
use donder_runtime_types::sample_time_from_frame;

fn starter() -> DonderProject {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut project = check_project(&root, &Default::default(), &mut Default::default())
        .into_session()
        .unwrap()
        .project()
        .clone();
    port_per_universe(&mut project);
    project
}

/// Replace the starter's E1.31 controller with a Donder controller with a
/// port for each of its universes, one per fixture, so each fixture is an
/// output of its own.
fn port_per_universe(project: &mut DonderProject) {
    use donder_model::{
        ControllerPort, ControllerProtocol, DonderConfig, DonderDeviceId, DonderPort, RouteStart,
    };
    let controllers_store = project.reusable::<donder_model::Controller>();
    let id = *controllers_store.get_index(0).unwrap().0;
    let mut controller = project.reusable::<donder_model::Controller>()[&id].clone();
    let universes = *controller.universes().unwrap();
    let channels = u32::from(universes.channels);
    controller.protocol = ControllerProtocol::Donder(DonderConfig {
        device: DonderDeviceId::parse("000000000000").unwrap(),
        ports: (1..=universes.count)
            .map(|output| DonderPort {
                port: ControllerPort {
                    id: ControllerPortId(u32::from(output)),
                    name: format!("port_{output}"),
                    description: None,
                },
                output: output as u8,
                slot_count: universes.channels,
            })
            .collect(),
    });
    let mut patch = project.root().clone();
    let counts =
        donder_model::fixture_pixel_counts(project.reusable::<donder_model::FixtureDefinition>())
            .unwrap();
    let placements = donder_model::route_placements(project, &patch.routes, &counts);
    for route in &mut patch.routes {
        let start = placements[&route.id].channels.start;
        route.port = ControllerPortId(start / channels + 1);
        route.start = RouteStart::Channel(start % channels);
    }
    project
        .apply_edits([
            ProjectEdit::Put((id, controller).into()),
            ProjectEdit::Put(patch.into()),
        ])
        .unwrap();
}

/// The starter and its `layer_test` sequence.
fn starter_with_layer_test() -> (DonderProject, SequenceId) {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let session = check_project(&root, &Default::default(), &mut Default::default())
        .into_session()
        .unwrap();
    let mut project = session.project().clone();
    port_per_universe(&mut project);
    let id = session
        .project()
        .reusable::<donder_model::Sequence>()
        .keys()
        .copied()
        .find(|id| {
            session
                .source()
                .declared_name((*id).into())
                .is_some_and(|name| name.as_str() == "layer_test")
        })
        .unwrap();
    (project, id)
}

fn ports(project: &DonderProject) -> Vec<(ControllerId, ControllerOutput)> {
    project
        .reusable::<donder_model::Controller>()
        .iter()
        .flat_map(|(id, controller)| {
            controller
                .outputs()
                .into_iter()
                .map(|(output, _)| (*id, output))
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
    selected: &[(ControllerId, ControllerOutput)],
) -> PreparedShow {
    let full = &reference.sequence;
    let fragment = prepare(project, id, Retain::Outputs(selected)).unwrap();
    let mut workspace = fragment.clone().into_playback();
    let controllers_store = project.reusable::<donder_model::Controller>();
    let identity = |output: &donder_runtime::Output| {
        let controller = *controllers_store
            .get_index(output.controller_index as usize)
            .unwrap()
            .0;
        (
            controller,
            project.reusable::<donder_model::Controller>()[&controller].runtime_output(output.port),
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
    for id in project.reusable::<donder_model::Sequence>().keys() {
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
    let mut patch = project.root().clone();
    // Two ports wire disjoint spans of one fixture, preserving authored effect coordinates.
    for (index, start) in [(0, 0), (1, 76)] {
        let route = &mut patch.routes[index];
        route.target = FixtureId(2);
        route.pixels = Some(PixelSpan { start, count: 37 });
        route.start = donder_model::RouteStart::Channel(7);
    }
    project.put(patch).unwrap();
    let ports = ports(&project);
    for id in project.reusable::<donder_model::Sequence>().keys() {
        let reference = prepare_reference(&project, id);
        let fragment = compare(&project, id, &reference, &[ports[1], ports[0]]);
        assert_eq!(fragment.fixtures().len(), 1);
        assert_eq!(fragment.pixel_count(), 74);
        compare(&project, id, &reference, &ports[1..2]);
    }
    // Whole-target effects use different context from per-fixture effects.
    let edits = project
        .reusable::<donder_model::Sequence>()
        .iter()
        .map(|(id, sequence)| (*id, sequence.clone()))
        .map(|(id, mut sequence)| {
            for clip in &mut sequence.composition.clips {
                if let Some(effect) = std::sync::Arc::make_mut(clip).effect_mut() {
                    effect.scope = donder_runtime_types::TargetScope::WholeTarget;
                }
            }
            for collection in &mut sequence.composition.marks {
                collection.marks = [58_000_000, 59_000_000, 60_000_000]
                    .map(|micros| {
                        donder_model::Mark::at(donder_language::DonderTime::from_micros(micros))
                    })
                    .to_vec();
            }
            ProjectEdit::Put((id, sequence).into())
        })
        .collect::<Vec<_>>();
    project.apply_edits(edits).unwrap();
    for id in project.reusable::<donder_model::Sequence>().keys() {
        let reference = prepare_reference(&project, id);
        compare(&project, id, &reference, &[ports[1], ports[0]]);
    }
    // Spatial reads must not see the compacted 37-pixel output domain: local
    // reads need the whole original fixture.
    for (query, expected_pixels) in [("source[target.count - 1 - pixel.index]", 113)] {
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
    let other = project.reusable::<donder_model::Controller>()[&original_id].clone();
    let mut setup = project.root().clone();
    setup.routes[1].controller = other_id;
    setup.routes[1].target = setup.routes[0].target;
    project
        .apply_edits([
            ProjectEdit::Put((other_id, other).into()),
            ProjectEdit::Put(setup.into()),
        ])
        .unwrap();
    for id in project.reusable::<donder_model::Sequence>().keys() {
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
        GraphNodePosition,
    };
    let (mut project, layer_test) = starter_with_layer_test();
    let invert = *project
        .reusable::<donder_model::ScriptDefinition>()
        .iter()
        .find(|(_, definition)| definition.name() == "Invert")
        .unwrap()
        .0;
    let ports = ports(&project);
    let id = layer_test;
    let mut sequence = project.get::<donder_model::Sequence>(id).unwrap().clone();
    sequence
        .composition
        .clips
        .retain(|clip| clip.effect().is_some());
    // Effects target the second port; the first must still receive inverted black.
    let second = project
        .layout()
        .iter_fixtures()
        .find(|fixture| fixture.name.as_str() == "output_02")
        .unwrap()
        .id;
    for effect in &mut sequence.composition.clips {
        std::sync::Arc::make_mut(effect).targets = vec![second];
    }
    let output = sequence
        .composition
        .graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Root))
        .unwrap()
        .id
        .clone();
    // Invert one layer; the other disconnected layer must also be pruned.
    let layer = sequence
        .composition
        .graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Layer { .. }))
        .unwrap()
        .id
        .clone();
    sequence.composition.graph.edges = vec![
        EffectGraphEdge {
            from: layer,
            to: CompositionGraphNodeId(10000),
            input: Some(donder_language::object_name("input")),
        },
        EffectGraphEdge {
            from: CompositionGraphNodeId(10000),
            to: output,
            input: None,
        },
    ];
    sequence
        .composition
        .graph
        .nodes
        .retain(|node| !matches!(node.kind, CompositionGraphNodeKind::Operator(_)));
    sequence.composition.graph.nodes.push(CompositionGraphNode {
        id: CompositionGraphNodeId(10000),
        position: GraphNodePosition { x: 0.0, y: 0.0 },
        kind: CompositionGraphNodeKind::Operator(GraphOperatorNode {
            name: donder_language::object_name("operator"),
            operator: invert,
            params: Default::default(),
        }),
    });
    project.put((id, sequence)).unwrap();
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
    let id = project
        .reusable::<donder_model::Sequence>()
        .get_index(0)
        .unwrap()
        .0;
    let empty = prepare(&project, id, Retain::Outputs(&[])).unwrap();
    assert!(empty.fixtures().is_empty());
    assert!(empty.clip_count() == 0);
    assert_eq!(empty.pixel_count(), 0);
    assert!(empty.outputs().is_empty());
}

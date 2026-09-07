use camino::Utf8PathBuf;
use dawn_elaboration::{PreparedSequenceOutput, SequenceOutputPrepareError};
use dawn_language::controller::{ControllerId, ControllerPortId};
use dawn_language::element::{ElementCellRange, ElementNodeId};
use dawn_language::model::DawnProject;
use dawn_language::patch::{FilterDefinition, PatchNode, PatchNodeId};
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
            assert_eq!(fragment.elements.len(), 1);
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
            "{}: pixels {} -> {}; target records {} -> {}; effects {} -> {}; programs {} -> {}; patch steps {} -> {}; graph buffer bytes {} -> {}",
            id.0.object(),
            full.sequence.signals.pixel_count,
            fragment.signals.pixel_count,
            full.sequence.signals.target_pixels.len(),
            fragment.signals.target_pixels.len(),
            full.sequence.signals.effects.len(),
            fragment.signals.effects.len(),
            full.sequence.signals.programs.len(),
            fragment.signals.programs.len(),
            full.sequence.patch.steps.len(),
            fragment.patch.steps.len(),
            frame_bytes(&full.sequence),
            frame_bytes(&fragment)
        );
        let reversed = ports.iter().rev().cloned().collect::<Vec<_>>();
        compare(&project, id, &reversed);
    }
}

#[test]
fn split_element_keeps_original_context_and_compacts_disjoint_cells() {
    let mut project = starter();
    let patch_id = project.setups[&project.root.setup].patch.clone();
    let patch = project.patches.get_mut(&patch_id).unwrap();
    // Two ports use disjoint parts of one element. Both retain their positions
    // in the original 113-cell effect target, including through time-warp inputs.
    for (base, start) in [(1, 0), (6, 76)] {
        let PatchNode::Source(source) = &mut patch.nodes[&PatchNodeId(base)] else {
            unreachable!()
        };
        source.selection.node = ElementNodeId(1);
        source.selection.cells = Some(ElementCellRange { start, count: 37 });
        source.output = dawn_language::patch::PatchValueType::Color { width: 37 };
        for offset in [1, 2] {
            let PatchNode::Filter(filter) = &mut patch.nodes[&PatchNodeId(base + offset)] else {
                unreachable!()
            };
            match filter {
                FilterDefinition::ColorBreakdown { cell_count, .. }
                | FilterDefinition::ComponentReorder { cell_count, .. } => *cell_count = 37,
                _ => unreachable!(),
            }
        }
        let PatchNode::Filter(FilterDefinition::Quantize8 { width }) =
            &mut patch.nodes[&PatchNodeId(base + 3)]
        else {
            unreachable!()
        };
        *width = 111;
        let PatchNode::Sink(sink) = &mut patch.nodes[&PatchNodeId(base + 4)] else {
            unreachable!()
        };
        sink.start_slot = 7;
        sink.slot_count = 111;
    }
    let ports = ports(&project);
    for id in &project.root.sequences {
        let fragment = compare(&project, id, &[ports[1].clone(), ports[0].clone()]);
        assert_eq!(fragment.elements.len(), 1);
        assert_eq!(fragment.signals.pixel_count, 74);
        let target = fragment.signals.target(fragment.signals.plan.target);
        assert_eq!(target[37].element_cell_index, 37);
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
    // Spatial reads must not see the compacted 37-cell output domain. Local
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
fn shared_patch_paths_and_multiple_controllers_keep_output_order() {
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
    let PatchNode::Sink(sink) = &mut patch.nodes[&PatchNodeId(10)] else {
        unreachable!()
    };
    sink.controller = other_id.clone();
    // Two sinks consume the same already-packed source. The abandoned second
    // source branch must not survive, and the shared branch must not duplicate.
    let edge = patch
        .edges
        .iter_mut()
        .find(|edge| edge.to == PatchNodeId(10))
        .unwrap();
    edge.from = PatchNodeId(4);
    for id in &project.root.sequences {
        let fragment = compare(
            &project,
            id,
            &[(other_id.clone(), selected[1].1), selected[0].clone()],
        );
        assert_eq!(fragment.elements.len(), 1);
        assert_eq!(fragment.patch.steps.len(), 4);
        assert_eq!(fragment.signals.pixel_count, 113);
        let unpatched = compare(&project, id, &selected[1..2]);
        assert!(unpatched.elements.is_empty());
        assert!(unpatched.patch.steps.is_empty());
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
        effect.target.node = ElementNodeId(2);
        effect.target.cells = None;
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
    assert!(empty.elements.is_empty());
    assert!(empty.signals.effects.is_empty());
    assert!(empty.signals.programs.is_empty());
    assert!(empty.signals.target_pixels.is_empty());
    assert!(empty.patch.steps.is_empty());
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

#[test]
fn indexed_and_fixture_controls_follow_compacted_cells_and_unused_controls_drop() {
    use dawn_language::control::{ControlClip, ControlClipId, ControlTarget, ControlValue};
    use dawn_language::element::{
        ElementNode, ElementNodeKind, ElementSelection, IndexedOption, IndexedOptionId,
    };
    use dawn_language::fixture_profile::{
        DimmingCurve, FixtureBehaviorRule, FixtureChannel, FixtureChannelRole, FixtureFunction,
        FixtureFunctionId, FixtureFunctionKind, FixtureProfile, FixtureProfileId,
    };
    use dawn_language::identity::SourceIdentity;
    use dawn_language::patch::{PatchEdge, PatchPortId, PatchSink, PatchSource, PatchValueType};
    use dawn_language::values::{DawnDuration, DawnTime};
    let mut project = starter();
    let ports = ports(&project);
    let setup = project.setups[&project.root.setup].clone();
    let profile_id = FixtureProfileId(SourceIdentity::from_document(
        setup.elements.0.document_id().clone(),
        "dimmer_test".into(),
    ));
    let function = FixtureFunctionId(9);
    project.definitions.fixture_profiles.definitions.insert(
        profile_id.clone(),
        FixtureProfile {
            id: profile_id.clone(),
            functions: [(
                function,
                FixtureFunction {
                    name: "dimmer".into(),
                    tag: None,
                    kind: FixtureFunctionKind::Range,
                    curve: DimmingCurve::Linear,
                },
            )]
            .into(),
            channels: vec![FixtureChannel {
                slot: 0,
                role: FixtureChannelRole::Coarse { function },
                curve: DimmingCurve::Linear,
            }],
            behavior_rules: vec![FixtureBehaviorRule::Dimmer {
                function,
                off: 0.2,
                on: 1.0,
            }],
        },
    );
    let tree = project.element_trees.get_mut(&setup.elements).unwrap();
    for (id, kind) in [
        (10001, ElementNodeKind::Scalar { cells: 6 }),
        (
            10002,
            ElementNodeKind::Indexed {
                cells: 4,
                options: vec![
                    IndexedOption {
                        id: IndexedOptionId(0),
                        name: "off".into(),
                    },
                    IndexedOption {
                        id: IndexedOptionId(7),
                        name: "mode".into(),
                    },
                ],
            },
        ),
        (
            10003,
            ElementNodeKind::Fixture {
                profile: profile_id.clone(),
            },
        ),
        (
            10004,
            ElementNodeKind::Fixture {
                profile: profile_id.clone(),
            },
        ),
    ] {
        tree.nodes.insert(
            ElementNodeId(id),
            ElementNode {
                name: id.to_string(),
                kind,
            },
        );
        tree.roots.push(ElementNodeId(id));
    }
    let selection = |node, cells| ElementSelection {
        tree: setup.elements.clone(),
        node: ElementNodeId(node),
        cells,
    };
    let patch = project.patches.get_mut(&setup.patch).unwrap();
    patch.nodes.clear();
    patch.edges.clear();
    let routes = [
        (
            10002,
            Some(ElementCellRange { start: 1, count: 2 }),
            PatchValueType::Indexed { width: 2 },
            vec![
                FilterDefinition::IndexedValueMapping {
                    entries: [(0, 0.0), (7, 0.5)].into(),
                    width: 2,
                },
                FilterDefinition::Quantize8 { width: 2 },
            ],
            0,
            0,
            2,
        ),
        (
            10003,
            None,
            PatchValueType::FixtureState {
                width: 1,
                profile: profile_id.clone(),
            },
            vec![FilterDefinition::FixtureProfileEncoding {
                profile: profile_id.clone(),
                fixture_count: 1,
                slot_count: 1,
            }],
            0,
            2,
            1,
        ),
        (
            10004,
            None,
            PatchValueType::FixtureState {
                width: 1,
                profile: profile_id.clone(),
            },
            vec![FilterDefinition::FixtureProfileEncoding {
                profile: profile_id,
                fixture_count: 1,
                slot_count: 1,
            }],
            1,
            0,
            1,
        ),
    ];
    for (node, cells, output, filters, port, start_slot, slot_count) in routes {
        let start = patch.nodes.len() as u32;
        patch.nodes.insert(
            PatchNodeId(start),
            PatchNode::Source(PatchSource {
                selection: selection(node, cells),
                output,
            }),
        );
        for filter in filters {
            let id = patch.nodes.len() as u32;
            patch
                .nodes
                .insert(PatchNodeId(id), PatchNode::Filter(filter));
        }
        let end = patch.nodes.len() as u32;
        patch.nodes.insert(
            PatchNodeId(end),
            PatchNode::Sink(PatchSink {
                controller: ports[port].0.clone(),
                port: ports[port].1,
                start_slot,
                slot_count,
            }),
        );
        for id in start..end {
            patch.edges.push(PatchEdge {
                from: PatchNodeId(id),
                from_port: PatchPortId(0),
                to: PatchNodeId(id + 1),
                to_port: PatchPortId(0),
            });
        }
    }
    let id = project
        .root
        .sequences
        .iter()
        .find(|id| id.0.object() == "empty")
        .unwrap()
        .clone();
    let sequence = project.sequences.get_mut(&id).unwrap();
    sequence.control_clips = [
        (
            ControlTarget::Scalar(selection(10001, None)),
            ControlValue::ConstantNormalized(0.25),
        ),
        (
            ControlTarget::Indexed(selection(10002, None)),
            ControlValue::Indexed {
                option: IndexedOptionId(7),
                range_curve: None,
            },
        ),
        (
            ControlTarget::FixtureFunction {
                selection: selection(10003, None),
                function,
            },
            ControlValue::ConstantNormalized(0.7),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (target, value))| ControlClip {
        id: ControlClipId(index as u32),
        start: DawnTime::from_micros(0),
        duration: DawnDuration::from_micros(10_000_000),
        target,
        value,
    })
    .collect();
    let fragment = compare(&project, &id, &ports[..1]);
    assert_eq!(fragment.elements.len(), 2);
    assert_eq!(fragment.controls.len(), 2);
    assert_eq!(fragment.controls[0].addresses.len(), 2);
    assert_eq!(fragment.controls[1].addresses.len(), 1);
    assert_eq!(fragment.fixture_behaviors.rules.len(), 1);
    assert_eq!(fragment.patch.fixture_programs.len(), 1);
    let mut workspace = fragment.workspace();
    let mut buffers = vec![vec![0; fragment.output_widths[0] as usize]];
    fragment
        .evaluate(SampleTime::from_ticks(0), &mut buffers, &mut workspace)
        .unwrap();
    assert_eq!(&buffers[0][..3], &[128, 128, 179]);
    let other = compare(&project, &id, &ports[1..2]);
    assert_eq!(other.elements.len(), 1);
    assert!(other.controls.is_empty());
    let both = compare(&project, &id, &ports[..2]);
    assert_eq!(both.fixture_behaviors.rules.len(), 1);
    assert_eq!(both.fixture_behaviors.bindings.len(), 2);
}

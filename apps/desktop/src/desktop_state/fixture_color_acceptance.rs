use super::DesktopState;
use super::advanced_patch_acceptance::accepted_elements;
use super::advanced_patch_acceptance::{accepted_patch, replace_patch};
use crate::dto::*;
use crate::project::{new_project_files, write_new_project_files};
use camino::Utf8PathBuf;

#[test]
fn fixture_colors_behaviors_and_overrides_render_and_roundtrip() {
    for model in [GuiFixtureColorModel::Rgb, GuiFixtureColorModel::Rgbw] {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
        write_new_project_files(&root, &new_project_files("Fixture colors").unwrap()).unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let mut settings = state.snapshot().settings;
        settings.autosave_project_edits = false;
        state.update_app_settings(settings);
        let initial = state.project_session().unwrap();
        let setup = initial.project.root.setup.clone();
        let sequence = initial.project.root.sequences[0].clone();
        let css_color = |name: &str| {
            include_str!("../../frontend/src/styles.css")
                .lines()
                .find_map(|line| line.trim().strip_prefix(name))
                .unwrap()
                .trim()
                .trim_end_matches(';')
                .to_string()
        };
        let white = css_color("--dawn-white:");
        let black = css_color("--dawn-default-project-color:");
        let request = |view, path: String, key: String| GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path,
            view,
            object_key: Some(key),
        };
        let setup_edit = |edit| {
            let result = state.apply_gui_edit(
                request(
                    DocumentViewId::Setup,
                    setup.0.document().to_string(),
                    setup.0.object().into(),
                ),
                GuiEditCommand::Setup { edit },
            );
            match result.document {
                GuiDocument::Setup { document } => document,
                other => panic!("{other:?}"),
            }
        };
        let sequence_edit = |edit| {
            let result = state.apply_gui_edit(
                request(
                    DocumentViewId::Sequence,
                    sequence.0.document().to_string(),
                    sequence.0.object().into(),
                ),
                GuiEditCommand::Sequence { edit },
            );
            assert!(
                matches!(result.document, GuiDocument::Sequence { .. }),
                "{:?}",
                result.document
            );
        };
        let entry = |id, value| GuiFixtureEntry {
            id,
            name: format!("Entry {id}"),
            dmx_min: value,
            dmx_max: value,
            curve_control: false,
            color: None,
            tag: None,
        };
        let function = |id, kind| GuiFixtureFunction {
            id,
            name: format!("Function {id}"),
            tag: None,
            kind,
            curve: GuiDimmingCurve::Linear,
        };
        let mut components = vec![
            GuiFixtureColorComponent::Red,
            GuiFixtureColorComponent::Green,
            GuiFixtureColorComponent::Blue,
        ];
        if matches!(model, GuiFixtureColorModel::Rgbw) {
            components.push(GuiFixtureColorComponent::White);
        }
        let width = components.len() as u16;
        let mut channels = components
            .iter()
            .enumerate()
            .map(|(slot, component)| GuiFixtureChannel {
                slot: slot as u16,
                role: GuiFixtureChannelRole::ColorComponent {
                    function: 1,
                    component: *component,
                },
                curve: GuiDimmingCurve::Linear,
            })
            .collect::<Vec<_>>();
        channels.extend((2..=5).map(|function| GuiFixtureChannel {
            slot: width + function as u16 - 2,
            role: GuiFixtureChannelRole::Coarse { function },
            curve: GuiDimmingCurve::Linear,
        }));
        let definition = GuiFixtureDefinition {
            functions: vec![
                function(1, GuiFixtureFunctionKind::ColorMixing { model }),
                function(2, GuiFixtureFunctionKind::Range),
                function(
                    3,
                    GuiFixtureFunctionKind::Indexed {
                        entries: vec![entry(10, 0), entry(20, 255)],
                    },
                ),
                function(
                    4,
                    GuiFixtureFunctionKind::ColorWheel {
                        entries: vec![entry(30, 0), entry(40, 100)],
                    },
                ),
                function(
                    5,
                    GuiFixtureFunctionKind::Indexed {
                        entries: vec![entry(50, 0), entry(60, 200)],
                    },
                ),
            ],
            channels,
            behavior_rules: vec![
                GuiFixtureBehavior::Dimmer {
                    function: 2,
                    off: 0.0,
                    on: 1.0,
                },
                GuiFixtureBehavior::Shutter {
                    function: 3,
                    closed: 10,
                    open: 20,
                },
                GuiFixtureBehavior::ColorWheel {
                    function: 4,
                    entries: vec![
                        GuiFixtureColorMapping {
                            color: black.clone(),
                            entry: 30,
                        },
                        GuiFixtureColorMapping {
                            color: white.clone(),
                            entry: 40,
                        },
                    ],
                },
                GuiFixtureBehavior::PrismGate {
                    function: 5,
                    disabled: 50,
                    enabled: 60,
                },
            ],
        };
        state.open_file_path(setup.0.document().as_str());
        let profile = setup_edit(SetupGuiEdit::CreateFixtureProfile {
            name: "colors".into(),
            definition: definition.clone(),
        })
        .fixture_profiles[0]
            .source_ref
            .clone();
        let node = accepted_elements(
            &state,
            ElementTreeGuiEdit::AddControlElement {
                name: "Fixture".into(),
                parent: None,
                definition: SetupControlElement::Fixture {
                    profile: profile.clone(),
                },
            },
        )
        .elements[0]
            .id;
        let controller = setup_edit(SetupGuiEdit::AddController {
            config: SetupControllerConfig::E131 {
                source_name: "Fixture colors".into(),
                bind_address: "0.0.0.0".into(),
                priority: 100,
                destination: Some("127.0.0.1".into()),
            },
            ports: vec![SetupControllerPort {
                id: 1,
                address: 1,
                slot_count: width + 4,
            }],
        })
        .controllers[0]
            .source_ref
            .clone();
        setup_edit(SetupGuiEdit::AssignFixtureOutput {
            node,
            controller,
            port: 1,
            start_slot: 0,
            mode: SetupOutputAssignmentMode::Add,
        });
        let accepted = state.project_session().unwrap();
        let mut invalid = definition;
        invalid.behavior_rules.push(GuiFixtureBehavior::Dimmer {
            function: 2,
            off: 0.0,
            on: 0.5,
        });
        let rejected = state.apply_gui_edit(
            request(
                DocumentViewId::FixtureProfile,
                profile.path.clone(),
                profile.object_key.clone(),
            ),
            GuiEditCommand::FixtureProfile {
                definition: invalid,
            },
        );
        assert!(matches!(rejected.document, GuiDocument::Blocked { .. }));
        assert!(std::sync::Arc::ptr_eq(
            &accepted,
            &state.project_session().unwrap()
        ));
        state.open_file_path(sequence.0.document().as_str());
        sequence_edit(SequenceGuiEdit::AddEffect {
            initial_color: white.clone(),
            effect: SequenceEffectReference::Builtin {
                effect: SequenceBuiltinEffect::Pulse,
            },
            target: ElementTarget {
                kind: ElementTargetKind::Element,
                name: node.to_string(),
            },
            scope: SequenceEffectScope::WholeTarget,
            start_seconds: 0.0,
            mark_collection_key: None,
        });
        let effect = state.project_session().unwrap().project.sequences[&sequence].effects[0]
            .id
            .0;
        sequence_edit(SequenceGuiEdit::UpdateEffectParam {
            id: effect,
            name: "pulse_shape".into(),
            value: SequenceEffectParamValue::Curve {
                value: crate::dto::SequenceCurveValue {
                    source: crate::dto::SequenceLibrarySource::Inline,
                    points: vec![
                        SequenceCurvePoint {
                            time: 0.0,
                            value: 1.0,
                        },
                        SequenceCurvePoint {
                            time: 1.0,
                            value: 1.0,
                        },
                    ],
                },
            },
        });
        for (function, value) in [
            (1, SequenceControlValue::ConstantColor { value: black }),
            (2, SequenceControlValue::ConstantNormalized { value: 0.5 }),
            (
                3,
                SequenceControlValue::FixtureIndexed {
                    entry: 10,
                    range_curve: None,
                },
            ),
            (
                4,
                SequenceControlValue::FixtureIndexed {
                    entry: 30,
                    range_curve: None,
                },
            ),
            (
                5,
                SequenceControlValue::FixtureIndexed {
                    entry: 50,
                    range_curve: None,
                },
            ),
        ] {
            sequence_edit(SequenceGuiEdit::UpsertControlClip {
                id: None,
                start_seconds: 0.5,
                duration_seconds: 0.25,
                target: SequenceControlTarget::FixtureFunction {
                    node,
                    cells: None,
                    function,
                },
                value,
            });
        }
        let final_session = state.project_session().unwrap();
        state.undo_active_edit();
        state.redo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *final_session);
        let output = dawn_elaboration::PreparedSequenceOutput::prepare(
            &final_session.project,
            &setup,
            &sequence,
        )
        .unwrap();
        let mut lit = if width == 4 {
            vec![0, 0, 0, 255]
        } else {
            vec![255; 3]
        };
        lit.extend([255, 255, 100, 200]);
        let mut overridden = vec![0; width as usize];
        overridden.extend([128, 0, 0, 0]);
        for (time, expected) in [
            (0.25, lit.clone()),
            (0.6, overridden),
            (0.9, lit),
            (60.0, vec![0; width as usize + 4]),
        ] {
            assert_eq!(
                output.render_seconds(time).unwrap().controller_frames[0].slots,
                expected
            );
        }
        state.save_all().unwrap();
        let reloaded = dawn_project_io::load_package(&root).unwrap().session;
        assert_eq!(reloaded.project, final_session.project);
        let reopened =
            dawn_elaboration::PreparedSequenceOutput::prepare(&reloaded.project, &setup, &sequence)
                .unwrap();
        for time in [0.25, 0.6, 0.9, 60.0] {
            assert_eq!(
                reopened.render_seconds(time).unwrap().controller_frames,
                output.render_seconds(time).unwrap().controller_frames
            );
        }
        state.open_file_path(setup.0.document().as_str());
        let GuiDocument::Setup { document } = state
            .get_gui_document(request(
                DocumentViewId::Setup,
                setup.0.document().to_string(),
                setup.0.object().into(),
            ))
            .document
        else {
            panic!("setup projection missing")
        };
        let mut nodes = document.patch_definitions;
        let mut edges = document.patch_edges;
        let mut encoders = 0;
        for patch_node in &mut nodes {
            patch_node.id += 100;
            match &mut patch_node.definition {
                PatchGuiNodeDefinition::Source {
                    cells,
                    output: PatchGuiValueType::FixtureState { width, .. },
                    ..
                } => {
                    assert_eq!(*width, 1);
                    *cells = Some(PatchGuiCellRange { start: 0, count: 1 });
                }
                PatchGuiNodeDefinition::Filter {
                    filter:
                        PatchGuiFilter::FixtureProfileEncoding {
                            fixture_count,
                            slot_count,
                            ..
                        },
                } => {
                    assert_eq!(*fixture_count, 1);
                    assert_eq!(*slot_count, u32::from(width + 4));
                    encoders += 1;
                }
                _ => {}
            }
        }
        assert_eq!(encoders, 1);
        for edge in &mut edges {
            edge.from_node += 100;
            edge.to_node += 100;
        }
        let mut invalid = nodes.clone();
        for patch_node in &mut invalid {
            if let PatchGuiNodeDefinition::Filter {
                filter: PatchGuiFilter::FixtureProfileEncoding { slot_count, .. },
            } = &mut patch_node.definition
            {
                *slot_count += 1;
            }
        }
        let rejected = replace_patch(&state, invalid, edges.clone());
        assert!(
            matches!(rejected.document, GuiDocument::Blocked { .. }),
            "{:?}",
            rejected.document
        );
        assert!(std::sync::Arc::ptr_eq(
            &final_session,
            &state.project_session().unwrap()
        ));
        let projected = accepted_patch(&state, nodes.clone(), edges.clone());
        assert_eq!(
            serde_json::to_value(projected.nodes).unwrap(),
            serde_json::to_value(nodes).unwrap()
        );
        assert_eq!(
            serde_json::to_value(projected.edges).unwrap(),
            serde_json::to_value(edges).unwrap()
        );
        let advanced = state.project_session().unwrap();
        state.undo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *final_session);
        state.redo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *advanced);
        state.save_all().unwrap();
        let reloaded = dawn_project_io::load_package(&root).unwrap().session;
        assert_eq!(reloaded.project, advanced.project);
        let reopened =
            dawn_elaboration::PreparedSequenceOutput::prepare(&reloaded.project, &setup, &sequence)
                .unwrap();
        for time in [0.25, 0.6, 0.9, 60.0] {
            assert_eq!(
                reopened.render_seconds(time).unwrap().controller_frames,
                output.render_seconds(time).unwrap().controller_frames
            );
        }
    }
}

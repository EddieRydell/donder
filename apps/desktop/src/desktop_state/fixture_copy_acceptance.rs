use super::DesktopState;
use super::advanced_patch_acceptance::accepted_elements;
use crate::dto::*;
use crate::project::{new_project_files, write_new_project_files};
use camino::Utf8PathBuf;

#[test]
fn copied_fixture_profiles_keep_group_controls_and_encoded_output() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_project_files("Fixture copy").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let initial = state.project_session().unwrap();
    let setup_id = initial.project.root.setup.clone();
    let sequence_id = initial.project.root.sequences[0].clone();
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
                setup_id.0.document().to_string(),
                setup_id.0.object().into(),
            ),
            GuiEditCommand::Setup { edit },
        );
        match result.document {
            GuiDocument::Setup { document } => document,
            other => panic!("{other:?}"),
        }
    };
    state.open_file_path(setup_id.0.document().as_str());
    let profile = setup_edit(SetupGuiEdit::CreateFixtureProfile {
        name: "dimmer".into(),
        definition: GuiFixtureDefinition {
            functions: vec![
                GuiFixtureFunction {
                    id: 7,
                    name: "Level".into(),
                    tag: None,
                    kind: GuiFixtureFunctionKind::Range,
                    curve: GuiDimmingCurve::Linear,
                },
                GuiFixtureFunction {
                    id: 19,
                    name: "Shutter".into(),
                    tag: None,
                    curve: GuiDimmingCurve::Linear,
                    kind: GuiFixtureFunctionKind::Indexed {
                        entries: vec![
                            GuiFixtureEntry {
                                id: 41,
                                name: "Closed".into(),
                                dmx_min: 0,
                                dmx_max: 31,
                                curve_control: false,
                                color: None,
                                tag: None,
                            },
                            GuiFixtureEntry {
                                id: 42,
                                name: "Open".into(),
                                dmx_min: 32,
                                dmx_max: 255,
                                curve_control: true,
                                color: None,
                                tag: None,
                            },
                        ],
                    },
                },
            ],
            channels: vec![
                GuiFixtureChannel {
                    slot: 0,
                    role: GuiFixtureChannelRole::Coarse { function: 7 },
                    curve: GuiDimmingCurve::Linear,
                },
                GuiFixtureChannel {
                    slot: 1,
                    role: GuiFixtureChannelRole::Fine { function: 7 },
                    curve: GuiDimmingCurve::Linear,
                },
                GuiFixtureChannel {
                    slot: 2,
                    role: GuiFixtureChannelRole::Coarse { function: 19 },
                    curve: GuiDimmingCurve::Linear,
                },
            ],
            behavior_rules: vec![],
        },
    })
    .fixture_profiles[0]
        .source_ref
        .clone();
    assert!(profile.path.starts_with("fixture-profiles/"));
    let group = accepted_elements(
        &state,
        ElementTreeGuiEdit::AddGroup {
            name: "Fixtures".into(),
            parent: None,
        },
    )
    .root_ids[0];
    let mut fixtures = Vec::new();
    for name in ["Left", "Right"] {
        let document = accepted_elements(
            &state,
            ElementTreeGuiEdit::AddControlElement {
                name: name.into(),
                parent: Some(group),
                definition: SetupControlElement::Fixture {
                    profile: profile.clone(),
                },
            },
        );
        fixtures.push(document.elements.last().unwrap().id);
    }
    let controller = setup_edit(SetupGuiEdit::AddController {
        config: SetupControllerConfig::E131 {
            source_name: "Fixture copy".into(),
            bind_address: "0.0.0.0".into(),
            priority: 100,
            destination: Some("127.0.0.1".into()),
        },
        ports: vec![SetupControllerPort {
            id: 1,
            address: 1,
            slot_count: 6,
        }],
    })
    .controllers[0]
        .source_ref
        .clone();
    for (index, node) in fixtures.iter().enumerate() {
        setup_edit(SetupGuiEdit::AssignFixtureOutput {
            node: *node,
            controller: controller.clone(),
            port: 1,
            start_slot: index as u16 * 3,
            mode: SetupOutputAssignmentMode::Add,
        });
    }
    state.open_file_path(sequence_id.0.document().as_str());
    for (node, function, value) in [
        (
            group,
            7,
            SequenceControlValue::ConstantNormalized { value: 0.5 },
        ),
        (
            fixtures[0],
            19,
            SequenceControlValue::FixtureIndexed {
                entry: 42,
                range_curve: None,
            },
        ),
    ] {
        let result = state.apply_gui_edit(
            request(
                DocumentViewId::Sequence,
                sequence_id.0.document().to_string(),
                sequence_id.0.object().into(),
            ),
            GuiEditCommand::Sequence {
                edit: SequenceGuiEdit::UpsertControlClip {
                    id: None,
                    start_seconds: 1.0,
                    duration_seconds: 2.0,
                    target: SequenceControlTarget::FixtureFunction {
                        node,
                        cells: None,
                        function,
                    },
                    value,
                },
            },
        );
        assert!(
            matches!(result.document, GuiDocument::Sequence { .. }),
            "{:?}",
            result.document
        );
    }
    let before = state.project_session().unwrap();
    let original_output =
        dawn_elaboration::PreparedSequenceOutput::prepare(&before.project, &setup_id, &sequence_id)
            .unwrap();
    state.open_file_path(setup_id.0.document().as_str());
    setup_edit(SetupGuiEdit::CopyLayout);
    let after = state.project_session().unwrap();
    let tree = &after.project.element_trees[&after.project.setups[&setup_id].elements];
    let mut copied_profile = None;
    for id in fixtures {
        let dawn_language::element::ElementNodeKind::Fixture { profile } =
            &tree.nodes[&dawn_language::element::ElementNodeId(id)].kind
        else {
            panic!("fixture lost")
        };
        assert!(after.source.is_project_owned(profile.0.document_id()));
        if let Some(previous) = &copied_profile {
            assert_eq!(profile, previous);
        }
        copied_profile = Some(profile.clone());
    }
    let copied_profile = copied_profile.unwrap();
    assert_eq!(
        after.project.definitions.fixture_profiles.definitions.len(),
        2
    );
    for node in after.project.patches[&after.project.setups[&setup_id].patch]
        .nodes
        .values()
    {
        if let dawn_language::patch::PatchNode::Filter(
            dawn_language::patch::FilterDefinition::FixtureProfileEncoding { profile, .. },
        ) = node
        {
            assert_eq!(*profile, copied_profile);
        }
    }
    for (old, new) in before.project.sequences[&sequence_id]
        .control_clips
        .iter()
        .zip(&after.project.sequences[&sequence_id].control_clips)
    {
        assert_eq!(old.id, new.id);
        assert_eq!(old.value, new.value);
        assert_eq!(old.target.selection().node, new.target.selection().node);
        assert_eq!(new.target.selection().tree, tree.id);
    }
    let output =
        dawn_elaboration::PreparedSequenceOutput::prepare(&after.project, &setup_id, &sequence_id)
            .unwrap();
    for (time, expected) in [(0.0, [0; 6]), (1.5, [128, 0, 32, 128, 0, 0]), (3.0, [0; 6])] {
        let frame = output.render_seconds(time).unwrap();
        assert_eq!(frame.controller_frames[0].slots, expected);
        assert_eq!(
            frame.controller_frames[0].slots,
            original_output
                .render_seconds(time)
                .unwrap()
                .controller_frames[0]
                .slots
        );
    }
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *before);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *after);
    state.save_all().unwrap();
    assert_eq!(
        dawn_project_io::load_package(&root)
            .unwrap()
            .session
            .project,
        after.project
    );
    drop(state);
    let plan = dawn_project_io::plan_path_change(
        &after,
        setup_id.0.document(),
        camino::Utf8Path::new("setups/copied.setup.dawn"),
    )
    .unwrap();
    let moved = dawn_project_io::apply_path_change(&after, &plan).unwrap();
    assert_eq!(
        dawn_project_io::load_package(&root)
            .unwrap()
            .session
            .project,
        moved.project
    );
    let moved_output = dawn_elaboration::PreparedSequenceOutput::prepare(
        &moved.project,
        &moved.project.root.setup,
        &sequence_id,
    )
    .unwrap();
    assert_eq!(
        moved_output.render_seconds(1.5).unwrap().controller_frames[0].slots,
        [128, 0, 32, 128, 0, 0]
    );
}

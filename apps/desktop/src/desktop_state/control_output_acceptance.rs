use super::DesktopState;
use super::advanced_patch_acceptance::{accepted_elements, edit_elements};
use super::advanced_patch_acceptance::{accepted_patch, replace_patch};
use crate::dto::*;
use crate::project::{new_project_files, write_new_project_files};
use camino::Utf8PathBuf;

#[test]
fn guided_controls_render_replace_reject_overlap_and_roundtrip() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_project_files("Control outputs").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let initial = state.project_session().unwrap();
    let setup = initial.project.root.setup.clone();
    let sequence = initial.project.root.sequences[0].clone();
    let setup_edit = |edit| {
        state.apply_gui_edit(
            GuiDocumentRequest {
                project_revision: state.snapshot().project_revision,
                path: setup.0.document().to_string(),
                view: DocumentViewId::Setup,
                object_key: Some(setup.0.object().into()),
            },
            GuiEditCommand::Setup { edit },
        )
    };
    let accepted = |edit| {
        let result = setup_edit(edit);
        match result.document {
            GuiDocument::Setup { document } => document,
            other => panic!("{other:?}"),
        }
    };
    state.open_file_path(setup.0.document().as_str());
    let scalar = accepted_elements(
        &state,
        ElementTreeGuiEdit::AddControlElement {
            name: "Dimmers".into(),
            parent: None,
            definition: SetupControlElement::Scalar { cells: 2 },
        },
    )
    .elements
    .last()
    .unwrap()
    .id;
    let indexed = accepted_elements(
        &state,
        ElementTreeGuiEdit::AddControlElement {
            name: "Switches".into(),
            parent: None,
            definition: SetupControlElement::Indexed {
                cells: 2,
                options: vec![
                    SetupIndexedOption {
                        id: 0,
                        name: "Off".into(),
                    },
                    SetupIndexedOption {
                        id: 9,
                        name: "On".into(),
                    },
                ],
            },
        },
    )
    .elements
    .last()
    .unwrap()
    .id;
    let controller = accepted(SetupGuiEdit::AddController {
        config: SetupControllerConfig::E131 {
            source_name: "Controls".into(),
            bind_address: "0.0.0.0".into(),
            priority: 100,
            destination: Some("127.0.0.1".into()),
        },
        ports: vec![SetupControllerPort {
            id: 1,
            address: 1,
            slot_count: 8,
        }],
    })
    .controllers[0]
        .source_ref
        .clone();
    let assignment = |node, start_slot, mapping| SetupControlOutputAssignment {
        node,
        controller: controller.clone(),
        port: 1,
        start_slot,
        mapping,
    };
    let indexed_mapping = || SetupControlOutputMapping::Indexed {
        entries: vec![
            SetupIndexedChannel { id: 0, value: 0 },
            SetupIndexedChannel { id: 9, value: 200 },
        ],
    };
    accepted(SetupGuiEdit::AssignControlOutput {
        assignment: assignment(scalar, 0, SetupControlOutputMapping::Scalar),
        mode: SetupOutputAssignmentMode::Add,
    });
    accepted(SetupGuiEdit::AssignControlOutput {
        assignment: assignment(indexed, 2, indexed_mapping()),
        mode: SetupOutputAssignmentMode::Add,
    });
    let before = state.project_session().unwrap();
    for bad in [
        assignment(scalar, 2, SetupControlOutputMapping::Scalar),
        assignment(scalar, 7, SetupControlOutputMapping::Scalar),
        assignment(
            indexed,
            4,
            SetupControlOutputMapping::Indexed { entries: vec![] },
        ),
        assignment(
            indexed,
            4,
            SetupControlOutputMapping::Indexed {
                entries: vec![
                    SetupIndexedChannel { id: 0, value: 0 },
                    SetupIndexedChannel { id: 0, value: 200 },
                ],
            },
        ),
    ] {
        setup_edit(SetupGuiEdit::AssignControlOutput {
            assignment: bad,
            mode: SetupOutputAssignmentMode::Replace,
        });
        assert!(std::sync::Arc::ptr_eq(
            &before,
            &state.project_session().unwrap()
        ));
    }
    accepted(SetupGuiEdit::AssignControlOutput {
        assignment: assignment(scalar, 4, SetupControlOutputMapping::Scalar),
        mode: SetupOutputAssignmentMode::Replace,
    });
    let replaced = state.project_session().unwrap();
    assert_ne!(*before, *replaced);
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *before);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *replaced);
    accepted(SetupGuiEdit::AssignControlOutput {
        assignment: assignment(indexed, 6, indexed_mapping()),
        mode: SetupOutputAssignmentMode::Add,
    });
    state.open_file_path(sequence.0.document().as_str());
    for (target, value) in [
        (
            SequenceControlTarget::Scalar {
                node: scalar,
                cells: None,
            },
            SequenceControlValue::ConstantNormalized { value: 0.5 },
        ),
        (
            SequenceControlTarget::Indexed {
                node: indexed,
                cells: None,
            },
            SequenceControlValue::Indexed {
                option: 9,
                range_curve: None,
            },
        ),
    ] {
        let result = state.apply_gui_edit(
            GuiDocumentRequest {
                project_revision: state.snapshot().project_revision,
                path: sequence.0.document().to_string(),
                view: DocumentViewId::Sequence,
                object_key: Some(sequence.0.object().into()),
            },
            GuiEditCommand::Sequence {
                edit: SequenceGuiEdit::UpsertControlClip {
                    id: None,
                    start_seconds: 1.0,
                    duration_seconds: 2.0,
                    target,
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
    let final_session = state.project_session().unwrap();
    let output = dawn_elaboration::PreparedSequenceOutput::prepare(
        &final_session.project,
        &setup,
        &sequence,
    )
    .unwrap();
    for (time, expected) in [
        (0.0, [0; 8]),
        (1.5, [0, 0, 200, 200, 128, 128, 200, 200]),
        (3.0, [0; 8]),
    ] {
        assert_eq!(
            output.render_seconds(time).unwrap().controller_frames[0].slots,
            expected
        );
    }
    state.open_file_path(setup.0.document().as_str());
    let resize = |node, cells| ElementTreeGuiEdit::UpdateControlElement {
        id: node,
        name: if node == scalar {
            "Dimmers"
        } else {
            "Switches"
        }
        .into(),
        definition: if node == scalar {
            SetupControlElement::Scalar { cells }
        } else {
            SetupControlElement::Indexed {
                cells,
                options: vec![
                    SetupIndexedOption {
                        id: 0,
                        name: "Off".into(),
                    },
                    SetupIndexedOption {
                        id: 9,
                        name: "On".into(),
                    },
                ],
            }
        },
    };
    let patch_id = final_session.project.setups[&setup].patch.clone();
    let original_patch = &final_session.project.patches[&patch_id];
    accepted_elements(&state, resize(scalar, 1));
    accepted_elements(&state, resize(indexed, 1));
    let smaller = state.project_session().unwrap();
    let resized_patch = &smaller.project.patches[&patch_id];
    assert_eq!(
        original_patch.nodes.keys().collect::<Vec<_>>(),
        resized_patch.nodes.keys().collect::<Vec<_>>()
    );
    assert_eq!(original_patch.edges, resized_patch.edges);
    let output =
        dawn_elaboration::PreparedSequenceOutput::prepare(&smaller.project, &setup, &sequence)
            .unwrap();
    assert_eq!(
        output.render_seconds(1.5).unwrap().controller_frames[0].slots,
        [0, 0, 200, 0, 128, 0, 200, 0]
    );
    state.undo_active_edit();
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *smaller);
    accepted_elements(&state, resize(scalar, 2));
    accepted_elements(&state, resize(indexed, 2));
    let restored = state.project_session().unwrap();
    assert_eq!(restored.project, final_session.project);
    for invalid in [resize(scalar, 3), resize(indexed, 3), resize(scalar, 0)] {
        edit_elements(&state, invalid);
        assert!(std::sync::Arc::ptr_eq(
            &restored,
            &state.project_session().unwrap()
        ));
    }
    let final_session = state.project_session().unwrap();
    edit_elements(
        &state,
        ElementTreeGuiEdit::UpdateControlElement {
            id: indexed,
            name: "Switches".into(),
            definition: SetupControlElement::Indexed {
                cells: 2,
                options: vec![
                    SetupIndexedOption {
                        id: 0,
                        name: "Off".into(),
                    },
                    SetupIndexedOption {
                        id: 9,
                        name: "On".into(),
                    },
                    SetupIndexedOption {
                        id: 11,
                        name: "Blink".into(),
                    },
                ],
            },
        },
    );
    assert!(std::sync::Arc::ptr_eq(
        &final_session,
        &state.project_session().unwrap()
    ));
    for missing in [0, 9] {
        let mut invalid = final_session.project.clone();
        let entries = invalid
            .patches
            .get_mut(&patch_id)
            .unwrap()
            .nodes
            .values_mut()
            .find_map(|node| {
                if let dawn_language::patch::PatchNode::Filter(
                    dawn_language::patch::FilterDefinition::IndexedValueMapping { entries, .. },
                ) = node
                {
                    Some(entries)
                } else {
                    None
                }
            })
            .unwrap();
        entries.shift_remove(&missing);
        let error = dawn_language::validation::validate_project(&invalid)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(if missing == 0 { "ID 0" } else { "option 9" }),
            "{error}"
        );
    }
    let mut nonzero_options = (*final_session).clone();
    let tree_id = nonzero_options.project.setups[&setup].elements.clone();
    let dawn_language::element::ElementNodeKind::Indexed { options, .. } = &mut nonzero_options
        .project
        .element_trees
        .get_mut(&tree_id)
        .unwrap()
        .nodes
        .get_mut(&dawn_language::element::ElementNodeId(indexed))
        .unwrap()
        .kind
    else {
        panic!("indexed element missing")
    };
    options.retain(|option| option.id.0 != 0);
    dawn_language::validation::validate_project(&nonzero_options.project).unwrap();
    let copied_root = root.parent().unwrap().join("nonzero-options");
    dawn_project_io::export_editable_project(&nonzero_options, &copied_root).unwrap();
    let nonzero_options = dawn_project_io::load_package(&copied_root).unwrap().session;
    let output = dawn_elaboration::PreparedSequenceOutput::prepare(
        &nonzero_options.project,
        &setup,
        &sequence,
    )
    .unwrap();
    assert_eq!(
        output.render_seconds(0.0).unwrap().controller_frames[0].slots,
        [0; 8]
    );
    assert_eq!(
        output.render_seconds(1.5).unwrap().controller_frames[0].slots,
        [0, 0, 200, 200, 128, 128, 200, 200]
    );
    state.save_all().unwrap();
    let reloaded = dawn_project_io::load_package(&root).unwrap().session;
    assert_eq!(reloaded.project, final_session.project);
    let output =
        dawn_elaboration::PreparedSequenceOutput::prepare(&reloaded.project, &setup, &sequence)
            .unwrap();
    assert_eq!(
        output.render_seconds(1.5).unwrap().controller_frames[0].slots,
        [0, 0, 200, 200, 128, 128, 200, 200]
    );

    let GuiDocument::Setup { document } = state
        .get_gui_document(GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: setup.0.document().to_string(),
            view: DocumentViewId::Setup,
            object_key: Some(setup.0.object().into()),
        })
        .document
    else {
        panic!("setup projection missing")
    };
    let mut nodes = document.patch_definitions;
    let edges = document.patch_edges;
    let mut mappings = 0;
    for node in &mut nodes {
        if let PatchGuiNodeDefinition::Filter {
            filter: PatchGuiFilter::IndexedValueMapping { entries, .. },
        } = &mut node.definition
        {
            entries
                .iter_mut()
                .find(|entry| entry.id == 9)
                .unwrap()
                .value = 0.25;
            mappings += 1;
        }
    }
    assert_eq!(mappings, 2);
    let mut invalid = nodes.clone();
    for node in &mut invalid {
        if let PatchGuiNodeDefinition::Filter {
            filter: PatchGuiFilter::IndexedValueMapping { entries, .. },
        } = &mut node.definition
        {
            entries.push(entries[0].clone());
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
    accepted_patch(&state, nodes, edges);
    let advanced = state.project_session().unwrap();
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *final_session);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *advanced);
    let rendered =
        dawn_elaboration::PreparedSequenceOutput::prepare(&advanced.project, &setup, &sequence)
            .unwrap();
    assert_eq!(
        rendered.render_seconds(1.5).unwrap().controller_frames[0].slots,
        [0, 0, 64, 64, 128, 128, 64, 64]
    );
    state.save_all().unwrap();
    let reloaded = dawn_project_io::load_package(&root).unwrap().session;
    assert_eq!(reloaded.project, advanced.project);
    let reopened =
        dawn_elaboration::PreparedSequenceOutput::prepare(&reloaded.project, &setup, &sequence)
            .unwrap();
    for time in [0.0, 1.5, 3.0] {
        assert_eq!(
            reopened.render_seconds(time).unwrap().controller_frames,
            rendered.render_seconds(time).unwrap().controller_frames
        );
    }
}

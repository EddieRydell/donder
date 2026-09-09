use super::DesktopState;
use crate::dto::*;
use crate::project::{new_project_files, write_new_project_files};
use camino::Utf8PathBuf;

#[test]
fn advanced_scalar_patch_rejects_corrects_renders_and_roundtrips() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_project_files("Advanced patch").unwrap()).unwrap();
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
    let accepted = |edit| match setup_edit(edit).document {
        GuiDocument::Setup { document } => document,
        other => panic!("{other:?}"),
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
    .elements[0]
        .id;
    let controller = accepted(SetupGuiEdit::AddController {
        config: SetupControllerConfig::E131 {
            source_name: "Advanced patch".into(),
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
    let filter = |id, filter| PatchGuiNode {
        id,
        definition: PatchGuiNodeDefinition::Filter { filter },
    };
    let sink = |id, start_slot, slot_count| PatchGuiNode {
        id,
        definition: PatchGuiNodeDefinition::Sink {
            controller: controller.clone(),
            port: 1,
            start_slot,
            slot_count,
        },
    };
    let mut nodes = vec![
        PatchGuiNode {
            id: 1,
            definition: PatchGuiNodeDefinition::Source {
                tree: crate::gui::ResolvedGuiObject {
                    identity: initial.project.setups[&setup].elements.0.clone(),
                    kind: dawn_project_io::SourceObjectKind::ElementTree,
                }
                .source_ref(),
                node: scalar,
                cells: Some(PatchGuiCellRange { start: 1, count: 1 }),
                output: PatchGuiValueType::Scalar { width: 1 },
            },
        },
        filter(2, PatchGuiFilter::ScalarToComponents { width: 1 }),
        filter(
            3,
            PatchGuiFilter::DimmingCurve {
                width: 1,
                curve: GuiDimmingCurve::Gamma { exponent: 2.0 },
            },
        ),
        filter(
            4,
            PatchGuiFilter::ScaleInvert {
                width: 1,
                scale: 0.5,
                invert: true,
            },
        ),
        filter(
            5,
            PatchGuiFilter::FanOut {
                width: 1,
                outputs: 3,
            },
        ),
        filter(6, PatchGuiFilter::Quantize8 { width: 1 }),
        filter(
            7,
            PatchGuiFilter::Quantize16 {
                width: 1,
                byte_order: GuiByteOrder::CoarseFine,
            },
        ),
        filter(
            8,
            PatchGuiFilter::Quantize16 {
                width: 1,
                byte_order: GuiByteOrder::FineCoarse,
            },
        ),
        sink(9, 1, 1),
        sink(10, 2, 2),
        sink(11, 4, 2),
    ];
    let edges: Vec<_> = [
        (1, 0, 2),
        (2, 0, 3),
        (3, 0, 4),
        (4, 0, 5),
        (5, 0, 6),
        (5, 1, 7),
        (5, 2, 8),
        (6, 0, 9),
        (7, 0, 10),
        (8, 0, 11),
    ]
    .into_iter()
    .map(|(from_node, from_port, to_node)| SetupPatchEdge {
        from_node,
        from_port,
        to_node,
        to_port: 0,
    })
    .collect();
    let before = state.project_session().unwrap();
    let mut invalid = nodes.clone();
    invalid[6] = filter(
        7,
        PatchGuiFilter::Quantize16 {
            width: 2,
            byte_order: GuiByteOrder::CoarseFine,
        },
    );
    let rejected = replace_patch(&state, invalid, edges.clone());
    assert!(
        matches!(rejected.document, GuiDocument::Blocked { .. }),
        "{:?}",
        rejected.document
    );
    assert!(std::sync::Arc::ptr_eq(
        &before,
        &state.project_session().unwrap()
    ));
    let document = accepted_patch(&state, nodes.clone(), edges.clone());
    assert_eq!(
        serde_json::to_value(document.nodes).unwrap(),
        serde_json::to_value(&nodes).unwrap()
    );
    assert_eq!(
        serde_json::to_value(document.edges).unwrap(),
        serde_json::to_value(&edges).unwrap()
    );
    let after = state.project_session().unwrap();
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *before);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *after);
    state.open_file_path(sequence.0.document().as_str());
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
                target: SequenceControlTarget::Scalar {
                    node: scalar,
                    cells: None,
                },
                value: SequenceControlValue::ConstantNormalized { value: 0.5 },
            },
        },
    );
    assert!(
        matches!(result.document, GuiDocument::Sequence { .. }),
        "{:?}",
        result.document
    );
    let render = |session: &dawn_project_io::ProjectSession, time| {
        dawn_elaboration::PreparedSequenceOutput::prepare(&session.project, &setup, &sequence)
            .unwrap()
            .render_seconds(time)
            .unwrap()
            .controller_frames[0]
            .slots
            .clone()
    };
    // 0.5 squared, inverted, then halved is 0.375: 8-bit 96, 16-bit 0x6000.
    assert_eq!(
        render(&state.project_session().unwrap(), 1.5),
        [0, 96, 96, 0, 0, 96]
    );
    // An inactive scalar is zero; the authored inversion intentionally outputs half intensity.
    assert_eq!(
        render(&state.project_session().unwrap(), 0.0),
        [0, 128, 128, 0, 0, 128]
    );
    state.open_file_path(setup.0.document().as_str());
    nodes[2] = filter(
        3,
        PatchGuiFilter::DimmingCurve {
            width: 1,
            curve: GuiDimmingCurve::Custom {
                points: vec![
                    SequenceCurvePoint {
                        time: 0.0,
                        value: 0.0,
                    },
                    SequenceCurvePoint {
                        time: 0.5,
                        value: 0.75,
                    },
                    SequenceCurvePoint {
                        time: 1.0,
                        value: 1.0,
                    },
                ],
            },
        },
    );
    accepted_patch(&state, nodes, edges);
    let final_session = state.project_session().unwrap();
    assert_eq!(render(&final_session, 1.5), [0, 32, 32, 0, 0, 32]);
    state.save_all().unwrap();
    let reloaded = dawn_project_io::load_package(&root).unwrap().session;
    assert_eq!(reloaded.project, final_session.project);
    for time in [0.0, 1.5, 3.0] {
        assert_eq!(render(&reloaded, time), render(&final_session, time));
    }
}

pub(super) fn replace_patch(
    state: &DesktopState,
    nodes: Vec<PatchGuiNode>,
    edges: Vec<SetupPatchEdge>,
) -> GuiEditResult {
    let session = state.project_session().unwrap();
    let patch = &session.project.setups[&session.project.root.setup].patch;
    state.open_file_path(patch.0.document().as_str());
    state.apply_gui_edit(
        GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: patch.0.document().to_string(),
            view: DocumentViewId::Patch,
            object_key: Some(patch.0.object().into()),
        },
        GuiEditCommand::Patch { nodes, edges },
    )
}

pub(super) fn accepted_patch(
    state: &DesktopState,
    nodes: Vec<PatchGuiNode>,
    edges: Vec<SetupPatchEdge>,
) -> PatchGuiDocument {
    match replace_patch(state, nodes, edges).document {
        GuiDocument::Patch { document } => document,
        other => panic!("{other:?}"),
    }
}

pub(super) fn edit_elements(state: &DesktopState, edit: ElementTreeGuiEdit) -> GuiEditResult {
    let session = state.project_session().unwrap();
    let tree = &session.project.setups[&session.project.root.setup].elements;
    let request = state
        .resolve_gui_source(
            &tree.0.module_id().to_string(),
            tree.0.document().as_str(),
            tree.0.object(),
        )
        .unwrap();
    state.open_file_path(&request.path);
    state.apply_gui_edit(request, GuiEditCommand::ElementTree { edit })
}

pub(super) fn accepted_elements(
    state: &DesktopState,
    edit: ElementTreeGuiEdit,
) -> ElementTreeGuiDocument {
    match edit_elements(state, edit).document {
        GuiDocument::ElementTree { document } => document,
        other => panic!("{other:?}"),
    }
}

pub(super) fn edit_layout(state: &DesktopState, edit: PreviewGuiEdit) -> GuiEditResult {
    let session = state.project_session().unwrap();
    let layout = &session.project.setups[&session.project.root.setup].preview;
    let request = state
        .resolve_gui_source(
            &layout.0.module_id().to_string(),
            layout.0.document().as_str(),
            layout.0.object(),
        )
        .unwrap();
    state.open_file_path(&request.path);
    state.apply_gui_edit(request, GuiEditCommand::Preview { edit })
}

pub(super) fn accepted_layout(state: &DesktopState, edit: PreviewGuiEdit) -> PreviewGuiDocument {
    match edit_layout(state, edit).document {
        GuiDocument::Preview { document } => document,
        other => panic!("{other:?}"),
    }
}

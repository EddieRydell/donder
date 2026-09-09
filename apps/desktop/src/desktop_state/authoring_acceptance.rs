use super::DesktopState;
use super::advanced_patch_acceptance::accepted_elements;
use crate::dto::*;
use crate::project::{new_project_files, write_new_project_files};
use camino::Utf8PathBuf;

#[test]
fn empty_project_authors_two_props_outputs_effect_and_reopens_without_yaml_edits() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_project_files("Acceptance").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let initial = state.project_session().unwrap();
    let setup_id = initial.project.root.setup.clone();
    let sequence_id = initial.project.root.sequences[0].clone();
    let setup_request = || GuiDocumentRequest {
        project_revision: state.snapshot().project_revision,
        path: setup_id.0.document().to_string(),
        view: DocumentViewId::Setup,
        object_key: Some(setup_id.0.object().into()),
    };
    state.open_file_path(setup_id.0.document().as_str());
    let project_setup = |result: GuiDocument| match result {
        GuiDocument::Setup { document } => document,
        other => panic!("setup operation rejected: {other:?}"),
    };
    let setup_edit = |edit| {
        project_setup(
            state
                .apply_gui_edit(setup_request(), GuiEditCommand::Setup { edit })
                .document,
        )
    };
    let empty = project_setup(state.get_gui_document(setup_request()).document);
    assert!(empty.elements.is_empty());
    assert!(empty.controllers.is_empty());
    let point = |x, y| Point3Meters {
        x_meters: x,
        y_meters: y,
        z_meters: 0.0,
    };
    for (name, y) in [("Prop A", 0.0), ("Prop B", 1.0)] {
        super::advanced_patch_acceptance::accepted_layout(
            &state,
            PreviewGuiEdit::AddPixelLight {
                light: SetupPixelLight {
                    capability: GuiColorCapability::Rgb,
                    name: name.into(),
                    parent: None,
                    geometry: Geometry::Lines {
                        points: vec![point(0.0, 0.0), point(2.0, 0.0)],
                        pixels: 30,
                    },
                    bulb_diameter_meters: 0.04,
                    position: point(0.0, y),
                },
            },
        );
    }
    let fixture = project_setup(state.get_gui_document(setup_request()).document).preview_links[0]
        .definition_ref
        .clone();
    let edited_fixture = state.apply_gui_edit(
        GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: fixture.path.clone(),
            view: DocumentViewId::Prop,
            object_key: Some(fixture.object_key.clone()),
        },
        GuiEditCommand::Prop {
            edit: PropGuiEdit::UpdateDefinition {
                geometry: Geometry::Lines {
                    points: vec![point(0.0, 0.0), point(3.0, 0.0)],
                    pixels: 30,
                },
                bulb_diameter_meters: 0.05,
            },
        },
    );
    assert!(matches!(edited_fixture.document, GuiDocument::Prop { .. }));
    let before_controller = state.project_session().unwrap();
    let document = setup_edit(SetupGuiEdit::AddController {
        config: SetupControllerConfig::E131 {
            source_name: "Acceptance".into(),
            bind_address: "0.0.0.0".into(),
            priority: 100,
            destination: Some("127.0.0.1".into()),
        },
        ports: vec![SetupControllerPort {
            id: 1,
            address: 1,
            slot_count: 180,
        }],
    });
    assert_eq!(document.elements.len(), 2);
    assert_eq!(document.preview_links.len(), 2);
    assert!(
        document
            .preview_links
            .iter()
            .all(|prop| prop.bindings.len() == 30)
    );
    let with_controller = state.project_session().unwrap();
    let authored_setup = &with_controller.project.setups[&setup_id];
    assert_eq!(
        authored_setup.elements.0.document(),
        "layouts/main.layout.dawn"
    );
    assert_eq!(
        authored_setup.preview.0.document(),
        "layouts/main.layout.dawn"
    );
    assert!(
        with_controller.project.preview_layouts[&authored_setup.preview]
            .props
            .iter()
            .all(|prop| prop.definition.0.document().starts_with("fixtures/"))
    );
    assert!(
        authored_setup.controllers[0]
            .0
            .document()
            .starts_with("controllers/")
    );
    state.undo_active_edit();
    assert_eq!(
        state.project_session().unwrap().project,
        before_controller.project
    );
    state.redo_active_edit();
    assert_eq!(
        state.project_session().unwrap().project,
        with_controller.project
    );
    let controller = document.controllers[0].source_ref.clone();
    let nodes = document.root_ids;
    for (index, node) in nodes.iter().enumerate() {
        let document = setup_edit(SetupGuiEdit::AssignPixelOutput {
            node: *node,
            controller: controller.clone(),
            first_port: 1,
            start_slot: (index * 90) as u16,
            component_order: vec![0, 1, 2],
            mode: SetupOutputAssignmentMode::Add,
        });
        assert_eq!(document.output_assignments.len(), index + 1);
    }
    let before_overlap = state.project_session().unwrap();
    let rejected = state.apply_gui_edit(
        setup_request(),
        GuiEditCommand::Setup {
            edit: SetupGuiEdit::AssignPixelOutput {
                node: nodes[1],
                controller,
                first_port: 1,
                start_slot: 0,
                component_order: vec![0, 1, 2],
                mode: SetupOutputAssignmentMode::Add,
            },
        },
    );
    assert!(matches!(rejected.document, GuiDocument::Blocked { .. }));
    assert!(std::sync::Arc::ptr_eq(
        &before_overlap,
        &state.project_session().unwrap()
    ));

    let before_copy = state.project_session().unwrap();
    let original_setup = before_copy.project.setups[&setup_id].clone();
    let original_controller = original_setup.controllers[0].clone();
    let copy_document = setup_edit(SetupGuiEdit::CopyController {
        controller: document.controllers[0].source_ref.clone(),
    });
    assert_eq!(copy_document.output_assignments.len(), 2);
    assert!(!copy_document.controllers[0].read_only);
    let after_copy = state.project_session().unwrap();
    let copied_setup = &after_copy.project.setups[&setup_id];
    assert_ne!(copied_setup.controllers[0], original_controller);
    assert_ne!(copied_setup.patch, original_setup.patch);
    assert_eq!(
        after_copy.project.controllers[&original_controller],
        before_copy.project.controllers[&original_controller]
    );
    assert_eq!(
        after_copy.project.patches[&original_setup.patch],
        before_copy.project.patches[&original_setup.patch]
    );
    for node in after_copy.project.patches[&copied_setup.patch]
        .nodes
        .values()
    {
        if let dawn_language::patch::PatchNode::Sink(sink) = node {
            assert_eq!(sink.controller, copied_setup.controllers[0]);
        }
    }
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *before_copy);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *after_copy);

    state.open_file_path(sequence_id.0.document().as_str());
    let request = GuiDocumentRequest {
        project_revision: state.snapshot().project_revision,
        path: sequence_id.0.document().to_string(),
        view: DocumentViewId::Sequence,
        object_key: Some(sequence_id.0.object().into()),
    };
    let result = state.apply_gui_edit(
        request,
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::AddEffect {
                initial_color: initial.project.sequences[&sequence_id].layers[0]
                    .color
                    .to_hex(),
                effect: SequenceEffectReference::Builtin {
                    effect: SequenceBuiltinEffect::Pulse,
                },
                target: ElementTarget {
                    kind: ElementTargetKind::Element,
                    name: nodes[0].to_string(),
                },
                scope: SequenceEffectScope::WholeTarget,
                start_seconds: 0.0,
                mark_collection_key: None,
            },
        },
    );
    assert!(
        matches!(result.document, GuiDocument::Sequence { .. }),
        "{result:?}"
    );
    state.open_file_path(setup_id.0.document().as_str());
    let before_layout = state.project_session().unwrap();
    let old_layout = before_layout.project.setups[&setup_id].clone();
    setup_edit(SetupGuiEdit::CopyLayout);
    let final_session = state.project_session().unwrap();
    let copied_layout = &final_session.project.setups[&setup_id];
    assert_ne!(copied_layout.elements, old_layout.elements);
    assert_ne!(copied_layout.preview, old_layout.preview);
    assert_ne!(copied_layout.patch, old_layout.patch);
    assert_eq!(
        final_session.project.element_trees[&old_layout.elements],
        before_layout.project.element_trees[&old_layout.elements]
    );
    assert_eq!(
        final_session.project.preview_layouts[&old_layout.preview],
        before_layout.project.preview_layouts[&old_layout.preview]
    );
    assert_eq!(
        final_session.project.patches[&old_layout.patch],
        before_layout.project.patches[&old_layout.patch]
    );
    for (copied, original) in final_session.project.preview_layouts[&copied_layout.preview]
        .props
        .iter()
        .zip(&before_layout.project.preview_layouts[&old_layout.preview].props)
    {
        assert_eq!(copied.bindings, original.bindings);
        assert_eq!(copied.position, original.position);
        assert_ne!(copied.definition, original.definition);
        assert_eq!(
            final_session.project.definitions.props.definitions[&copied.definition],
            before_layout.project.definitions.props.definitions[&original.definition]
        );
    }
    assert_eq!(
        final_session.project.sequences[&sequence_id].effects[0]
            .target
            .tree,
        copied_layout.elements
    );
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *before_layout);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *final_session);
    assert!(matches!(
        final_session.project.sequences[&sequence_id].audio,
        dawn_language::sequence::SequenceAudio::None
    ));
    let prepared = dawn_elaboration::PreparedSequenceOutput::prepare(
        &final_session.project,
        &setup_id,
        &sequence_id,
    )
    .unwrap();
    let mut illuminated = false;
    for tick in 0..60 {
        let frame = prepared.render_seconds(tick as f32 / 60.0).unwrap();
        assert_eq!(frame.controller_frames.len(), 1);
        let slots = &frame.controller_frames[0].slots;
        assert_eq!(slots.len(), 180);
        illuminated |= slots[..90].iter().any(|value| *value != 0);
        assert!(slots[90..].iter().all(|value| *value == 0));
    }
    assert!(illuminated);
    state.save_all().unwrap();
    let reloaded = dawn_project_io::load_package(&root).unwrap().session;
    assert_eq!(reloaded.project, final_session.project);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let display_path = final_session.project.setups[&setup_id]
            .elements
            .0
            .document();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(root.join(display_path))
            .unwrap();
        state.open_file_path(setup_id.0.document().as_str());
        accepted_elements(
            &state,
            ElementTreeGuiEdit::RenameElement {
                id: nodes[0],
                name: "Renamed prop".into(),
            },
        );
        state.open_file_path(sequence_id.0.document().as_str());
        assert!(state.save_all().is_err());
        let failed = state.snapshot();
        assert!(!failed.active_buffer.unwrap().dirty);
        assert!(
            failed
                .pending_saves
                .iter()
                .any(|save| save.path == display_path.as_str()
                    && matches!(save.state, DocumentSaveState::Failed { .. }))
        );
        drop(held);
        state.save_all().unwrap();
        assert!(state.snapshot().pending_saves.is_empty());
        let reloaded = dawn_project_io::load_package(&root).unwrap().session;
        assert_eq!(reloaded.project, state.project_session().unwrap().project);
    }
}

#[test]
fn dependency_controller_and_layout_copies_preserve_package_files_and_reopen() {
    use std::collections::BTreeMap;
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_project_files("Dependency copy").unwrap()).unwrap();
    let starter = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let library = root.join("rig");
    let paths = [
        "layouts/outputs.layout.dawn",
        "patches/outputs.patch.dawn",
        "setups/main.setup.dawn",
        "fixtures/vertical.fixture.dawn",
    ];
    let mut originals = BTreeMap::new();
    for path in paths {
        let bytes = std::fs::read(starter.join(path)).unwrap();
        std::fs::create_dir_all(library.join(path).parent().unwrap()).unwrap();
        std::fs::write(library.join(path), &bytes).unwrap();
        originals.insert(path, bytes);
    }
    let mut manifest = dawn_package::PackageManifest::read(&starter).unwrap();
    manifest.module_id = uuid::Uuid::new_v4();
    manifest.project = None;
    manifest.assets.clear();
    manifest.exports = [
        ("layout", paths[0]),
        ("patch", paths[1]),
        ("controllers", paths[2]),
    ]
    .into_iter()
    .map(|(name, path)| {
        (
            name.into(),
            dawn_package::ExportGroup {
                documents: vec![path.into()],
            },
        )
    })
    .collect();
    manifest.write(&library).unwrap();
    let mut project_manifest = dawn_package::PackageManifest::read(&root).unwrap();
    project_manifest.dependencies.insert(
        "rig".into(),
        dawn_package::Dependency::Path { path: "rig".into() },
    );
    project_manifest.write(&root).unwrap();
    let registry = dawn_package::Lockfile::read(&root).unwrap().registry;
    dawn_package::Lockfile::from_directory(&project_manifest, &root, registry)
        .unwrap()
        .write(&root)
        .unwrap();
    std::fs::write(root.join("setups/main.setup.dawn"), "imports:\n- from: { dependency: rig, export: layout }\n  as: layout\n- from: { dependency: rig, export: patch }\n  as: patch\n- from: { dependency: rig, export: controllers }\n  as: controllers\nmain:\n  type: setup\n  elements: layout.outputs_elements\n  preview: layout.outputs_preview\n  patch: patch.outputs\n  controllers: [controllers.output_controller]\n").unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let original = state.project_session().unwrap();
    let setup_id = original.project.root.setup.clone();
    state.open_file_path(setup_id.0.document().as_str());
    let request = || GuiDocumentRequest {
        project_revision: state.snapshot().project_revision,
        path: setup_id.0.document().to_string(),
        view: DocumentViewId::Setup,
        object_key: Some(setup_id.0.object().to_string()),
    };
    let GuiDocument::Setup { document } = state.get_gui_document(request()).document else {
        panic!("setup missing")
    };
    assert!(document.elements_read_only && document.preview_read_only && document.patch_read_only);
    assert!(document.controllers[0].read_only);
    for edit in [
        SetupGuiEdit::CopyController {
            controller: document.controllers[0].source_ref.clone(),
        },
        SetupGuiEdit::CopyLayout,
    ] {
        let result = state.apply_gui_edit(request(), GuiEditCommand::Setup { edit });
        assert!(
            matches!(result.document, GuiDocument::Setup { .. }),
            "{:?}",
            result.document
        );
    }
    let copied = state.project_session().unwrap();
    let GuiDocument::Setup { document } = state.get_gui_document(request()).document else {
        panic!("setup missing")
    };
    assert!(
        !document.elements_read_only && !document.preview_read_only && !document.patch_read_only
    );
    assert!(!document.controllers[0].read_only);
    assert_eq!(document.output_assignments.len(), 30);
    let copied_setup = &copied.project.setups[&setup_id];
    let copied_props = &copied.project.preview_layouts[&copied_setup.preview].props;
    assert_eq!(copied_props.len(), 30);
    assert!(
        copied_props
            .iter()
            .all(|prop| prop.definition == copied_props[0].definition)
    );
    assert!(
        copied
            .source
            .is_project_owned(copied_props[0].definition.0.document_id())
    );
    let imported_request = GuiDocumentRequest {
        path: "rig/setups/main.setup.dawn".into(),
        ..request()
    };
    assert!(matches!(
        state.get_gui_document(imported_request).document,
        GuiDocument::Setup { .. }
    ));
    let old_setup = &original.project.setups[&setup_id];
    assert_eq!(
        copied.project.element_trees[&old_setup.elements],
        original.project.element_trees[&old_setup.elements]
    );
    assert_eq!(
        copied.project.preview_layouts[&old_setup.preview],
        original.project.preview_layouts[&old_setup.preview]
    );
    assert_eq!(
        copied.project.patches[&old_setup.patch],
        original.project.patches[&old_setup.patch]
    );
    state.undo_active_edit();
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *original);
    state.redo_active_edit();
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *copied);
    super::advanced_patch_acceptance::accepted_elements(
        &state,
        ElementTreeGuiEdit::RenameElement {
            id: 1,
            name: "My first output".into(),
        },
    );
    state.save_all().unwrap();
    let saved = state.project_session().unwrap();
    assert_eq!(
        dawn_project_io::load_package(&root)
            .unwrap()
            .session
            .project,
        saved.project
    );
    for (path, bytes) in originals {
        assert_eq!(std::fs::read(library.join(path)).unwrap(), bytes);
    }
}

use super::DesktopState;
use crate::dto::*;
use crate::project::{new_test_project_files, write_new_project_files};
use camino::Utf8PathBuf;

pub(super) fn edit_layout(state: &DesktopState, edit: LayoutGuiEdit) -> GuiEditResult {
    let session = state.project_session().unwrap();
    let layout = session
        .project
        .setup(session.project.root.setup.id())
        .unwrap()
        .layout
        .id()
        .clone();
    state.open_file_path(layout.0.document().as_str());
    state.apply_gui_edit(
        GuiDocumentRequest {
            owned_path: layout.0.owned_path().iter().map(Into::into).collect(),
            project_revision: state.snapshot().project_revision,
            path: layout.0.document().to_string(),
            view: DocumentViewId::Layout,
            object_key: Some(layout.0.root_source().object().to_string()),
        },
        GuiEditCommand::Layout { edit },
    )
}

fn layout_document(result: GuiDocument) -> LayoutGuiDocument {
    match result {
        GuiDocument::Layout { document } => document,
        other => panic!("layout edit rejected: {other:?}"),
    }
}

fn fixture_reference(layout: &LayoutGuiDocument, index: usize) -> GuiObjectRef {
    let fixture = &layout.fixtures[index];
    let GuiLayoutFixtureKind::Fixture { definition, .. } = &fixture.kind else {
        panic!("fixture missing")
    };
    match definition {
        GuiFixtureSource::Reference { source } => source.clone(),
        GuiFixtureSource::Inline { .. } => {
            let mut owner = layout.source_ref.clone();
            owner.kind = ObjectKind::Fixture;
            owner
                .owned_path
                .push(GuiOwnedStep::Fixture { id: fixture.id });
            owner
        }
    }
}

fn fixture_edit(
    state: &DesktopState,
    reference: &GuiObjectRef,
    edit: FixtureGuiEdit,
) -> GuiDocument {
    state.open_file_path(&reference.path);
    state
        .apply_gui_edit(
            GuiDocumentRequest {
                owned_path: reference.owned_path.clone(),
                project_revision: state.snapshot().project_revision,
                path: reference.path.clone(),
                object_key: Some(reference.object_key.clone()),
                view: DocumentViewId::Fixture,
            },
            GuiEditCommand::Fixture { edit },
        )
        .document
}

fn setup_edit(state: &DesktopState, edit: SetupGuiEdit) -> SetupGuiDocument {
    setup_command(state, GuiEditCommand::Setup { edit })
}

fn setup_command(state: &DesktopState, command: GuiEditCommand) -> SetupGuiDocument {
    let session = state.project_session().unwrap();
    let id = session.project.root.setup.id();
    state.open_file_path(id.0.document().as_str());
    match state
        .apply_gui_edit(
            GuiDocumentRequest {
                owned_path: id.0.owned_path().iter().map(Into::into).collect(),
                project_revision: state.snapshot().project_revision,
                path: id.0.document().to_string(),
                object_key: Some(id.0.root_source().object().into()),
                view: DocumentViewId::Setup,
            },
            command,
        )
        .document
    {
        GuiDocument::Setup { document } => document,
        other => panic!("setup edit rejected: {other:?}"),
    }
}

fn transform(x: f32) -> Transform {
    Transform {
        position: Point3Meters {
            x_meters: x,
            y_meters: 0.0,
            z_meters: 0.0,
        },
        rotation: Rotation3Degrees {
            x_degrees: 0.0,
            y_degrees: 0.0,
            z_degrees: 0.0,
        },
        scale: Scale3 {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
    }
}

#[test]
fn same_file_references_share_geometry_and_survive_placement_removal() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Composition").unwrap()).unwrap();
    std::fs::create_dir_all(root.join("layouts")).unwrap();
    std::fs::write(root.join("project.donder"), "imports:\n- from: { documents: [layouts/main.layout.donder] }\n  as: layout\nshow:\n  type: project\n  setup:\n    type: setup\n    layout: layout.main\n    patch: { type: patch, routes: [] }\n    controllers: []\n  sequences: []\n").unwrap();
    std::fs::write(
        root.join("layouts/main.layout.donder"),
        r#"
assembly:
  type: fixture
  elements:
  - id: 11
    name: Pixel
    reverse: false
    shape: {type: pixel}
    diameter: 0.01
    transform: {position: { x: 1, y: 0, z: 0 }}
main:
  type: layout
  fixtures:
  - id: 1
    name: A
    type: fixture
    definition: assembly
  - id: 2
    name: B
    type: fixture
    definition: assembly
    transform:
      position: { x: 10, y: 0, z: 0 }
"#,
    )
    .unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let layout_request = || GuiDocumentRequest {
        owned_path: Vec::new(),
        project_revision: state.snapshot().project_revision,
        path: "layouts/main.layout.donder".into(),
        object_key: Some("main".into()),
        view: DocumentViewId::Layout,
    };
    state.open_file_path("layouts/main.layout.donder");
    let layout = layout_document(state.get_gui_document(layout_request()).document);
    let assembly = &fixture_reference(&layout, 0);
    assert_eq!(assembly.path, layout.path);
    assert!(matches!(
        fixture_edit(
            &state,
            assembly,
            FixtureGuiEdit::MoveElement {
                id: 11,
                delta: Point3Meters {
                    x_meters: 3.0,
                    y_meters: 4.0,
                    z_meters: 0.0
                },
            }
        ),
        GuiDocument::Fixture { .. }
    ));
    state.open_file_path("layouts/main.layout.donder");
    let layout = layout_document(state.get_gui_document(layout_request()).document);
    assert_eq!(
        layout
            .render_plan
            .pixels
            .iter()
            .map(|pixel| (
                pixel.owner,
                pixel.index,
                pixel.position.x_meters,
                pixel.position.y_meters
            ))
            .collect::<Vec<_>>(),
        [(1, 0, 4.0, 4.0), (2, 0, 14.0, 4.0),]
    );
    let accepted = state.project_session().unwrap();
    state.save_all().unwrap();
    let reloaded = donder_project_io::load_project(&root).unwrap();
    assert_eq!(reloaded.project, accepted.project);
    assert_eq!(
        reloaded.source.documents.len(),
        accepted.source.documents.len()
    );
    layout_document(edit_layout(&state, LayoutGuiEdit::SetFixtures { fixtures: vec![] }).document);
    assert_eq!(
        state
            .project_session()
            .unwrap()
            .project
            .definitions
            .fixtures
            .definitions
            .len(),
        1
    );
    state.save_all().unwrap();
    assert_eq!(
        donder_project_io::load_project(&root).unwrap().project,
        state.project_session().unwrap().project
    );
}

#[test]
fn empty_project_authors_shared_fixtures_routes_effect_and_reopens_without_yaml_edits() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Acceptance").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let empty_layout = setup_command(
        &state,
        GuiEditCommand::Ownership {
            slot: GuiOwnershipSlot::Layout,
            edit: GuiOwnershipEdit::MakeReusable {
                name: "Empty stage".into(),
                storage: ReusableStorage::SameFile,
            },
        },
    )
    .layout_ref;
    setup_command(
        &state,
        GuiEditCommand::Ownership {
            slot: GuiOwnershipSlot::Layout,
            edit: GuiOwnershipEdit::MakeIndependent,
        },
    );
    let initial = state.project_session().unwrap();
    let setup_id = initial.project.root.setup.id().clone();
    let sequence_id = initial.project.root.sequences[0].id().clone();
    let initial_color = initial.project.sequence(&sequence_id).unwrap().layers[0]
        .color
        .to_hex();
    let layout = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::AddDefinition {
                storage: FixtureStorage::NewFile,
                name: "Strip".into(),
                parent: None,
                transform: transform(0.0),
            },
        )
        .document,
    );
    assert_eq!(layout.fixtures.len(), 1);
    assert!(layout.render_plan.pixels.is_empty());
    let definition = fixture_reference(&layout, 0);
    assert_eq!(definition.path, "fixtures/strip.fixture.donder");
    let elements = (1..=3)
        .map(|id| GuiFixtureElement {
            id,
            name: format!("Pixel {id}"),
            reverse: false,
            shape: GuiFixtureShape::Pixel,
            transform: transform((id - 1) as f32),
            diameter_meters: 0.01,
        })
        .collect::<Vec<_>>();
    let GuiDocument::Fixture { document } = fixture_edit(
        &state,
        &definition,
        FixtureGuiEdit::SetElements {
            elements: elements.clone(),
        },
    ) else {
        panic!("definition edit failed")
    };
    assert_eq!(document.render_plan.pixels.len(), 3);
    let mut second = layout.fixtures[0].clone();
    second.id = 2;
    second.name = "Second strip".into();
    let GuiLayoutFixtureKind::Fixture {
        transform: position,
        ..
    } = &mut second.kind
    else {
        unreachable!()
    };
    *position = transform(10.0);
    let grouped = vec![GuiLayoutFixture {
        id: 100,
        name: "Both strips".into(),
        kind: GuiLayoutFixtureKind::Group {
            children: vec![layout.fixtures[0].clone(), second],
        },
    }];
    let layout = layout_document(
        edit_layout(&state, LayoutGuiEdit::SetFixtures { fixtures: grouped }).document,
    );
    assert_eq!(
        layout
            .render_plan
            .pixels
            .iter()
            .map(|pixel| pixel.position.x_meters)
            .collect::<Vec<_>>(),
        [0.0, 1.0, 2.0, 10.0, 11.0, 12.0]
    );
    let setup = setup_edit(
        &state,
        SetupGuiEdit::AddController {
            config: SetupControllerConfig::E131 {
                source_name: "Test".into(),
                bind_address: "0.0.0.0".into(),
                priority: 100,
                destination: None,
            },
            ports: vec![SetupControllerPort {
                id: 1,
                address: 1,
                slot_count: 21,
            }],
        },
    );
    let routes = vec![
        GuiPixelRoute {
            id: 1,
            layout: setup.layout_ref.clone(),
            fixture: 1,
            pixels: None,
            controller: setup.controllers[0].source_ref.clone(),
            port: 1,
            start_slot: 0,
            encoding: GuiPixelEncoding::Rgbw {
                order: [1, 0, 2, 3],
            },
            gamma: 1.0,
            brightness: 1.0,
        },
        GuiPixelRoute {
            id: 2,
            layout: setup.layout_ref.clone(),
            fixture: 2,
            pixels: None,
            controller: setup.controllers[0].source_ref.clone(),
            port: 1,
            start_slot: 12,
            encoding: GuiPixelEncoding::Rgb { order: [1, 0, 2] },
            gamma: 1.0,
            brightness: 1.0,
        },
    ];
    state.open_file_path(&setup.patch_ref.path);
    let result = state.apply_gui_edit(
        GuiDocumentRequest {
            owned_path: setup.patch_ref.owned_path.clone(),
            project_revision: state.snapshot().project_revision,
            path: setup.patch_ref.path.clone(),
            object_key: Some(setup.patch_ref.object_key.clone()),
            view: DocumentViewId::Patch,
        },
        GuiEditCommand::Patch { routes },
    );
    assert!(
        matches!(result.document, GuiDocument::Patch { .. }),
        "{:?}",
        result.document
    );
    state.open_file_path(sequence_id.0.document().as_str());
    let result = state.apply_gui_edit(
        GuiDocumentRequest {
            owned_path: sequence_id.0.owned_path().iter().map(Into::into).collect(),
            project_revision: state.snapshot().project_revision,
            path: sequence_id.0.document().to_string(),
            object_key: Some(sequence_id.0.root_source().object().into()),
            view: DocumentViewId::Sequence,
        },
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::AddEffect {
                initial_color,
                effect: SequenceEffectReference::Builtin {
                    effect: SequenceBuiltinEffect::Pulse,
                },
                target: FixtureTarget { fixture: 1 },
                scope: SequenceEffectScope::PerFixture,
                start_seconds: 0.0,
                mark_collection_key: None,
            },
        },
    );
    assert!(
        matches!(result.document, GuiDocument::Sequence { .. }),
        "{:?}",
        result.document
    );
    let before_invalid = state.project_session().unwrap();
    state.open_file_path(setup_id.0.document().as_str());
    let rejected_link = state.apply_gui_edit(
        GuiDocumentRequest {
            owned_path: setup_id.0.owned_path().iter().map(Into::into).collect(),
            project_revision: state.snapshot().project_revision,
            path: setup_id.0.document().to_string(),
            object_key: Some(setup_id.0.root_source().object().into()),
            view: DocumentViewId::Setup,
        },
        GuiEditCommand::Ownership {
            slot: GuiOwnershipSlot::Layout,
            edit: GuiOwnershipEdit::UseExisting {
                source: empty_layout,
            },
        },
    );
    assert!(matches!(
        rejected_link.document,
        GuiDocument::Blocked { .. }
    ));
    assert!(std::sync::Arc::ptr_eq(
        &before_invalid,
        &state.project_session().unwrap()
    ));

    let result = fixture_edit(
        &state,
        &definition,
        FixtureGuiEdit::SetElements {
            elements: vec![GuiFixtureElement {
                id: 1,
                name: "Invalid".into(),
                reverse: false,
                shape: GuiFixtureShape::Pixel,
                transform: transform(0.0),
                diameter_meters: 0.0,
            }],
        },
    );
    assert!(matches!(result, GuiDocument::Blocked { .. }));
    assert!(std::sync::Arc::ptr_eq(
        &before_invalid,
        &state.project_session().unwrap()
    ));
    let mut reordered = elements;
    reordered.reverse();
    let GuiDocument::Fixture { document } = fixture_edit(
        &state,
        &definition,
        FixtureGuiEdit::SetElements {
            elements: reordered,
        },
    ) else {
        panic!("reorder failed")
    };
    assert_eq!(
        document
            .render_plan
            .pixels
            .iter()
            .map(|pixel| (pixel.owner, pixel.index, pixel.position.x_meters))
            .collect::<Vec<_>>(),
        [(3, 0, 2.0), (2, 1, 1.0), (1, 2, 0.0)]
    );
    setup_command(
        &state,
        GuiEditCommand::Ownership {
            slot: GuiOwnershipSlot::Layout,
            edit: GuiOwnershipEdit::MakeReusable {
                name: "Original layout".into(),
                storage: ReusableStorage::SameFile,
            },
        },
    );
    let before_copy = state.project_session().unwrap();
    setup_command(
        &state,
        GuiEditCommand::Ownership {
            slot: GuiOwnershipSlot::Layout,
            edit: GuiOwnershipEdit::MakeIndependent,
        },
    );
    let final_session = state.project_session().unwrap();
    let copied_layout = final_session.project.setup(&setup_id).unwrap().layout.id();
    assert_ne!(
        *copied_layout,
        *before_copy.project.setup(&setup_id).unwrap().layout.id()
    );
    assert_eq!(
        final_session.project.definitions.fixtures,
        before_copy.project.definitions.fixtures
    );
    assert_eq!(
        final_session
            .project
            .sequence(final_session.project.root.sequences[0].id())
            .unwrap()
            .effects[0]
            .target
            .layout,
        *copied_layout
    );
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *before_copy);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *final_session);
    let prepared = donder_elaboration::PreparedSequenceOutput::prepare(
        &final_session.project,
        &setup_id,
        final_session.project.root.sequences[0].id(),
    )
    .unwrap();
    let mut illuminated = false;
    for frame in 0..60 {
        let rendered = prepared.render_seconds(frame as f32 / 60.0).unwrap();
        let slots = &rendered.controller_frames[0].slots;
        illuminated |= slots[..12].iter().any(|&value| value != 0);
        assert!(slots[12..].iter().all(|&value| value == 0));
    }
    assert!(illuminated);
    state.save_all().unwrap();
    assert_eq!(
        donder_project_io::load_project(&root).unwrap().project,
        final_session.project
    );
}

#[test]
fn local_controller_and_layout_copies_preserve_shared_files_and_reopen() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Local copy").unwrap()).unwrap();
    let starter = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let library = root.join("rig");
    let paths = [
        "layouts/outputs.layout.donder",
        "patches/outputs.patch.donder",
        "setups/main.setup.donder",
        "fixtures/vertical.fixture.donder",
    ];
    for path in paths {
        let text = std::fs::read_to_string(starter.join(path)).unwrap();
        let bytes = paths
            .iter()
            .fold(text, |text, path| {
                text.replace(&format!("- {path}"), &format!("- rig/{path}"))
            })
            .into_bytes();
        std::fs::create_dir_all(library.join(path).parent().unwrap()).unwrap();
        std::fs::write(library.join(path), &bytes).unwrap();
    }
    std::fs::create_dir_all(root.join("setups")).unwrap();
    std::fs::write(root.join("project.donder"), "imports:\n- from: { documents: [setups/main.setup.donder] }\n  as: setups\nshow:\n  type: project\n  setup: setups.main\n  sequences: []\n").unwrap();
    std::fs::write(root.join("setups/main.setup.donder"), "imports:\n- from: { documents: [rig/layouts/outputs.layout.donder] }\n  as: layout\n- from: { documents: [rig/patches/outputs.patch.donder] }\n  as: patch\n- from: { documents: [rig/setups/main.setup.donder] }\n  as: controllers\nmain:\n  type: setup\n  layout: layout.outputs_layout\n  patch: patch.outputs\n  controllers: [controllers.output_controller]\n").unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let original = state.project_session().unwrap();
    let setup_id = original.project.root.setup.id().clone();
    state.open_file_path(setup_id.0.document().as_str());
    let request = || GuiDocumentRequest {
        owned_path: Vec::new(),
        project_revision: state.snapshot().project_revision,
        path: setup_id.0.document().to_string(),
        view: DocumentViewId::Setup,
        object_key: Some(setup_id.0.root_source().object().to_string()),
    };
    let GuiDocument::Setup { document } = state.get_gui_document(request()).document else {
        panic!("setup missing")
    };
    assert!(!document.layout_read_only && !document.patch_read_only);
    assert!(!document.controllers[0].read_only);
    for slot in [
        GuiOwnershipSlot::Controller { index: 0 },
        GuiOwnershipSlot::Layout,
    ] {
        let result = state.apply_gui_edit(
            request(),
            GuiEditCommand::Ownership {
                slot,
                edit: GuiOwnershipEdit::MakeIndependent,
            },
        );
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
    assert!(!document.layout_read_only && !document.patch_read_only);
    assert!(!document.controllers[0].read_only);
    let copied_setup = &copied.project.setups[&setup_id];
    assert_eq!(
        copied
            .project
            .patch(copied_setup.patch.id())
            .unwrap()
            .routes
            .len(),
        30
    );
    assert_eq!(
        copied
            .project
            .layout(copied_setup.layout.id())
            .unwrap()
            .iter_fixtures()
            .count(),
        31
    );
    assert_eq!(
        copied.project.definitions.fixtures,
        original.project.definitions.fixtures
    );
    let imported_request = GuiDocumentRequest {
        owned_path: Vec::new(),
        path: "rig/setups/main.setup.donder".into(),
        ..request()
    };
    assert!(matches!(
        state.get_gui_document(imported_request).document,
        GuiDocument::Setup { .. }
    ));
    let old_setup = &original.project.setups[&setup_id];
    assert_eq!(
        copied.project.layouts[old_setup.layout.id()],
        original.project.layouts[old_setup.layout.id()]
    );
    assert_eq!(
        copied.project.patches[old_setup.patch.id()],
        original.project.patches[old_setup.patch.id()]
    );
    state.undo_active_edit();
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *original);
    state.redo_active_edit();
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *copied);
    let request = GuiDocumentRequest {
        owned_path: copied_setup
            .layout
            .id()
            .0
            .owned_path()
            .iter()
            .map(Into::into)
            .collect(),
        project_revision: state.snapshot().project_revision,
        path: copied_setup.layout.id().0.document().to_string(),
        object_key: Some(copied_setup.layout.id().0.root_source().object().into()),
        view: DocumentViewId::Layout,
    };
    let mut layout = layout_document(state.get_gui_document(request).document);
    layout.fixtures[0].name = "My outputs".into();
    layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::SetFixtures {
                fixtures: layout.fixtures,
            },
        )
        .document,
    );
    state.save_all().unwrap();
    let saved = state.project_session().unwrap();
    assert_eq!(
        donder_project_io::load_project(&root).unwrap().project,
        saved.project
    );
    for path in paths {
        let document = original
            .source
            .document_for_workspace_path(&Utf8PathBuf::from(format!("rig/{path}")));
        let document = document.unwrap();
        assert_eq!(
            donder_project_io::source_document_text(&saved, &document).unwrap(),
            donder_project_io::source_document_text(&original, &document).unwrap(),
        );
    }
}

#[test]
fn shape_handles_conversion_and_undo_preserve_output_order() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Shapes").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let layout = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::AddDefinition {
                storage: FixtureStorage::Inline,
                name: "Shapes".into(),
                parent: None,
                transform: transform(0.0),
            },
        )
        .document,
    );
    let definition = &fixture_reference(&layout, 0);
    let element = GuiFixtureElement {
        id: 1,
        name: "Line".into(),
        transform: transform(1.0),
        diameter_meters: 0.01,
        reverse: true,
        shape: GuiFixtureShape::Line {
            length: 2.0,
            count: 3,
        },
    };
    assert!(matches!(
        fixture_edit(
            &state,
            definition,
            FixtureGuiEdit::SetElements {
                elements: vec![element]
            }
        ),
        GuiDocument::Fixture { .. }
    ));
    let GuiDocument::Fixture { document: moved } = fixture_edit(
        &state,
        definition,
        FixtureGuiEdit::MoveHandle {
            id: 1,
            index: 1,
            position: Point3Meters {
                x_meters: 1.0,
                y_meters: 4.0,
                z_meters: 0.0,
            },
        },
    ) else {
        panic!("handle edit failed")
    };
    let positions = |document: &FixtureGuiDocument| {
        document
            .render_plan
            .pixels
            .iter()
            .map(|pixel| (pixel.position.x_meters, pixel.position.y_meters))
            .collect::<Vec<_>>()
    };
    for ((x, y), (expected_x, expected_y)) in
        positions(&moved)
            .into_iter()
            .zip([(1.0, 4.0), (1.0, 2.0), (1.0, 0.0)])
    {
        assert!((x - expected_x).abs() < 0.00001 && (y - expected_y).abs() < 0.00001);
    }
    let before_conversion = state.project_session().unwrap();
    let GuiDocument::Fixture {
        document: converted,
    } = fixture_edit(
        &state,
        definition,
        FixtureGuiEdit::ConvertToPixels { id: 1 },
    )
    else {
        panic!("conversion failed")
    };
    assert_eq!(converted.elements.len(), 3);
    assert!(
        converted
            .elements
            .iter()
            .all(|element| matches!(element.shape, GuiFixtureShape::Pixel))
    );
    for ((x, y), (expected_x, expected_y)) in
        positions(&converted).into_iter().zip(positions(&moved))
    {
        assert!((x - expected_x).abs() < 0.00001 && (y - expected_y).abs() < 0.00001);
    }
    let after_conversion = state.project_session().unwrap();
    state.undo_active_edit();
    assert_eq!(
        state.project_session().unwrap().project,
        before_conversion.project
    );
    state.redo_active_edit();
    assert_eq!(
        state.project_session().unwrap().project,
        after_conversion.project
    );
    state.save_all().unwrap();
    assert_eq!(
        donder_project_io::load_project(&root).unwrap().project,
        after_conversion.project
    );
}

#[test]
fn fixture_storage_and_removal_preserve_shared_data_and_undo() {
    for storage in [
        FixtureStorage::Inline,
        FixtureStorage::SameFile,
        FixtureStorage::NewFile,
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
        write_new_project_files(&root, &new_test_project_files("Storage").unwrap()).unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let mut settings = state.snapshot().settings;
        settings.autosave_project_edits = false;
        state.update_app_settings(settings);
        let layout = layout_document(
            edit_layout(
                &state,
                LayoutGuiEdit::AddDefinition {
                    name: "My Strip".into(),
                    storage,
                    parent: None,
                    transform: transform(0.0),
                },
            )
            .document,
        );
        let definition = fixture_reference(&layout, 0);
        assert_eq!(
            definition.path.replace('\\', "/"),
            match storage {
                FixtureStorage::Inline | FixtureStorage::SameFile => "project.donder",
                FixtureStorage::NewFile => "fixtures/my_strip.fixture.donder",
            }
        );
        state.save_all().unwrap();
        assert_eq!(
            donder_project_io::load_project(&root).unwrap().project,
            state.project_session().unwrap().project
        );
        let mut second = layout.fixtures[0].clone();
        second.id = 2;
        second.name = "Shared placement".into();
        let group = GuiLayoutFixture {
            id: 3,
            name: "Group".into(),
            kind: GuiLayoutFixtureKind::Group {
                children: vec![second],
            },
        };
        layout_document(
            edit_layout(
                &state,
                LayoutGuiEdit::SetFixtures {
                    fixtures: vec![layout.fixtures[0].clone(), group.clone()],
                },
            )
            .document,
        );
        layout_document(
            edit_layout(
                &state,
                LayoutGuiEdit::SetFixtures {
                    fixtures: vec![group],
                },
            )
            .document,
        );
        assert_eq!(
            state
                .project_session()
                .unwrap()
                .project
                .definitions
                .fixtures
                .definitions
                .len(),
            usize::from(!matches!(storage, FixtureStorage::Inline))
        );
        let before = state.project_session().unwrap();
        let removed = layout_document(
            edit_layout(&state, LayoutGuiEdit::SetFixtures { fixtures: vec![] }).document,
        );
        assert!(removed.fixtures.is_empty());
        let after = state.project_session().unwrap();
        assert_eq!(
            after.project.definitions.fixtures.definitions.len(),
            usize::from(!matches!(storage, FixtureStorage::Inline))
        );
        state.undo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *before);
        state.redo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *after);
        state.save_all().unwrap();
        assert_eq!(
            donder_project_io::load_project(&root).unwrap().project,
            after.project
        );
        let text = std::fs::read_to_string(root.join("project.donder")).unwrap();
        assert!(!text.contains("fixture_1:"));
        if matches!(storage, FixtureStorage::NewFile) {
            assert!(root.join("fixtures/my_strip.fixture.donder").exists());
        }
    }
}

#[test]
fn inline_fixture_copies_have_independent_ownership() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Shared").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let original = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::AddDefinition {
                name: "Shared".into(),
                storage: FixtureStorage::Inline,
                parent: None,
                transform: transform(0.0),
            },
        )
        .document,
    );
    let reference = fixture_reference(&original, 0);
    assert!(matches!(
        fixture_edit(
            &state,
            &reference,
            FixtureGuiEdit::SetElements {
                elements: vec![GuiFixtureElement {
                    id: 1,
                    name: "Owned pixel".into(),
                    transform: transform(2.0),
                    diameter_meters: 0.01,
                    reverse: false,
                    shape: GuiFixtureShape::Pixel,
                }]
            }
        ),
        GuiDocument::Fixture { .. }
    ));
    setup_command(
        &state,
        GuiEditCommand::Ownership {
            slot: GuiOwnershipSlot::Layout,
            edit: GuiOwnershipEdit::MakeReusable {
                name: "Original layout".into(),
                storage: ReusableStorage::NewFile,
            },
        },
    );
    let original_layout = state
        .project_session()
        .unwrap()
        .project
        .setup(state.project_session().unwrap().project.root.setup.id())
        .unwrap()
        .layout
        .id()
        .clone();
    setup_command(
        &state,
        GuiEditCommand::Ownership {
            slot: GuiOwnershipSlot::Layout,
            edit: GuiOwnershipEdit::MakeIndependent,
        },
    );
    let copied = state.project_session().unwrap();
    let copied_layout = copied
        .project
        .layout(
            copied
                .project
                .setup(copied.project.root.setup.id())
                .unwrap()
                .layout
                .id(),
        )
        .unwrap();
    assert!(
        matches!(&copied_layout.fixtures[0].kind, donder_language::layout::LayoutFixtureKind::Fixture {definition: donder_language::fixture::FixtureSource::Inline(value),..} if value.elements.len() == 1)
    );
    state.open_file_path(original_layout.0.document().as_str());
    layout_document(
        state
            .apply_gui_edit(
                GuiDocumentRequest {
                    owned_path: Vec::new(),
                    project_revision: state.snapshot().project_revision,
                    path: original_layout.0.document().to_string(),
                    object_key: Some(original_layout.0.root_source().object().into()),
                    view: DocumentViewId::Layout,
                },
                GuiEditCommand::Layout {
                    edit: LayoutGuiEdit::SetFixtures { fixtures: vec![] },
                },
            )
            .document,
    );
    assert_eq!(
        state
            .project_session()
            .unwrap()
            .project
            .definitions
            .fixtures
            .definitions
            .len(),
        0
    );
    layout_document(edit_layout(&state, LayoutGuiEdit::SetFixtures { fixtures: vec![] }).document);
    assert!(
        state
            .project_session()
            .unwrap()
            .project
            .definitions
            .fixtures
            .definitions
            .is_empty()
    );
    state.save_all().unwrap();
    assert_eq!(
        donder_project_io::load_project(&root).unwrap().project,
        state.project_session().unwrap().project
    );
}

#[test]
fn nested_layout_and_fixture_edits_keep_the_owner_and_history() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Inline show").unwrap()).unwrap();
    std::fs::write(root.join("project.donder"), "show:\n  type: project\n  setup:\n    type: setup\n    layout: {type: layout, fixtures: []}\n    patch: {type: patch, routes: []}\n    controllers: []\n  sequences: []\n").unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let layout = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::AddDefinition {
                name: "Owned strip".into(),
                storage: FixtureStorage::Inline,
                parent: None,
                transform: transform(0.0),
            },
        )
        .document,
    );
    assert_eq!(
        layout.source_ref.owned_path,
        vec![GuiOwnedStep::Setup, GuiOwnedStep::Layout]
    );
    let fixture = fixture_reference(&layout, 0);
    let before = state.project_session().unwrap();
    let GuiDocument::Fixture { document } = fixture_edit(
        &state,
        &fixture,
        FixtureGuiEdit::SetElements {
            elements: vec![GuiFixtureElement {
                id: 1,
                name: "Pixel".into(),
                diameter_meters: 0.01,
                transform: transform(0.0),
                reverse: false,
                shape: GuiFixtureShape::Pixel,
            }],
        },
    ) else {
        panic!("owned fixture edit failed")
    };
    assert_eq!(document.name, "Owned strip");
    assert_eq!(document.render_plan.pixels.len(), 1);
    let after = state.project_session().unwrap();
    assert!(after.project.layouts.is_empty());
    assert!(after.project.definitions.fixtures.definitions.is_empty());
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *before);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *after);
    // Use the desktop save barrier so this readback cannot race background autosave.
    state.save_all().unwrap();
    let reloaded = donder_project_io::load_project(&root).unwrap();
    assert_eq!(reloaded.project, after.project);
}
#[test]
fn ownership_controls_promote_and_unlink_every_slot_with_save_and_history() {
    for storage in [ReusableStorage::SameFile, ReusableStorage::NewFile] {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
        write_new_project_files(&root, &new_test_project_files("Owned show").unwrap()).unwrap();
        std::fs::write(root.join("project.donder"), "show:\n  type: project\n  setup:\n    type: setup\n    layout:\n      type: layout\n      fixtures:\n      - id: 1\n        name: My Strip\n        type: fixture\n        definition: {type: fixture, elements: []}\n    patch: {type: patch, routes: []}\n    controllers:\n    - id: 1\n      type: controller\n      protocol: {type: e131, source_name: Test, bind_address: 0.0.0.0, priority: 100, mode: multicast}\n      ports: [{id: 1, slot_count: 512, universe: 1}]\n  sequences:\n  - id: 1\n    type: sequence\n    duration: 1s\n    frame_rate: 30\n    audio: null\n    layers: []\n    effects: []\n    composition_graph:\n      nodes: [{id: 1, position: {x: 0, y: 0}, type: output}]\n      edges: []\n").unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let mut settings = state.snapshot().settings;
        settings.autosave_project_edits = false;
        state.update_app_settings(settings);
        for slot in [
            GuiOwnershipSlot::Fixture { id: 1 },
            GuiOwnershipSlot::Controller { index: 0 },
            GuiOwnershipSlot::Layout,
            GuiOwnershipSlot::Patch,
            GuiOwnershipSlot::Sequence { index: 0 },
            GuiOwnershipSlot::Setup,
        ] {
            let session = state.project_session().unwrap();
            let setup = session
                .project
                .setup(session.project.root.setup.id())
                .unwrap();
            let (owner, view) = match &slot {
                GuiOwnershipSlot::Fixture { .. } => {
                    (setup.layout.id().0.clone(), DocumentViewId::Layout)
                }
                GuiOwnershipSlot::Controller { .. }
                | GuiOwnershipSlot::Layout
                | GuiOwnershipSlot::Patch => (setup.id.0.clone(), DocumentViewId::Setup),
                GuiOwnershipSlot::Sequence { .. } | GuiOwnershipSlot::Setup => (
                    session.project.root.id.0.clone().into(),
                    DocumentViewId::Project,
                ),
            };
            state.open_file_path(owner.document().as_str());
            let request = || GuiDocumentRequest {
                owned_path: owner.owned_path().iter().map(Into::into).collect(),
                project_revision: state.snapshot().project_revision,
                path: owner.document().to_string(),
                object_key: Some(owner.root_source().object().into()),
                view: view.clone(),
            };
            for step in 0..4 {
                let edit = match step {
                    0 => GuiOwnershipEdit::MakeReusable {
                        name: "Reusable source".into(),
                        storage: storage.clone(),
                    },
                    1 | 3 => GuiOwnershipEdit::MakeIndependent,
                    2 => {
                        let projected = state.get_gui_document(request()).document;
                        let sources = match projected {
                            GuiDocument::Project { document } => document.available_sources,
                            GuiDocument::Setup { document } => document.available_sources,
                            GuiDocument::Layout { document } => document.available_fixtures,
                            other => panic!("Missing source choices: {other:?}"),
                        };
                        let kind = match slot {
                            GuiOwnershipSlot::Setup => ObjectKind::Setup,
                            GuiOwnershipSlot::Sequence { .. } => ObjectKind::Sequence,
                            GuiOwnershipSlot::Layout => ObjectKind::Layout,
                            GuiOwnershipSlot::Patch => ObjectKind::Patch,
                            GuiOwnershipSlot::Controller { .. } => ObjectKind::Controller,
                            GuiOwnershipSlot::Fixture { .. } => ObjectKind::Fixture,
                        };
                        GuiOwnershipEdit::UseExisting {
                            source: sources
                                .into_iter()
                                .find(|source| source.kind == kind)
                                .unwrap(),
                        }
                    }
                    _ => unreachable!(),
                };
                let before = state.project_session().unwrap();
                let result = state.apply_gui_edit(
                    request(),
                    GuiEditCommand::Ownership {
                        slot: slot.clone(),
                        edit,
                    },
                );
                assert!(
                    !matches!(result.document, GuiDocument::Blocked { .. }),
                    "{:?}",
                    result.document
                );
                let after = state.project_session().unwrap();
                state.undo_active_edit();
                assert_eq!(*state.project_session().unwrap(), *before);
                state.redo_active_edit();
                assert_eq!(*state.project_session().unwrap(), *after);
                state.save_all().unwrap();
                assert_eq!(
                    donder_project_io::load_project(&root).unwrap().project,
                    after.project
                );
            }
        }
        let session = state.project_session().unwrap();
        let setup = session
            .project
            .setup(session.project.root.setup.id())
            .unwrap();
        assert_eq!(
            session.project.layout(setup.layout.id()).unwrap().fixtures[0].name,
            "My Strip"
        );
        assert_eq!(session.project.definitions.fixtures.definitions.len(), 1);
        assert_eq!(session.project.controllers.len(), 1);
        assert_eq!(session.project.layouts.len(), 1);
        assert_eq!(session.project.patches.len(), 1);
        assert_eq!(session.project.sequences.len(), 1);
        assert_eq!(session.project.setups.len(), 1);
    }
}

#[test]
fn sequence_creation_storage_choices_are_undoable_and_roundtrip() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Sequences").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    state.open_file_path("project.donder");
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    for storage in [
        NewSequenceStorage::Inline,
        NewSequenceStorage::SameFile {
            name: "Opening Cue".into(),
        },
        NewSequenceStorage::NewFile {
            name: "Opening Cue".into(),
        },
    ] {
        let before = state.project_session().unwrap();
        let color = before
            .project
            .sequence(before.project.root.sequences[0].id())
            .unwrap()
            .layers[0]
            .color
            .to_hex();
        let inline = matches!(storage, NewSequenceStorage::Inline);
        let new_file = matches!(storage, NewSequenceStorage::NewFile { .. });
        let result = state
            .create_sequence(NewSequenceRequest {
                storage,
                initial_color: color,
                duration_seconds: 30.0,
                frame_rate: 40,
            })
            .unwrap();
        let after = state.project_session().unwrap();
        assert_eq!(
            after.project.root.sequences.len(),
            before.project.root.sequences.len() + 1
        );
        let source = after.project.root.sequences.last().unwrap();
        assert_eq!(
            matches!(source, donder_language::ownership::ValueSource::Inline(_)),
            inline
        );
        assert_eq!(result.source.owned_path.is_empty(), !inline);
        assert_eq!(result.source.path != "project.donder", new_file);
        let sequence = after.project.sequence(source.id()).unwrap();
        assert_eq!(sequence.duration.as_seconds_f32(), 30.0);
        assert_eq!(sequence.frame_rate, 40);
        state.undo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *before);
        state.redo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *after);
        state.save_all().unwrap();
        assert_eq!(
            donder_project_io::load_project(&root).unwrap().project,
            after.project
        );
    }
    let before = state.project_session().unwrap();
    let color = before
        .project
        .sequence(before.project.root.sequences[0].id())
        .unwrap()
        .layers[0]
        .color
        .to_hex();
    assert!(
        state
            .create_sequence(NewSequenceRequest {
                storage: NewSequenceStorage::Inline,
                initial_color: color,
                duration_seconds: 0.0,
                frame_rate: 40,
            })
            .is_err()
    );
    assert!(std::sync::Arc::ptr_eq(
        &before,
        &state.project_session().unwrap()
    ));
}

#[test]
fn duplicated_groups_own_geometry_and_preserve_original_sources() {
    for storage in [
        FixtureStorage::Inline,
        FixtureStorage::SameFile,
        FixtureStorage::NewFile,
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
        write_new_project_files(&root, &new_test_project_files("Duplicates").unwrap()).unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let mut settings = state.snapshot().settings;
        settings.autosave_project_edits = false;
        state.update_app_settings(settings);
        let layout = layout_document(
            edit_layout(
                &state,
                LayoutGuiEdit::AddDefinition {
                    name: "Original Strip".into(),
                    storage,
                    parent: None,
                    transform: transform(2.0),
                },
            )
            .document,
        );
        let source = fixture_reference(&layout, 0);
        let pixel = GuiFixtureElement {
            id: 1,
            name: "Pixel".into(),
            transform: transform(0.0),
            diameter_meters: 0.01,
            reverse: false,
            shape: GuiFixtureShape::Pixel,
        };
        assert!(matches!(
            fixture_edit(
                &state,
                &source,
                FixtureGuiEdit::SetElements {
                    elements: vec![pixel]
                }
            ),
            GuiDocument::Fixture { .. }
        ));
        let current = state.project_session().unwrap();
        let layout_id = current
            .project
            .setup(current.project.root.setup.id())
            .unwrap()
            .layout
            .id();
        let request = GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: layout_id.0.document().to_string(),
            object_key: Some(layout_id.0.root_source().object().into()),
            owned_path: layout_id.0.owned_path().iter().map(Into::into).collect(),
            view: DocumentViewId::Layout,
        };
        let layout = layout_document(state.get_gui_document(request).document);
        edit_layout(
            &state,
            LayoutGuiEdit::SetFixtures {
                fixtures: vec![GuiLayoutFixture {
                    id: 2,
                    name: "Group".into(),
                    kind: GuiLayoutFixtureKind::Group {
                        children: layout.fixtures,
                    },
                }],
            },
        );
        let before = state.project_session().unwrap();
        let duplicate = layout_document(
            edit_layout(&state, LayoutGuiEdit::DuplicateFixture { id: 2 }).document,
        );
        let after = state.project_session().unwrap();
        assert_eq!(duplicate.fixtures.len(), 2);
        let GuiLayoutFixtureKind::Group { children } = &duplicate.fixtures[1].kind else {
            panic!("Expected copied group")
        };
        assert_eq!(duplicate.fixtures[1].id, 3);
        assert_eq!(children[0].id, 4);
        let GuiLayoutFixtureKind::Fixture {
            definition,
            transform: placement,
        } = &children[0].kind
        else {
            panic!("Expected copied fixture")
        };
        assert!(matches!(definition, GuiFixtureSource::Inline { .. }));
        assert_eq!(placement.position.x_meters, 2.0);
        assert_eq!(
            after.project.definitions.fixtures,
            before.project.definitions.fixtures
        );
        state.undo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *before);
        state.redo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *after);
        let mut copied_source = duplicate.source_ref;
        copied_source.kind = ObjectKind::Fixture;
        copied_source
            .owned_path
            .push(GuiOwnedStep::Fixture { id: 4 });
        assert!(matches!(
            fixture_edit(
                &state,
                &copied_source,
                FixtureGuiEdit::SetElements { elements: vec![] }
            ),
            GuiDocument::Fixture { .. }
        ));
        let changed = state.project_session().unwrap();
        assert_eq!(
            changed.project.layout(layout_id).unwrap().fixtures[0],
            before.project.layout(layout_id).unwrap().fixtures[0]
        );
        assert_eq!(
            changed.project.definitions.fixtures,
            before.project.definitions.fixtures
        );
        state.save_all().unwrap();
        assert_eq!(
            donder_project_io::load_project(&root).unwrap().project,
            changed.project
        );
    }
}

#[test]
fn layout_tree_moves_preserve_owned_identity_and_support_history() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Tree").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    for (name, x) in [("First", 2.0), ("Second", 4.0)] {
        edit_layout(
            &state,
            LayoutGuiEdit::AddDefinition {
                name: name.into(),
                storage: FixtureStorage::Inline,
                parent: None,
                transform: transform(x),
            },
        );
    }
    let layout = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::ReparentFixture {
                id: 2,
                parent: None,
                before: None,
            },
        )
        .document,
    );
    let first_source = fixture_reference(&layout, 0);
    let group = |id, name: &str, children| GuiLayoutFixture {
        id,
        name: name.into(),
        kind: GuiLayoutFixtureKind::Group { children },
    };
    edit_layout(
        &state,
        LayoutGuiEdit::SetFixtures {
            fixtures: vec![
                group(
                    3,
                    "First group",
                    vec![layout.fixtures[0].clone(), group(5, "Nested", vec![])],
                ),
                layout.fixtures[1].clone(),
                group(4, "Second group", vec![]),
            ],
        },
    );
    let initial = state.project_session().unwrap();
    let layout_id = initial
        .project
        .setup(initial.project.root.setup.id())
        .unwrap()
        .layout
        .id();
    let fixture_id = donder_language::layout::FixtureInstanceId;
    let original_first = initial
        .project
        .layout(layout_id)
        .unwrap()
        .fixture(fixture_id(1))
        .unwrap()
        .clone();
    for (id, parent, before) in [
        (2, Some(3), Some(1)),
        (1, Some(4), None),
        (4, Some(3), Some(5)),
        (2, None, Some(3)),
        (2, None, None),
    ] {
        let before_session = state.project_session().unwrap();
        let result = edit_layout(
            &state,
            LayoutGuiEdit::ReparentFixture { id, parent, before },
        );
        assert!(
            !matches!(result.document, GuiDocument::Blocked { .. }),
            "{:?}",
            result.document
        );
        let after = state.project_session().unwrap();
        assert_eq!(
            after
                .project
                .layout(layout_id)
                .unwrap()
                .fixture(fixture_id(1))
                .unwrap(),
            &original_first
        );
        assert_eq!(
            after.project.definitions.fixtures,
            initial.project.definitions.fixtures
        );
        state.undo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *before_session);
        state.redo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *after);
    }
    let current = state.project_session().unwrap();
    assert_eq!(
        current
            .project
            .layout(layout_id)
            .unwrap()
            .fixtures
            .iter()
            .map(|item| item.id.0)
            .collect::<Vec<_>>(),
        vec![3, 2]
    );
    for (id, parent, before) in [
        (3, Some(5), None),
        (3, Some(3), None),
        (2, Some(1), None),
        (2, Some(4), Some(5)),
        (99, None, None),
    ] {
        let result = edit_layout(
            &state,
            LayoutGuiEdit::ReparentFixture { id, parent, before },
        );
        assert!(matches!(result.document, GuiDocument::Blocked { .. }));
        assert!(std::sync::Arc::ptr_eq(
            &current,
            &state.project_session().unwrap()
        ));
    }
    // Group ancestry is not part of an owned fixture's address.
    let request = GuiDocumentRequest {
        project_revision: state.snapshot().project_revision,
        path: first_source.path,
        object_key: Some(first_source.object_key),
        owned_path: first_source.owned_path,
        view: DocumentViewId::Fixture,
    };
    assert!(matches!(
        state.get_gui_document(request).document,
        GuiDocument::Fixture { .. }
    ));
    state.save_all().unwrap();
    assert_eq!(
        donder_project_io::load_project(&root).unwrap().project,
        current.project
    );
}

#[test]
fn repeated_groups_are_independent_ordered_and_one_history_edit() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Repeat").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let layout = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::AddDefinition {
                name: "Strip".into(),
                storage: FixtureStorage::SameFile,
                parent: None,
                transform: transform(2.0),
            },
        )
        .document,
    );
    let source = fixture_reference(&layout, 0);
    fixture_edit(
        &state,
        &source,
        FixtureGuiEdit::SetElements {
            elements: vec![GuiFixtureElement {
                id: 1,
                name: "Pixel".into(),
                transform: transform(0.0),
                diameter_meters: 0.01,
                reverse: false,
                shape: GuiFixtureShape::Pixel,
            }],
        },
    );
    edit_layout(
        &state,
        LayoutGuiEdit::SetFixtures {
            fixtures: vec![GuiLayoutFixture {
                id: 2,
                name: "Group".into(),
                kind: GuiLayoutFixtureKind::Group {
                    children: layout.fixtures,
                },
            }],
        },
    );
    let before = state.project_session().unwrap();
    let result = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::RepeatFixtures {
                ids: vec![2],
                offsets: vec![
                    Point3Meters {
                        x_meters: 3.0,
                        y_meters: 1.0,
                        z_meters: 0.0,
                    },
                    Point3Meters {
                        x_meters: 6.0,
                        y_meters: 2.0,
                        z_meters: 0.0,
                    },
                ],
            },
        )
        .document,
    );
    assert_eq!(result.fixtures.len(), 3);
    for (copy, x, y) in [
        (&result.fixtures[1], 5.0, 1.0),
        (&result.fixtures[2], 8.0, 2.0),
    ] {
        let GuiLayoutFixtureKind::Group { children } = &copy.kind else {
            panic!("Expected group")
        };
        let GuiLayoutFixtureKind::Fixture {
            definition,
            transform,
        } = &children[0].kind
        else {
            panic!("Expected fixture")
        };
        assert!(matches!(definition, GuiFixtureSource::Inline { elements } if elements.len() == 1));
        assert_eq!(transform.position.x_meters, x);
        assert_eq!(transform.position.y_meters, y);
    }
    let after = state.project_session().unwrap();
    assert_eq!(
        after.project.definitions.fixtures,
        before.project.definitions.fixtures
    );
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *before);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *after);
    // Ancestor/descendant selection and an overflowing later copy reject the entire edit.
    for (ids, offsets) in [
        (
            vec![2, 1],
            vec![Point3Meters {
                x_meters: 1.0,
                y_meters: 0.0,
                z_meters: 0.0,
            }],
        ),
        (
            vec![2],
            vec![
                Point3Meters {
                    x_meters: 2000.0,
                    y_meters: 0.0,
                    z_meters: 0.0,
                },
                Point3Meters {
                    x_meters: 1.0,
                    y_meters: 0.0,
                    z_meters: 0.0,
                },
            ],
        ),
    ] {
        edit_layout(&state, LayoutGuiEdit::RepeatFixtures { ids, offsets });
        assert_eq!(*state.project_session().unwrap(), *after);
    }
    state.save_all().unwrap();
    assert_eq!(
        donder_project_io::load_project(&root).unwrap().project,
        after.project
    );
}

#[test]
fn line_endpoints_move_independently_with_rotation_scale_and_history() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Endpoints").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let layout = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::AddDefinition {
                storage: FixtureStorage::Inline,
                name: "Line".into(),
                parent: None,
                transform: transform(0.0),
            },
        )
        .document,
    );
    let source = fixture_reference(&layout, 0);
    for (x_rotation, y_rotation, z_rotation, x_scale) in [
        (0.0, 0.0, 0.0, 1.0),
        (0.0, 0.0, 45.0, 2.0),
        (25.0, 30.0, -40.0, -2.0),
    ] {
        let mut placement = transform(1.0);
        placement.rotation = Rotation3Degrees {
            x_degrees: x_rotation,
            y_degrees: y_rotation,
            z_degrees: z_rotation,
        };
        placement.scale = Scale3 {
            x: x_scale,
            y: 0.5,
            z: 3.0,
        };
        let GuiDocument::Fixture { document: original } = fixture_edit(
            &state,
            &source,
            FixtureGuiEdit::SetElements {
                elements: vec![GuiFixtureElement {
                    id: 1,
                    name: "Line".into(),
                    transform: placement,
                    diameter_meters: 0.01,
                    reverse: true,
                    shape: GuiFixtureShape::Line {
                        length: 2.0,
                        count: 5,
                    },
                }],
            },
        ) else {
            panic!("Expected fixture")
        };
        let mut current = original;
        for index in [0, 1] {
            let opposite = current.handles[(1 - index) as usize].position.clone();
            let previous = current.handles[index as usize].position.clone();
            let moved_to = Point3Meters {
                x_meters: previous.x_meters + 0.75,
                y_meters: previous.y_meters - 1.25,
                z_meters: previous.z_meters,
            };
            let before = state.project_session().unwrap();
            let GuiDocument::Fixture { document: moved } = fixture_edit(
                &state,
                &source,
                FixtureGuiEdit::MoveHandle {
                    id: 1,
                    index,
                    position: moved_to.clone(),
                },
            ) else {
                panic!("Expected fixture")
            };
            for (actual, expected) in [
                (&moved.handles[index as usize].position, &moved_to),
                (&moved.handles[(1 - index) as usize].position, &opposite),
            ] {
                assert!((actual.x_meters - expected.x_meters).abs() < 0.00001);
                assert!((actual.y_meters - expected.y_meters).abs() < 0.00001);
                assert!((actual.z_meters - expected.z_meters).abs() < 0.00001);
            }
            assert_eq!(moved.render_plan.pixels.len(), 5);
            assert!(moved.elements[0].reverse);
            let after = state.project_session().unwrap();
            state.undo_active_edit();
            assert_eq!(*state.project_session().unwrap(), *before);
            state.redo_active_edit();
            assert_eq!(*state.project_session().unwrap(), *after);
            current = moved;
        }
    }
    state.save_all().unwrap();
    assert_eq!(
        donder_project_io::load_project(&root).unwrap().project,
        state.project_session().unwrap().project
    );
}

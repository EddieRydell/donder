use super::DesktopState;
use crate::dto::*;
use crate::project::{new_test_project_files, write_new_project_files};
use camino::Utf8PathBuf;

pub(super) fn edit_layout(state: &DesktopState, edit: LayoutGuiEdit) -> GuiEditResult {
    let session = state.project_session().unwrap();
    let layout = session
        .project
        .setup(session.project.root().setup.id())
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
    let id = session.project.root().setup.id();
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
    write_root_content(
        &root,
        "import layout from <layouts/main.data.donder>;",
        "  setup: Setup {
    description: none,
    layout: layout.main,
    patch: Patch { description: none, routes: [] },
    controllers: [],
  },
  sequences: [],",
    );
    std::fs::write(
        root.join("layouts/main.data.donder"),
        r#"
FixtureDefinition assembly {
  description: none,
  shapes: [
    Shape {
      name: pixel,
      diameter: 0.01m,
      reverse: false,
      transform: Transform { position: (1m, 0m, 0m), rotation: (0.0, 0.0, 0.0), scale: (1.0, 1.0, 1.0) },
      geometry: Pixel,
    },
  ],
}

Layout main {
  description: none,
  root: [a, b],
  items: [
    Fixture {
      name: a,
      description: none,
      definition: assembly,
      transform: Transform { position: (0m, 0m, 0m), rotation: (0.0, 0.0, 0.0), scale: (1.0, 1.0, 1.0) },
    },
    Fixture {
      name: b,
      description: none,
      definition: assembly,
      transform: Transform { position: (10m, 0m, 0m), rotation: (0.0, 0.0, 0.0), scale: (1.0, 1.0, 1.0) },
    },
  ],
}
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
        path: "layouts/main.data.donder".into(),
        object_key: Some("main".into()),
        view: DocumentViewId::Layout,
    };
    state.open_file_path("layouts/main.data.donder");
    let layout = layout_document(state.get_gui_document(layout_request()).document);
    let assembly = &fixture_reference(&layout, 0);
    assert_eq!(assembly.path, layout.path);
    assert!(matches!(
        fixture_edit(
            &state,
            assembly,
            FixtureGuiEdit::MoveElement {
                id: 1,
                delta: Point3Meters {
                    x_meters: 3.0,
                    y_meters: 4.0,
                    z_meters: 0.0
                },
            }
        ),
        GuiDocument::Fixture { .. }
    ));
    state.open_file_path("layouts/main.data.donder");
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
    layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::SetFixtures {
                fixtures: vec![],
                root: vec![],
            },
        )
        .document,
    );
    assert_eq!(
        state
            .project_session()
            .unwrap()
            .project
            .definitions()
            .fixtures
            .definitions
            .len(),
        1
    );
    state.save_all().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(
        &root,
        &state.project_session().unwrap(),
    );
}

#[test]
fn empty_project_authors_shared_fixtures_routes_effect_and_reopens_without_text_edits() {
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
    let setup_id = initial.project.root().setup.id().clone();
    let sequence_id = initial.project.root().sequences[0].id().clone();
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
    assert_eq!(definition.path, "fixtures/strip.data.donder");
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
    let grouped = vec![
        GuiLayoutFixture {
            id: 100,
            name: "Both strips".into(),
            description: None,
            kind: GuiLayoutFixtureKind::Group {
                members: vec![layout.fixtures[0].id, second.id],
            },
        },
        layout.fixtures[0].clone(),
        second,
    ];
    let layout = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::SetFixtures {
                fixtures: grouped,
                root: vec![100],
            },
        )
        .document,
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
                name: "port_1".into(),
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
                effect: SequenceEffectReference::Custom {
                    module_id: initial.source.project_module_id().to_string(),
                    path: "effects/standard.donder".into(),
                    effect_name: "Pulse".into(),
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
        final_session.project.definitions().fixtures,
        before_copy.project.definitions().fixtures
    );
    assert_eq!(
        final_session
            .project
            .sequence(final_session.project.root().sequences[0].id())
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
    let prepared = donder_elaboration::prepare(
        &final_session.project,
        final_session.project.root().sequences[0].id(),
        donder_elaboration::PrepareOutputs::All,
    )
    .unwrap();
    let mut playback = prepared.into_playback();
    let mut illuminated = false;
    for frame in 0..60 {
        let rendered =
            playback.evaluate(donder_language::values::sample_time_from_frame(frame, 60).unwrap());
        let slots = rendered.outputs().next().unwrap().bytes;
        illuminated |= slots[..12].iter().any(|&value| value != 0);
        assert!(slots[12..].iter().all(|&value| value == 0));
    }
    assert!(illuminated);
    state.save_all().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(&root, &final_session);
}

#[test]
fn local_controller_and_layout_copies_preserve_shared_files_and_reopen() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Local copy").unwrap()).unwrap();
    let starter = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let library = root.join("rig");
    let paths = [
        "layouts/outputs.data.donder",
        "patches/outputs.data.donder",
        "setups/main.data.donder",
        "fixtures/vertical.data.donder",
    ];
    for path in paths {
        let text = std::fs::read_to_string(starter.join(path)).unwrap();
        let bytes = paths
            .iter()
            .fold(text, |text, path| {
                text.replace(&format!("<{path}>"), &format!("<rig/{path}>"))
            })
            .into_bytes();
        std::fs::create_dir_all(library.join(path).parent().unwrap()).unwrap();
        std::fs::write(library.join(path), &bytes).unwrap();
    }
    std::fs::create_dir_all(root.join("setups")).unwrap();
    write_root_content(
        &root,
        "import setups from <setups/main.data.donder>;",
        "  setup: setups.main,\n  sequences: [],",
    );
    std::fs::write(
        root.join("setups/main.data.donder"),
        "import layout from <rig/layouts/outputs.data.donder>;
import patch from <rig/patches/outputs.data.donder>;
import controllers from <rig/setups/main.data.donder>;

Setup main {
  description: none,
  layout: layout.outputs_layout,
  patch: patch.outputs,
  controllers: [controllers.output_controller],
}
",
    )
    .unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let original = state.project_session().unwrap();
    let setup_id = original.project.root().setup.id().clone();
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
    let copied_setup = &copied.project.reusable_setups()[&setup_id];
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
        copied.project.definitions().fixtures,
        original.project.definitions().fixtures
    );
    let imported_request = GuiDocumentRequest {
        owned_path: Vec::new(),
        path: "rig/setups/main.data.donder".into(),
        ..request()
    };
    assert!(matches!(
        state.get_gui_document(imported_request).document,
        GuiDocument::Setup { .. }
    ));
    let old_setup = &original.project.reusable_setups()[&setup_id];
    assert_eq!(
        copied.project.reusable_layouts()[old_setup.layout.id()],
        original.project.reusable_layouts()[old_setup.layout.id()]
    );
    assert_eq!(
        copied.project.reusable_patches()[old_setup.patch.id()],
        original.project.reusable_patches()[old_setup.patch.id()]
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
                root: layout.root,
            },
        )
        .document,
    );
    state.save_all().unwrap();
    let saved = state.project_session().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(&root, &saved);
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
    crate::desktop_foundation_tests::tests::assert_reloads(&root, &after_conversion);
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
                FixtureStorage::Inline | FixtureStorage::SameFile => "project.data.donder",
                FixtureStorage::NewFile => "fixtures/my_strip.data.donder",
            }
        );
        let mut second = layout.fixtures[0].clone();
        second.id = 2;
        second.name = "Shared placement".into();
        let group = GuiLayoutFixture {
            id: 3,
            name: "Group".into(),
            description: None,
            kind: GuiLayoutFixtureKind::Group { members: vec![2] },
        };
        layout_document(
            edit_layout(
                &state,
                LayoutGuiEdit::SetFixtures {
                    fixtures: vec![layout.fixtures[0].clone(), group.clone(), second.clone()],
                    root: vec![layout.fixtures[0].id, 3],
                },
            )
            .document,
        );
        layout_document(
            edit_layout(
                &state,
                LayoutGuiEdit::SetFixtures {
                    fixtures: vec![group, second],
                    root: vec![3],
                },
            )
            .document,
        );
        assert_eq!(
            state
                .project_session()
                .unwrap()
                .project
                .definitions()
                .fixtures
                .definitions
                .len(),
            usize::from(!matches!(storage, FixtureStorage::Inline))
        );
        let before = state.project_session().unwrap();
        let removed = layout_document(
            edit_layout(
                &state,
                LayoutGuiEdit::SetFixtures {
                    fixtures: vec![],
                    root: vec![],
                },
            )
            .document,
        );
        assert!(removed.fixtures.is_empty());
        let after = state.project_session().unwrap();
        assert_eq!(
            after.project.definitions().fixtures.definitions.len(),
            usize::from(!matches!(storage, FixtureStorage::Inline))
        );
        state.undo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *before);
        state.redo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *after);
        state.save_all().unwrap();
        crate::desktop_foundation_tests::tests::assert_reloads(&root, &after);
        let text = std::fs::read_to_string(root.join("project.data.donder")).unwrap();
        assert!(!text.contains("fixture_1"));
        if matches!(storage, FixtureStorage::NewFile) {
            assert!(root.join("fixtures/my_strip.data.donder").exists());
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
        .setup(state.project_session().unwrap().project.root().setup.id())
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
                .setup(copied.project.root().setup.id())
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
                    edit: LayoutGuiEdit::SetFixtures {
                        fixtures: vec![],
                        root: vec![],
                    },
                },
            )
            .document,
    );
    assert_eq!(
        state
            .project_session()
            .unwrap()
            .project
            .definitions()
            .fixtures
            .definitions
            .len(),
        0
    );
    layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::SetFixtures {
                fixtures: vec![],
                root: vec![],
            },
        )
        .document,
    );
    assert!(
        state
            .project_session()
            .unwrap()
            .project
            .definitions()
            .fixtures
            .definitions
            .is_empty()
    );
    state.save_all().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(
        &root,
        &state.project_session().unwrap(),
    );
}

#[test]
fn nested_layout_and_fixture_edits_keep_the_owner_and_history() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Inline show").unwrap()).unwrap();
    write_root_content(
        &root,
        "",
        "  setup: Setup {
    description: none,
    layout: Layout { description: none, root: [], items: [] },
    patch: Patch { description: none, routes: [] },
    controllers: [],
  },
  sequences: [],",
    );
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
    assert_eq!(document.name, "owned_strip");
    assert_eq!(document.render_plan.pixels.len(), 1);
    let after = state.project_session().unwrap();
    assert!(after.project.reusable_layouts().is_empty());
    assert!(after.project.definitions().fixtures.definitions.is_empty());
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
        write_root_content(
            &root,
            "",
            "  setup: Setup {
    description: none,
    layout: Layout {
      description: none,
      root: [my_strip],
      items: [
        Fixture {
          name: my_strip,
          description: none,
          definition: FixtureDefinition { description: none, shapes: [] },
          transform: Transform { position: (0m, 0m, 0m), rotation: (0.0, 0.0, 0.0), scale: (1.0, 1.0, 1.0) },
        },
      ],
    },
    patch: Patch { description: none, routes: [] },
    controllers: [
      Controller main {
        description: none,
        protocol: E131 { source_name: \"Test\", bind_address: \"0.0.0.0\", priority: 100, mode: Multicast },
        ports: [Port { name: port_1, address: Universe { universe: 1 }, slots: 512 }],
      },
    ],
  },
  sequences: [
    Sequence main {
      description: none,
      duration: 1s,
      frame_rate: 30,
      audio: none,
      marks: [],
      layers: [],
      clips: [],
      graph: Graph { nodes: [OutputNode { position: (0.0, 0.0) }], edges: [] },
      automation: [],
    },
  ],",
        );
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
                .setup(session.project.root().setup.id())
                .unwrap();
            let (owner, view) = match &slot {
                GuiOwnershipSlot::Fixture { .. } => {
                    (setup.layout.id().0.clone(), DocumentViewId::Layout)
                }
                GuiOwnershipSlot::Controller { .. }
                | GuiOwnershipSlot::Layout
                | GuiOwnershipSlot::Patch => (setup.id.0.clone(), DocumentViewId::Setup),
                GuiOwnershipSlot::Sequence { .. } | GuiOwnershipSlot::Setup => (
                    session.project.root().id.0.clone().into(),
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
                // The promoted and final unlinked states cover both persisted ownership forms.
                if matches!(step, 0 | 3) {
                    state.save_all().unwrap();
                    crate::desktop_foundation_tests::tests::assert_reloads(&root, &after);
                }
            }
        }
        let session = state.project_session().unwrap();
        let setup = session
            .project
            .setup(session.project.root().setup.id())
            .unwrap();
        assert_eq!(
            session.project.layout(setup.layout.id()).unwrap().fixtures[0]
                .name
                .as_str(),
            "my_strip"
        );
        assert_eq!(session.project.definitions().fixtures.definitions.len(), 1);
        assert_eq!(session.project.reusable_controllers().len(), 1);
        assert_eq!(session.project.reusable_layouts().len(), 1);
        assert_eq!(session.project.reusable_patches().len(), 1);
        assert_eq!(session.project.reusable_sequences().len(), 1);
        assert_eq!(session.project.reusable_setups().len(), 1);
    }
}

#[test]
fn new_project_hue_shift_catalog_edits_and_imports_roundtrip() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Operators").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let initial = state.project_session().unwrap();
    let initial_id = initial.project.root().sequences[0].id().clone();
    let color = initial.project.sequence(&initial_id).unwrap().layers[0]
        .color
        .to_hex();
    state
        .create_sequence(NewSequenceRequest {
            storage: NewSequenceStorage::NewFile {
                name: "Imported operators".into(),
            },
            initial_color: color.clone(),
            duration_seconds: 30.0,
            frame_rate: 60,
        })
        .unwrap();
    let new_id = state
        .project_session()
        .unwrap()
        .project
        .root()
        .sequences
        .last()
        .unwrap()
        .id()
        .clone();
    for id in [initial_id, new_id] {
        state.open_file_path(id.0.document().as_str());
        let request = || GuiDocumentRequest {
            owned_path: id.0.owned_path().iter().map(Into::into).collect(),
            project_revision: state.snapshot().project_revision,
            path: id.0.document().to_string(),
            view: DocumentViewId::Sequence,
            object_key: Some(id.0.root_source().object().to_string()),
        };
        let GuiDocument::Sequence { document } = state.get_gui_document(request()).document else {
            panic!("sequence projection missing");
        };
        let hue_shift = document.composition_graph.operator_catalog.iter().find(|entry| {
            matches!(&entry.operator, SequenceGraphOperator::Custom { object_key, .. } if object_key == "HueShift")
        }).expect("new project catalog includes HueShift");
        let SequenceGraphOperator::Custom {
            module_id, path, ..
        } = &hue_shift.operator;
        assert_eq!(module_id, &id.0.module_id().to_string());
        assert_eq!(path, "operators/standard.donder");
        let edit = |edit| state.apply_gui_edit(request(), GuiEditCommand::Sequence { edit });
        let connection = document.composition_graph.edges[0].clone();
        let inserted = edit(SequenceGuiEdit::AddGraphOperatorNode {
            operator: hue_shift.operator.clone(),
            initial_color: color.clone(),
            x: 240.0,
            y: 80.0,
        })
        .document;
        let GuiDocument::Sequence { document } = inserted else {
            panic!("HueShift insertion rejected: {inserted:?}");
        };
        let node = document
            .composition_graph
            .nodes
            .iter()
            .find(|node| matches!(node.kind, SequenceGraphNodeKind::Operator { .. }))
            .unwrap();
        let SequenceGraphNodeKind::Operator { params, .. } = &node.kind else {
            unreachable!()
        };
        let shift = params.iter().find(|param| param.name == "shift").unwrap();
        assert!(shift.editable);
        assert!(shift.supports_automation);
        let disconnected = state.project_session().unwrap();
        state.undo_active_edit();
        assert!(
            state
                .project_session()
                .unwrap()
                .project
                .sequence(&id)
                .unwrap()
                .composition_graph
                .nodes
                .iter()
                .all(|candidate| !matches!(
                    candidate.kind,
                    donder_language::sequence::CompositionGraphNodeKind::Operator(_)
                ))
        );
        state.redo_active_edit();
        assert_eq!(
            state.project_session().unwrap().project,
            disconnected.project
        );
        let premature = edit(SequenceGuiEdit::ConnectGraphNodes {
            from_node: node.id.clone(),
            from_port: "output".into(),
            to_node: connection.to_node.clone(),
            to_port: connection.to_port.clone(),
        });
        assert!(matches!(premature.document, GuiDocument::Blocked { .. }));
        assert_eq!(
            state.project_session().unwrap().project,
            disconnected.project
        );
        let GuiDocument::Sequence { document } = edit(SequenceGuiEdit::UpdateGraphOperatorParam {
            node_id: node.id.clone(),
            name: "shift".into(),
            value: SequenceEffectParamValue::Float { value: 0.25 },
        })
        .document
        else {
            panic!("HueShift parameter edit rejected");
        };
        let edited = document
            .composition_graph
            .nodes
            .iter()
            .find(|candidate| candidate.id == node.id)
            .unwrap();
        let SequenceGraphNodeKind::Operator { params, .. } = &edited.kind else {
            unreachable!()
        };
        assert!(
            matches!(params.iter().find(|param| param.name == "shift").unwrap().value, SequenceEffectParamValue::Float { value } if value == 0.25)
        );
        let accepted = state.project_session().unwrap();
        state.save_all().unwrap();
        crate::desktop_foundation_tests::tests::assert_reloads(&root, &accepted);
        let authored = std::fs::read_to_string(root.join(id.0.document())).unwrap();
        assert!(authored.contains("operators/standard.donder"));
        assert!(authored.contains("operators.HueShift"));
        assert!(matches!(
            edit(SequenceGuiEdit::ConnectGraphNodes {
                from_node: connection.from_node.clone(),
                from_port: connection.from_port.clone(),
                to_node: node.id.clone(),
                to_port: "source".into(),
            })
            .document,
            GuiDocument::Sequence { .. }
        ));
        assert!(matches!(
            edit(SequenceGuiEdit::ReconnectGraphEdge {
                previous: connection.clone(),
                connection: SequenceGraphEdge {
                    from_node: node.id.clone(),
                    from_port: "output".into(),
                    to_node: connection.to_node,
                    to_port: connection.to_port,
                },
            })
            .document,
            GuiDocument::Sequence { .. }
        ));
    }
}

#[test]
fn sequence_creation_storage_choices_are_undoable_and_roundtrip() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Sequences").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    state.open_file_path("project.data.donder");
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
            .sequence(before.project.root().sequences[0].id())
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
            after.project.root().sequences.len(),
            before.project.root().sequences.len() + 1
        );
        let source = after.project.root().sequences.last().unwrap();
        assert_eq!(
            matches!(source, donder_language::ownership::ValueSource::Inline(_)),
            inline
        );
        assert_eq!(result.source.owned_path.is_empty(), !inline);
        assert_eq!(result.source.path != "project.data.donder", new_file);
        let sequence = after.project.sequence(source.id()).unwrap();
        assert_eq!(sequence.duration.as_seconds_f32(), 30.0);
        assert_eq!(sequence.frame_rate, 40);
        state.undo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *before);
        state.redo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *after);
        state.save_all().unwrap();
        crate::desktop_foundation_tests::tests::assert_reloads(&root, &after);
    }
    let before = state.project_session().unwrap();
    let color = before
        .project
        .sequence(before.project.root().sequences[0].id())
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
            .setup(current.project.root().setup.id())
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
                fixtures: std::iter::once(GuiLayoutFixture {
                    id: 2,
                    name: "Group".into(),
                    description: None,
                    kind: GuiLayoutFixtureKind::Group {
                        members: layout.fixtures.iter().map(|fixture| fixture.id).collect(),
                    },
                })
                .chain(layout.fixtures)
                .collect(),
                root: vec![2],
            },
        );
        let before = state.project_session().unwrap();
        let duplicate = layout_document(
            edit_layout(&state, LayoutGuiEdit::DuplicateFixture { id: 2 }).document,
        );
        let after = state.project_session().unwrap();
        // The group copy lists its own copied member, after the original in the root.
        assert_eq!(
            duplicate
                .fixtures
                .iter()
                .map(|fixture| fixture.id)
                .collect::<Vec<_>>(),
            [2, 3, 1, 4]
        );
        assert_eq!(duplicate.root, [2, 3]);
        let GuiLayoutFixtureKind::Group { members } = &duplicate.fixtures[1].kind else {
            panic!("Expected copied group")
        };
        assert_eq!(members, &[4]);
        let GuiLayoutFixtureKind::Fixture {
            definition,
            transform: placement,
        } = &duplicate.fixtures[3].kind
        else {
            panic!("Expected copied fixture")
        };
        assert!(matches!(definition, GuiFixtureSource::Inline { .. }));
        assert_eq!(placement.position.x_meters, 2.0);
        assert_eq!(
            after.project.definitions().fixtures,
            before.project.definitions().fixtures
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
            changed.project.definitions().fixtures,
            before.project.definitions().fixtures
        );
        state.save_all().unwrap();
        crate::desktop_foundation_tests::tests::assert_reloads(&root, &changed);
    }
}

#[test]
fn layout_membership_edits_preserve_owned_identity_and_support_history() {
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
            LayoutGuiEdit::MoveMember {
                id: 2,
                from: None,
                to: None,
                before: None,
            },
        )
        .document,
    );
    assert_eq!(layout.root, [1, 2]);
    let first_source = fixture_reference(&layout, 0);
    let group = |id, name: &str, members| GuiLayoutFixture {
        id,
        name: name.into(),
        description: None,
        kind: GuiLayoutFixtureKind::Group { members },
    };
    edit_layout(
        &state,
        LayoutGuiEdit::SetFixtures {
            fixtures: vec![
                group(3, "First group", vec![1, 5]),
                group(5, "Nested", vec![]),
                layout.fixtures[0].clone(),
                layout.fixtures[1].clone(),
                group(4, "Second group", vec![]),
            ],
            root: vec![3, 2, 4],
        },
    );
    let initial = state.project_session().unwrap();
    let layout_id = initial
        .project
        .setup(initial.project.root().setup.id())
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
    for edit in [
        LayoutGuiEdit::MoveMember {
            id: 2,
            from: None,
            to: Some(3),
            before: Some(1),
        },
        // A fixture may belong to several groups.
        LayoutGuiEdit::AddMember {
            id: 1,
            to: Some(4),
            before: None,
        },
        LayoutGuiEdit::MoveMember {
            id: 4,
            from: None,
            to: Some(3),
            before: Some(5),
        },
        LayoutGuiEdit::MoveMember {
            id: 2,
            from: Some(3),
            to: None,
            before: Some(3),
        },
        LayoutGuiEdit::RemoveMember {
            id: 1,
            from: Some(3),
        },
    ] {
        let before_session = state.project_session().unwrap();
        let result = edit_layout(&state, edit);
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
            after.project.definitions().fixtures,
            initial.project.definitions().fixtures
        );
        state.undo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *before_session);
        state.redo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *after);
    }
    let current = state.project_session().unwrap();
    let layout = current.project.layout(layout_id).unwrap();
    assert_eq!(layout.root, [fixture_id(2), fixture_id(3)]);
    assert_eq!(
        layout.children(Some(fixture_id(3))).unwrap(),
        [fixture_id(4), fixture_id(5)]
    );
    assert_eq!(
        layout.children(Some(fixture_id(4))).unwrap(),
        [fixture_id(1)]
    );
    for edit in [
        // Cycles, non-group destinations, repeated members and missing items.
        LayoutGuiEdit::MoveMember {
            id: 3,
            from: None,
            to: Some(5),
            before: None,
        },
        LayoutGuiEdit::AddMember {
            id: 3,
            to: Some(3),
            before: None,
        },
        LayoutGuiEdit::AddMember {
            id: 2,
            to: Some(1),
            before: None,
        },
        LayoutGuiEdit::AddMember {
            id: 1,
            to: Some(4),
            before: None,
        },
        LayoutGuiEdit::RemoveMember {
            id: 2,
            from: Some(4),
        },
        LayoutGuiEdit::MoveMember {
            id: 99,
            from: None,
            to: None,
            before: None,
        },
    ] {
        let result = edit_layout(&state, edit);
        assert!(matches!(result.document, GuiDocument::Blocked { .. }));
        assert!(std::sync::Arc::ptr_eq(
            &current,
            &state.project_session().unwrap()
        ));
    }
    // Group membership is not part of an owned fixture's address.
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
    crate::desktop_foundation_tests::tests::assert_reloads(&root, &current);
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
            fixtures: std::iter::once(GuiLayoutFixture {
                id: 2,
                name: "Group".into(),
                description: None,
                kind: GuiLayoutFixtureKind::Group {
                    members: layout.fixtures.iter().map(|fixture| fixture.id).collect(),
                },
            })
            .chain(layout.fixtures)
            .collect(),
            root: vec![2],
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
    // Copies follow the original in offset order, each with its own copied member.
    assert_eq!(result.root, [2, 5, 3]);
    let item = |id| {
        result
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .unwrap()
    };
    for (copy, x, y) in [(5, 5.0, 1.0), (3, 8.0, 2.0)] {
        let GuiLayoutFixtureKind::Group { members } = &item(copy).kind else {
            panic!("Expected group")
        };
        let [member] = members[..] else {
            panic!("Expected one copied member")
        };
        let GuiLayoutFixtureKind::Fixture {
            definition,
            transform,
        } = &item(member).kind
        else {
            panic!("Expected fixture")
        };
        assert!(matches!(definition, GuiFixtureSource::Inline { elements } if elements.len() == 1));
        assert_eq!(transform.position.x_meters, x);
        assert_eq!(transform.position.y_meters, y);
    }
    let after = state.project_session().unwrap();
    assert_eq!(
        after.project.definitions().fixtures,
        before.project.definitions().fixtures
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
    crate::desktop_foundation_tests::tests::assert_reloads(&root, &after);
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
    crate::desktop_foundation_tests::tests::assert_reloads(
        &root,
        &state.project_session().unwrap(),
    );
}

/// Replace the root document with `imports` and a `Project show` holding
/// `fields` after its metadata and description.
fn write_root_content(root: &camino::Utf8Path, imports: &str, fields: &str) {
    let metadata = donder_project_io::ProjectMetadata::read(root).unwrap();
    let text = format!(
        "{imports}\nProject show {{\n  format: {},\n  id: \"{}\",\n  description: none,\n{fields}\n}}\n",
        metadata.format_version, metadata.project_id
    );
    std::fs::write(root.join(donder_project_io::PROJECT_ROOT_FILE), text).unwrap();
}

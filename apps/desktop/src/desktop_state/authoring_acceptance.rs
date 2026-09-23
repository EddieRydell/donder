use super::DesktopState;
use crate::dto::*;
use crate::project::{new_project_files, write_new_project_files};
use camino::Utf8PathBuf;

pub(super) fn edit_layout(state: &DesktopState, edit: LayoutGuiEdit) -> GuiEditResult {
    let session = state.project_session().unwrap();
    let layout = session.project.setups[&session.project.root.setup]
        .layout
        .clone();
    state.open_file_path(layout.0.document().as_str());
    state.apply_gui_edit(
        GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: layout.0.document().to_string(),
            view: DocumentViewId::Layout,
            object_key: Some(layout.0.object().to_string()),
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

fn fixture_edit(
    state: &DesktopState,
    reference: &GuiObjectRef,
    edit: FixtureGuiEdit,
) -> GuiDocument {
    state.open_file_path(&reference.path);
    state
        .apply_gui_edit(
            GuiDocumentRequest {
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
    let session = state.project_session().unwrap();
    let id = &session.project.root.setup;
    state.open_file_path(id.0.document().as_str());
    match state
        .apply_gui_edit(
            GuiDocumentRequest {
                project_revision: state.snapshot().project_revision,
                path: id.0.document().to_string(),
                object_key: Some(id.0.object().into()),
                view: DocumentViewId::Setup,
            },
            GuiEditCommand::Setup { edit },
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
fn inline_definition_edits_share_instance_geometry_and_persist() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_project_files("Composition").unwrap()).unwrap();
    std::fs::write(
        root.join("layouts/main.layout.donder"),
        r#"
assembly:
  type: fixture
  pixels:
  - id: 11
    diameter: 0.01
    position: { x: 1, y: 0, z: 0 }
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
        project_revision: state.snapshot().project_revision,
        path: "layouts/main.layout.donder".into(),
        object_key: Some("main".into()),
        view: DocumentViewId::Layout,
    };
    state.open_file_path("layouts/main.layout.donder");
    let layout = layout_document(state.get_gui_document(layout_request()).document);
    let GuiLayoutFixtureKind::Fixture {
        definition: assembly,
        ..
    } = &layout.fixtures[0].kind
    else {
        panic!("instance missing")
    };
    assert_eq!(assembly.path, layout.path);
    assert!(matches!(
        fixture_edit(
            &state,
            assembly,
            FixtureGuiEdit::MovePixel {
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
    let reloaded = donder_project_io::load_package(&root).unwrap().session;
    assert_eq!(reloaded.project, accepted.project);
    assert_eq!(
        reloaded.source.documents.len(),
        accepted.source.documents.len()
    );
}

#[test]
fn empty_project_authors_shared_fixtures_routes_effect_and_reopens_without_yaml_edits() {
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
    let initial_color = initial.project.sequences[&sequence_id].layers[0]
        .color
        .to_hex();
    let layout = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::AddDefinition {
                name: "Strip".into(),
                parent: None,
            },
        )
        .document,
    );
    assert_eq!(layout.fixtures.len(), 1);
    assert!(layout.render_plan.pixels.is_empty());
    let GuiLayoutFixtureKind::Fixture { definition, .. } = &layout.fixtures[0].kind else {
        panic!("fixture missing")
    };
    let definition = definition.clone();
    assert_eq!(definition.path, layout.path);
    let pixels = (1..=3)
        .map(|id| GuiPixel {
            id,
            position: transform((id - 1) as f32).position,
            diameter_meters: 0.01,
        })
        .collect::<Vec<_>>();
    let GuiDocument::Fixture { document } = fixture_edit(
        &state,
        &definition,
        FixtureGuiEdit::SetPixels {
            pixels: pixels.clone(),
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
            project_revision: state.snapshot().project_revision,
            path: sequence_id.0.document().to_string(),
            object_key: Some(sequence_id.0.object().into()),
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
    let result = fixture_edit(
        &state,
        &definition,
        FixtureGuiEdit::SetPixels {
            pixels: vec![GuiPixel {
                id: 1,
                position: transform(0.0).position,
                diameter_meters: 0.0,
            }],
        },
    );
    assert!(matches!(result, GuiDocument::Blocked { .. }));
    assert!(std::sync::Arc::ptr_eq(
        &before_invalid,
        &state.project_session().unwrap()
    ));
    let mut reordered = pixels;
    reordered.reverse();
    let GuiDocument::Fixture { document } = fixture_edit(
        &state,
        &definition,
        FixtureGuiEdit::SetPixels { pixels: reordered },
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
    let before_copy = state.project_session().unwrap();
    setup_edit(&state, SetupGuiEdit::CopyLayout);
    let final_session = state.project_session().unwrap();
    let copied_layout = &final_session.project.setups[&setup_id].layout;
    assert_ne!(*copied_layout, before_copy.project.setups[&setup_id].layout);
    assert_eq!(
        final_session.project.definitions.fixtures,
        before_copy.project.definitions.fixtures
    );
    assert_eq!(
        final_session.project.sequences[&sequence_id].effects[0]
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
        &sequence_id,
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
        donder_project_io::load_package(&root)
            .unwrap()
            .session
            .project,
        final_session.project
    );
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
        "layouts/outputs.layout.donder",
        "patches/outputs.patch.donder",
        "setups/main.setup.donder",
        "fixtures/vertical.fixture.donder",
    ];
    let mut originals = BTreeMap::new();
    for path in paths {
        let bytes = std::fs::read(starter.join(path)).unwrap();
        std::fs::create_dir_all(library.join(path).parent().unwrap()).unwrap();
        std::fs::write(library.join(path), &bytes).unwrap();
        originals.insert(path, bytes);
    }
    let mut manifest = donder_package::PackageManifest::read(&starter).unwrap();
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
            donder_package::ExportGroup {
                documents: if name == "layout" {
                    vec![path.into(), paths[3].into()]
                } else {
                    vec![path.into()]
                },
            },
        )
    })
    .collect();
    manifest.write(&library).unwrap();
    let mut project_manifest = donder_package::PackageManifest::read(&root).unwrap();
    project_manifest.dependencies.insert(
        "rig".into(),
        donder_package::Dependency::Path { path: "rig".into() },
    );
    project_manifest.write(&root).unwrap();
    let registry = donder_package::Lockfile::read(&root).unwrap().registry;
    donder_package::Lockfile::from_directory(&project_manifest, &root, registry)
        .unwrap()
        .write(&root)
        .unwrap();
    std::fs::write(root.join("setups/main.setup.donder"), "imports:\n- from: { dependency: rig, export: layout }\n  as: layout\n- from: { dependency: rig, export: patch }\n  as: patch\n- from: { dependency: rig, export: controllers }\n  as: controllers\nmain:\n  type: setup\n  layout: layout.outputs_layout\n  patch: patch.outputs\n  controllers: [controllers.output_controller]\n").unwrap();
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
    assert!(document.layout_read_only && document.patch_read_only);
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
    assert!(!document.layout_read_only && !document.patch_read_only);
    assert!(!document.controllers[0].read_only);
    let copied_setup = &copied.project.setups[&setup_id];
    assert_eq!(copied.project.patches[&copied_setup.patch].routes.len(), 30);
    assert_eq!(
        copied.project.layouts[&copied_setup.layout]
            .iter_fixtures()
            .count(),
        31
    );
    assert_eq!(
        copied.project.definitions.fixtures,
        original.project.definitions.fixtures
    );
    let imported_request = GuiDocumentRequest {
        path: "rig/setups/main.setup.donder".into(),
        ..request()
    };
    assert!(matches!(
        state.get_gui_document(imported_request).document,
        GuiDocument::Setup { .. }
    ));
    let old_setup = &original.project.setups[&setup_id];
    assert_eq!(
        copied.project.layouts[&old_setup.layout],
        original.project.layouts[&old_setup.layout]
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
    let request = GuiDocumentRequest {
        project_revision: state.snapshot().project_revision,
        path: copied_setup.layout.0.document().to_string(),
        object_key: Some(copied_setup.layout.0.object().into()),
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
        donder_project_io::load_package(&root)
            .unwrap()
            .session
            .project,
        saved.project
    );
    for (path, bytes) in originals {
        assert_eq!(std::fs::read(library.join(path)).unwrap(), bytes);
    }
}

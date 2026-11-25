use super::DesktopState;
use super::gui_editing::GuiEditResult;
use crate::project::{new_test_project_files, write_new_project_files};
use camino::Utf8PathBuf;
use donder_editor::dto::{
    CompositionGuiEdit, ConsoleAction, ConsoleControlRef, ConsoleEventKind, ConsoleGuiDocument,
    ConsoleGuiEdit, ConsolePlayLength, FixtureGuiDocument, FixtureGuiEdit, FixtureTarget,
    GuiControlInput, GuiDocument, GuiDocumentRequest, GuiEdit, GuiField, GuiFixtureElement,
    GuiFixtureShape, GuiInputControl, GuiInputControlKind, GuiInputDriver, GuiLayoutFixture,
    GuiLayoutFixtureKind, GuiMember, GuiObjectRef, GuiPixelEncoding, GuiPixelRoute, GuiSite,
    GuiUniverses, GuiValue, LayoutGuiDocument, LayoutGuiEdit, NewClipContent, Point3Meters,
    ProjectGuiDocument, ProjectGuiEdit, Rotation3Degrees, Scale3, SequenceClipScope,
    SequenceEffectReference, SequenceGraphEdge, SequenceGraphNodeKind, SequenceGraphOperator,
    SequenceGuiEdit, SequenceParamValue, SetupControllerConfig, Transform,
};

pub(super) fn edit_layout(state: &DesktopState, edit: LayoutGuiEdit) -> GuiEditResult {
    let session = state.project_session().unwrap();
    let layout = session.project().root().layout;
    state.open_file_path(address(state, layout).document().as_str());
    state.apply_gui_edit(
        GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: address(state, layout).document().to_string(),
            object_key: address(state, layout).object().to_string(),
        },
        GuiEdit::Layout { edit },
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
    definition.clone()
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
                object_key: reference.object_key.clone(),
            },
            GuiEdit::Fixture { edit },
        )
        .document
}

fn project_edit(state: &DesktopState, edit: ProjectGuiEdit) -> ProjectGuiDocument {
    project_command(state, GuiEdit::Project { edit })
}

fn project_command(state: &DesktopState, command: GuiEdit) -> ProjectGuiDocument {
    let session = state.project_session().unwrap();
    let id = address(state, session.project().root().id);
    state.open_file_path(id.document().as_str());
    match state
        .apply_gui_edit(
            GuiDocumentRequest {
                project_revision: state.snapshot().project_revision,
                path: id.document().to_string(),
                object_key: id.object().into(),
            },
            command,
        )
        .document
    {
        GuiDocument::Project { document } => document,
        other => panic!("project edit rejected: {other:?}"),
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
fn empty_project_authors_shared_fixtures_routes_effect_and_reopens_without_text_edits() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_test_project_files("Acceptance").unwrap()).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let initial = state.project_session().unwrap();
    let sequence_id = *initial
        .project()
        .reusable::<donder_model::Sequence>()
        .get_index(0)
        .unwrap()
        .0;
    let initial_color = initial
        .project()
        .get::<donder_model::Sequence>(sequence_id)
        .unwrap()
        .composition
        .layers[0]
        .color
        .to_hex();
    let layout = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::AddDefinition {
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
                members: [layout.fixtures[0].id, second.id]
                    .map(|id| donder_editor::dto::GuiGroupMember::Item { id })
                    .to_vec(),
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
    let setup = project_edit(
        &state,
        ProjectGuiEdit::AddController {
            name: "controller".into(),
            config: SetupControllerConfig::E131 {
                sender_name: "Test".into(),
                bind_address: "0.0.0.0".into(),
                priority: 100,
                destination: None,
                universes: GuiUniverses {
                    start: 1,
                    count: 1,
                    channels: 21,
                },
                fseq_channel: None,
            },
            ports: vec![donder_editor::dto::SetupControllerPort {
                id: 1,
                name: "1".into(),
                description: None,
                donder: None,
            }],
        },
    );
    let routes = vec![
        GuiPixelRoute {
            id: 1,
            fixture: 1,
            pixels: None,
            reverse: false,
            controller: setup.controllers[0].source_ref.clone(),
            port: 1,
            start: donder_editor::dto::GuiRouteStart::Channel { channel: 0 },
            encoding: GuiPixelEncoding::Rgbw {
                order: [1, 0, 2, 3],
            },
            gamma: 1.0,
            brightness: 1.0,
            dither: false,
        },
        GuiPixelRoute {
            id: 2,
            fixture: 2,
            pixels: None,
            reverse: false,
            controller: setup.controllers[0].source_ref.clone(),
            port: 1,
            start: donder_editor::dto::GuiRouteStart::Channel { channel: 12 },
            encoding: GuiPixelEncoding::Rgb { order: [1, 0, 2] },
            gamma: 1.0,
            brightness: 1.0,
            dither: false,
        },
    ];
    project_command(
        &state,
        GuiEdit::put(GuiSite::of(None, None), GuiValue::Routes { routes }),
    );
    state.open_file_path(address(&state, sequence_id).document().as_str());
    let result = state.apply_gui_edit(
        GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: address(&state, sequence_id).document().to_string(),
            object_key: address(&state, sequence_id).object().into(),
        },
        GuiEdit::Sequence {
            edit: SequenceGuiEdit::AddClip {
                target: FixtureTarget { fixture: 1 },
                start_seconds: 0.0,
                duration_seconds: 1.0,
                content: NewClipContent::Effect {
                    initial_color,
                    effect: SequenceEffectReference {
                        path: "effects/standard.donder".into(),
                        effect_name: "Pulse".into(),
                    },
                    scope: SequenceClipScope::PerFixture,
                },
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
    let final_session = state.project_session().unwrap();
    let prepared = donder_elaboration::prepare(
        final_session.project(),
        final_session
            .project()
            .reusable::<donder_model::Sequence>()
            .get_index(0)
            .unwrap()
            .0,
        donder_elaboration::Retain::All,
    )
    .unwrap();
    let mut playback = prepared.into_playback();
    let mut illuminated = false;
    for frame in 0..60 {
        let rendered =
            playback.evaluate(donder_runtime_types::sample_time_from_frame(frame, 60).unwrap());
        let slots = rendered.outputs().next().unwrap().bytes;
        illuminated |= slots[..12].iter().any(|&value| value != 0);
        assert!(slots[12..].iter().all(|&value| value == 0));
    }
    assert!(illuminated);
    state.save_all().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(&root, &final_session);
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
        state.project_session().unwrap().project(),
        before_conversion.project()
    );
    state.redo_active_edit();
    assert_eq!(
        state.project_session().unwrap().project(),
        after_conversion.project()
    );
    state.save_all().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(&root, &after_conversion);
}

#[test]
fn fixture_storage_and_removal_preserve_shared_data_and_undo() {
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
                parent: None,
                transform: transform(0.0),
            },
        )
        .document,
    );
    let definition = fixture_reference(&layout, 0);
    assert_eq!(
        definition.path.replace('\\', "/"),
        "fixtures/my_strip.data.donder"
    );
    let mut second = layout.fixtures[0].clone();
    second.id = 2;
    second.name = "Shared placement".into();
    let group = GuiLayoutFixture {
        id: 3,
        name: "Group".into(),
        description: None,
        kind: GuiLayoutFixtureKind::Group {
            members: vec![donder_editor::dto::GuiGroupMember::Item { id: 2 }],
        },
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
            .project()
            .reusable::<donder_model::FixtureDefinition>()
            .len(),
        1
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
        after
            .project()
            .reusable::<donder_model::FixtureDefinition>()
            .len(),
        1
    );
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *before);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *after);
    state.save_all().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(&root, &after);
    let text = fs_err::read_to_string(root.join("project.data.donder")).unwrap();
    assert!(!text.contains("fixture_1"));
    assert!(root.join("fixtures/my_strip.data.donder").exists());
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
    let id = *initial
        .project()
        .reusable::<donder_model::Sequence>()
        .get_index(0)
        .unwrap()
        .0;
    let color = initial
        .project()
        .get::<donder_model::Sequence>(id)
        .unwrap()
        .composition
        .layers[0]
        .color
        .to_hex();
    state.open_file_path(address(&state, id).document().as_str());
    let request = || GuiDocumentRequest {
        project_revision: state.snapshot().project_revision,
        path: address(&state, id).document().to_string(),
        object_key: address(&state, id).object().to_string(),
    };
    let GuiDocument::Sequence { document } = state.get_gui_document(request()).document else {
        panic!("sequence projection missing");
    };
    let hue_shift = document.composition_graph.operator_catalog.iter().find(|entry| {
            matches!(&entry.operator, SequenceGraphOperator { object_key, .. } if object_key == "HueShift")
        }).expect("new project catalog includes HueShift");
    assert_eq!(hue_shift.operator.path, "operators/standard.donder");
    let edit = |edit| {
        state.apply_gui_edit(
            request(),
            GuiEdit::Sequence {
                edit: SequenceGuiEdit::Composition { edit },
            },
        )
    };
    let put = |site, value| state.apply_gui_edit(request(), GuiEdit::put(site, value));
    let connection = document.composition_graph.edges[0].clone();
    let inserted = edit(CompositionGuiEdit::AddGraphOperatorNode {
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
            .project()
            .get::<donder_model::Sequence>(id)
            .unwrap()
            .composition
            .graph
            .nodes
            .iter()
            .all(|candidate| !matches!(
                candidate.kind,
                donder_model::CompositionGraphNodeKind::Operator(_)
            ))
    );
    state.redo_active_edit();
    assert_eq!(
        state.project_session().unwrap().project(),
        disconnected.project()
    );
    let GuiDocument::Sequence { document } = put(
        GuiSite::of(
            Some(GuiMember::Node { id: node.id }),
            Some(GuiField::Param {
                name: "shift".into(),
            }),
        ),
        GuiValue::Param {
            value: SequenceParamValue::Float { value: 0.25 },
        },
    )
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
        matches!(params.iter().find(|param| param.name == "shift").unwrap().value, SequenceParamValue::Float { value } if value == 0.25)
    );
    let accepted = state.project_session().unwrap();
    state.save_all().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(&root, &accepted);
    let authored = fs_err::read_to_string(root.join(address(&state, id).document())).unwrap();
    assert!(authored.contains("operators/standard.donder"));
    assert!(authored.contains("scripts.HueShift"));
    assert!(matches!(
        edit(CompositionGuiEdit::ConnectGraphNodes {
            connection: SequenceGraphEdge {
                from_node: connection.from_node,
                to_node: node.id,
                input: Some("source".into()),
            },
        })
        .document,
        GuiDocument::Sequence { .. }
    ));
    assert!(matches!(
        edit(CompositionGuiEdit::ReconnectGraphEdge {
            previous: connection.clone(),
            connection: SequenceGraphEdge {
                from_node: node.id,
                to_node: connection.to_node,
                input: connection.input,
            },
        })
        .document,
        GuiDocument::Sequence { .. }
    ));
}

#[test]
fn duplicated_groups_own_geometry_and_preserve_original_sources() {
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
    let layout_id = current.project().root().layout;
    let request = GuiDocumentRequest {
        project_revision: state.snapshot().project_revision,
        path: address(&state, layout_id).document().to_string(),
        object_key: address(&state, layout_id).object().into(),
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
                    members: layout
                        .fixtures
                        .iter()
                        .map(|fixture| donder_editor::dto::GuiGroupMember::Item { id: fixture.id })
                        .collect(),
                },
            })
            .chain(layout.fixtures)
            .collect(),
            root: vec![2],
        },
    );
    let before = state.project_session().unwrap();
    let duplicate = layout_document(
        edit_layout(
            &state,
            LayoutGuiEdit::RepeatFixtures {
                ids: vec![2],
                offsets: vec![Point3Meters {
                    x_meters: 0.0,
                    y_meters: 0.0,
                    z_meters: 0.0,
                }],
            },
        )
        .document,
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
    assert_eq!(
        members,
        &[donder_editor::dto::GuiGroupMember::Item { id: 4 }]
    );
    let GuiLayoutFixtureKind::Fixture {
        definition,
        transform: placement,
    } = &duplicate.fixtures[3].kind
    else {
        panic!("Expected copied fixture")
    };
    // The copy places a copy of the definition, declared beside the layout.
    assert_eq!(definition.object_key, "original_strip_copy");
    assert_eq!(definition.path, duplicate.source_ref.path);
    assert_eq!(placement.position.x_meters, 2.0);
    assert_eq!(
        after
            .project()
            .reusable::<donder_model::FixtureDefinition>()
            .len(),
        before
            .project()
            .reusable::<donder_model::FixtureDefinition>()
            .len()
            + 1
    );
    let copied_source = definition.clone();
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *before);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *after);
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
        changed
            .project()
            .get::<donder_model::Layout>(layout_id)
            .unwrap()
            .fixtures[0],
        before
            .project()
            .get::<donder_model::Layout>(layout_id)
            .unwrap()
            .fixtures[0]
    );
    assert_eq!(
        changed
            .project()
            .reusable::<donder_model::FixtureDefinition>()
            .first(),
        before
            .project()
            .reusable::<donder_model::FixtureDefinition>()
            .first()
    );
    state.save_all().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(&root, &changed);
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
    let group = |id, name: &str, members: Vec<u32>| GuiLayoutFixture {
        id,
        name: name.into(),
        description: None,
        kind: GuiLayoutFixtureKind::Group {
            members: members
                .into_iter()
                .map(|id| donder_editor::dto::GuiGroupMember::Item { id })
                .collect(),
        },
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
    let layout_id = initial.project().root().layout;
    let fixture_id = donder_model::FixtureId;
    let original_first = initial
        .project()
        .get::<donder_model::Layout>(layout_id)
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
                .project()
                .get::<donder_model::Layout>(layout_id)
                .unwrap()
                .fixture(fixture_id(1))
                .unwrap(),
            &original_first
        );
        assert_eq!(
            after
                .project()
                .reusable::<donder_model::FixtureDefinition>(),
            initial
                .project()
                .reusable::<donder_model::FixtureDefinition>()
        );
        state.undo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *before_session);
        state.redo_active_edit();
        assert_eq!(*state.project_session().unwrap(), *after);
    }
    let current = state.project_session().unwrap();
    let layout = current
        .project()
        .get::<donder_model::Layout>(layout_id)
        .unwrap();
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
        object_key: first_source.object_key,
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
                    members: layout
                        .fixtures
                        .iter()
                        .map(|fixture| donder_editor::dto::GuiGroupMember::Item { id: fixture.id })
                        .collect(),
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
        let [donder_editor::dto::GuiGroupMember::Item { id: member }] = members[..] else {
            panic!("Expected one copied member")
        };
        let GuiLayoutFixtureKind::Fixture {
            definition,
            transform,
        } = &item(member).kind
        else {
            panic!("Expected fixture")
        };
        // Each copy places a copy of the definition, declared beside the layout.
        assert_eq!(definition.path, result.source_ref.path);
        let definition = state
            .project_session()
            .unwrap()
            .declared_id(&donder_project_io::SourceIdentity::from_document(
                donder_project_io::DocumentId::new(definition.path.as_str().into()),
                definition.object_key.clone(),
            ))
            .and_then(|id| donder_model::FixtureDefinitionId::try_from(id).ok())
            .unwrap();
        assert_eq!(
            state
                .project_session()
                .unwrap()
                .project()
                .reusable::<donder_model::FixtureDefinition>()[&definition]
                .elements
                .len(),
            1
        );
        assert_eq!(transform.position.x_meters, x);
        assert_eq!(transform.position.y_meters, y);
    }
    let after = state.project_session().unwrap();
    assert_eq!(
        after
            .project()
            .reusable::<donder_model::FixtureDefinition>()
            .len(),
        before
            .project()
            .reusable::<donder_model::FixtureDefinition>()
            .len()
            + 2
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
                    x_meters: 2146.0,
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

/// Where an object of the state's session is written.
fn address(
    state: &DesktopState,
    id: impl Into<donder_model::ObjectId>,
) -> donder_project_io::SourceIdentity {
    state.project_session().unwrap().address_of(id)
}

fn object_request(
    state: &DesktopState,
    id: impl Into<donder_model::ObjectId>,
) -> GuiDocumentRequest {
    let identity = &address(state, id);
    state.open_file_path(identity.document().as_str());
    GuiDocumentRequest {
        project_revision: state.snapshot().project_revision,
        path: identity.document().to_string(),
        object_key: identity.object().into(),
    }
}

fn console_edit(
    state: &DesktopState,
    console: &donder_model::ConsoleId,
    edit: ConsoleGuiEdit,
) -> ConsoleGuiDocument {
    match state
        .apply_gui_edit(object_request(state, *console), GuiEdit::Console { edit })
        .document
    {
        GuiDocument::Console { document } => document,
        other => panic!("console edit rejected: {other:?}"),
    }
}

#[test]
fn live_input_editors_author_devices_and_consoles_and_roundtrip() {
    let (_temporary, root) = crate::desktop_foundation_tests::tests::starter_copy();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    let session = state.project_session().unwrap();
    let keyboard = *session
        .project()
        .reusable::<donder_model::InputDevice>()
        .keys()
        .next()
        .unwrap();

    // Renaming a control renames the reference every console prints for it.
    let request = object_request(&state, keyboard);
    let GuiDocument::InputDevice { document } = state.get_gui_document(request.clone()).document
    else {
        panic!("input device projection missing");
    };
    assert_eq!(document.key_codes.len(), donder_model::KeyCode::ALL.len());
    let mut controls = document.device.controls.clone();
    controls[0].name = "hit".into();
    let renamed = state.apply_gui_edit(
        request,
        GuiEdit::put(
            GuiSite::of(None, None),
            GuiValue::InputDevice {
                driver: document.device.driver,
                controls: controls.clone(),
            },
        ),
    );
    let GuiDocument::InputDevice { document } = renamed.document else {
        panic!("control rename rejected: {:?}", renamed.document);
    };
    assert_eq!(document.device.controls[0].name, "hit");
    // Two controls cannot read one key.
    controls[1].input = controls[0].input.clone();
    let duplicate = state.apply_gui_edit(
        object_request(&state, keyboard),
        GuiEdit::put(
            GuiSite::of(None, None),
            GuiValue::InputDevice {
                driver: GuiInputDriver::Keyboard,
                controls,
            },
        ),
    );
    assert!(matches!(duplicate.document, GuiDocument::Blocked { .. }));

    // A new device gets its own file.
    let added = project_edit(
        &state,
        ProjectGuiEdit::AddInputDevice {
            name: "pads".into(),
            driver: GuiInputDriver::Keyboard,
            controls: vec![GuiInputControl {
                id: 1,
                name: "pad".into(),
                description: Some("The big pad.".into()),
                kind: GuiInputControlKind::Button,
                input: GuiControlInput::Key {
                    code: "KeyP".into(),
                },
            }],
        },
    );
    assert_eq!(added.input_devices.len(), 2);
    assert_eq!(added.input_devices[1].name, "pads");
    assert_eq!(
        added.input_devices[1].source_ref.path,
        "input_devices/pads.data.donder"
    );

    // A new console starts with the default layer drawn to the output.
    let project = project_edit(
        &state,
        ProjectGuiEdit::AddConsole {
            name: "live".into(),
            initial_color: "#38bdf8".into(),
        },
    );
    assert_eq!(project.consoles.len(), 2);
    let console = *state
        .project_session()
        .unwrap()
        .project()
        .reusable::<donder_model::Console>()
        .get_index(1)
        .unwrap()
        .0;
    let GuiDocument::Console { document } = state
        .get_gui_document(object_request(&state, console))
        .document
    else {
        panic!("console projection missing");
    };
    assert_eq!(document.layers.len(), 1);
    assert_eq!(document.composition_graph.nodes.len(), 2);
    assert_eq!(document.composition_graph.edges.len(), 1);
    assert_eq!(document.perform_index, 1);
    let pad = &document.input_devices[1];
    let effect = document
        .effect_definitions
        .iter()
        .find(|definition| definition.effect.effect_name == "Pulse")
        .unwrap()
        .effect
        .clone();
    let input = ConsoleControlRef {
        device: pad.device.clone(),
        control: pad.buttons[0].control,
    };
    let target = document.targets[0].target.clone();
    console_edit(
        &state,
        &console,
        ConsoleGuiEdit::Composition {
            edit: CompositionGuiEdit::CreateLayer {
                name: "Accents".into(),
                color: "#ff0000".into(),
            },
        },
    );
    let document = console_edit(
        &state,
        &console,
        ConsoleGuiEdit::AddClip {
            initial_color: "#ffffff".into(),
            effect,
            target,
        },
    );
    let clip = document.clips[0].id;
    state.apply_gui_edit(
        object_request(&state, console),
        GuiEdit::put(
            GuiSite::of(Some(GuiMember::Clip { id: clip }), Some(GuiField::Name)),
            GuiValue::Text {
                text: "Flash".into(),
            },
        ),
    );
    let play = |copies| ConsoleAction::Play {
        clip_id: clip,
        copies,
        length: ConsolePlayLength::Fixed {
            duration_seconds: 0.5,
        },
    };
    let before = state.project_session().unwrap();
    let document = console_edit(
        &state,
        &console,
        ConsoleGuiEdit::AddAction {
            control: input,
            event: ConsoleEventKind::Press,
            action: play(3),
        },
    );
    let event = document.events[0].id;
    assert!(matches!(
        document.events[0].actions[..],
        [ConsoleAction::Play { copies: 3, .. }]
    ));
    let after = state.project_session().unwrap();
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *before);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *after);
    // A clip plays at most 16 copies at once.
    let rejected = state.apply_gui_edit(
        object_request(&state, console),
        GuiEdit::put(
            GuiSite::of(Some(GuiMember::Event { id: event }), None),
            GuiValue::Actions {
                actions: vec![play(17)],
            },
        ),
    );
    assert!(matches!(rejected.document, GuiDocument::Blocked { .. }));
    // A played clip cannot also play for the whole session.
    let rejected = state.apply_gui_edit(
        object_request(&state, console),
        GuiEdit::put(
            GuiSite::of(Some(GuiMember::Clip { id: clip }), Some(GuiField::Always)),
            GuiValue::OptionalSeconds { seconds: Some(4.0) },
        ),
    );
    assert!(matches!(rejected.document, GuiDocument::Blocked { .. }));

    state.save_all().unwrap();
    let saved = state.project_session().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(&root, &saved);
    let jam = fs_err::read_to_string(root.join("consoles/jam.data.donder")).unwrap();
    assert!(jam.contains("devices.keyboard.hit"));
    let live = fs_err::read_to_string(root.join("consoles/live.data.donder")).unwrap();
    assert!(live.contains("name: flash"));
    assert!(live.contains("copies: 3"));
    assert!(live.contains("length: Fixed { duration: 0.5s }"));
}

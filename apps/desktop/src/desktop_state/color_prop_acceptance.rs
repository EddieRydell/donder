use super::DesktopState;
use super::advanced_patch_acceptance::accepted_layout;
use super::advanced_patch_acceptance::{accepted_elements, edit_elements};
use super::advanced_patch_acceptance::{accepted_patch, replace_patch};
use crate::dto::*;
use crate::project::{new_project_files, write_new_project_files};
use camino::Utf8PathBuf;

#[test]
fn rgbw_and_discrete_props_author_route_resize_duplicate_and_roundtrip() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
    write_new_project_files(&root, &new_project_files("Color props").unwrap()).unwrap();
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
    let point = Point3Meters {
        x_meters: 0.0,
        y_meters: 0.0,
        z_meters: 0.0,
    };
    let shape = |pixels| Geometry::Lines {
        points: vec![
            point.clone(),
            Point3Meters {
                x_meters: 2.0,
                ..point.clone()
            },
        ],
        pixels,
    };
    let edit = |edit| {
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
    let accepted = |command| match edit(command).document {
        GuiDocument::Setup { document } => document,
        other => panic!("{other:?}"),
    };
    state.open_file_path(setup.0.document().as_str());
    let rgbw = accepted_layout(
        &state,
        PreviewGuiEdit::AddPixelLight {
            light: SetupPixelLight {
                name: "RGBW".into(),
                parent: None,
                capability: GuiColorCapability::Rgbw,
                geometry: shape(3),
                bulb_diameter_meters: 0.04,
                position: point.clone(),
            },
        },
    )
    .hierarchy
    .elements[0]
        .id;
    let discrete = accepted_layout(
        &state,
        PreviewGuiEdit::AddPixelLight {
            light: SetupPixelLight {
                name: "Discrete".into(),
                parent: None,
                capability: GuiColorCapability::Discrete {
                    emitters: vec![
                        GuiDiscreteEmitter {
                            id: 4,
                            name: "A".into(),
                        },
                        GuiDiscreteEmitter {
                            id: 7,
                            name: "B".into(),
                        },
                    ],
                    mappings: vec![
                        GuiDiscreteColorMapping {
                            color: black,
                            levels: vec![
                                PatchGuiIndexedEntry { id: 4, value: 0.0 },
                                PatchGuiIndexedEntry { id: 7, value: 0.0 },
                            ],
                        },
                        GuiDiscreteColorMapping {
                            color: white.clone(),
                            levels: vec![
                                PatchGuiIndexedEntry { id: 4, value: 0.25 },
                                PatchGuiIndexedEntry { id: 7, value: 0.75 },
                            ],
                        },
                    ],
                },
                geometry: shape(1),
                bulb_diameter_meters: 0.04,
                position: point.clone(),
            },
        },
    )
    .hierarchy
    .elements[1]
        .id;
    let controller = accepted(SetupGuiEdit::AddController {
        config: SetupControllerConfig::E131 {
            source_name: "Color props".into(),
            bind_address: "0.0.0.0".into(),
            priority: 100,
            destination: Some("127.0.0.1".into()),
        },
        ports: vec![
            SetupControllerPort {
                id: 1,
                address: 1,
                slot_count: 8,
            },
            SetupControllerPort {
                id: 2,
                address: 2,
                slot_count: 8,
            },
            SetupControllerPort {
                id: 3,
                address: 3,
                slot_count: 2,
            },
        ],
    })
    .controllers[0]
        .source_ref
        .clone();
    for (node, port, order) in [(rgbw, 1, vec![1, 0, 2, 3]), (discrete, 3, vec![1, 0])] {
        accepted(SetupGuiEdit::AssignPixelOutput {
            node,
            controller: controller.clone(),
            first_port: port,
            start_slot: 0,
            component_order: order,
            mode: SetupOutputAssignmentMode::Add,
        });
    }
    let assigned = state.project_session().unwrap();
    let changed = accepted_elements(
        &state,
        ElementTreeGuiEdit::UpdateColorCapability {
            id: rgbw,
            capability: GuiColorCapability::Rgb,
            component_order: vec![1, 0, 2],
        },
    );
    assert!(matches!(
        changed
            .elements
            .iter()
            .find(|element| element.id == rgbw)
            .unwrap()
            .capability,
        Some(GuiColorCapability::Rgb)
    ));
    let rgb_session = state.project_session().unwrap();
    let patch_id = &rgb_session.project.setups[&setup].patch;
    let slots = rgb_session.project.patches[patch_id]
        .nodes
        .values()
        .filter_map(|node| {
            if let dawn_language::patch::PatchNode::Sink(sink) = node {
                Some(sink.slot_count)
            } else {
                None
            }
        })
        .sum::<u16>();
    assert_eq!(slots, 11);
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *assigned);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *rgb_session);
    edit_elements(
        &state,
        ElementTreeGuiEdit::UpdateColorCapability {
            id: discrete,
            capability: GuiColorCapability::Rgbw,
            component_order: vec![0, 1, 2, 3],
        },
    );
    assert!(std::sync::Arc::ptr_eq(
        &rgb_session,
        &state.project_session().unwrap()
    ));
    edit_elements(
        &state,
        ElementTreeGuiEdit::UpdateColorCapability {
            id: rgbw,
            capability: GuiColorCapability::Rgbw,
            component_order: vec![0, 1, 2],
        },
    );
    assert!(std::sync::Arc::ptr_eq(
        &rgb_session,
        &state.project_session().unwrap()
    ));
    accepted_elements(
        &state,
        ElementTreeGuiEdit::UpdateColorCapability {
            id: rgbw,
            capability: GuiColorCapability::Rgbw,
            component_order: vec![1, 0, 2, 3],
        },
    );
    let assigned = state.project_session().unwrap();
    edit(SetupGuiEdit::AssignPixelOutput {
        node: rgbw,
        controller: controller.clone(),
        first_port: 1,
        start_slot: 0,
        component_order: vec![0, 1, 2],
        mode: SetupOutputAssignmentMode::Replace,
    });
    assert!(std::sync::Arc::ptr_eq(
        &assigned,
        &state.project_session().unwrap()
    ));
    let fixture_edit = |geometry| {
        let session = state.project_session().unwrap();
        let layout = &session.project.preview_layouts[&session.project.setups[&setup].preview];
        let fixture = &layout
            .props
            .iter()
            .find(|prop| prop.bindings.iter().any(|binding| binding.node.0 == rgbw))
            .unwrap()
            .definition;
        let request = state
            .resolve_gui_source(
                &fixture.0.module_id().to_string(),
                fixture.0.document().as_str(),
                fixture.0.object(),
            )
            .unwrap();
        state.open_file_path(&request.path);
        state.apply_gui_edit(
            request,
            GuiEditCommand::Prop {
                edit: PropGuiEdit::UpdateDefinition {
                    geometry,
                    bulb_diameter_meters: 0.04,
                },
            },
        )
    };
    assert!(matches!(
        fixture_edit(shape(4)).document,
        GuiDocument::Prop { .. }
    ));
    let resized = state.project_session().unwrap();
    state.undo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *assigned);
    state.redo_active_edit();
    assert_eq!(*state.project_session().unwrap(), *resized);
    fixture_edit(shape(5));
    assert!(std::sync::Arc::ptr_eq(
        &resized,
        &state.project_session().unwrap()
    ));
    let placement_id = {
        let session = state.project_session().unwrap();
        session.project.preview_layouts[&session.project.setups[&setup].preview]
            .props
            .iter()
            .find(|prop| prop.bindings.iter().any(|binding| binding.node.0 == rgbw))
            .unwrap()
            .id
            .0
    };
    let duplicate = accepted_layout(
        &state,
        PreviewGuiEdit::DuplicatePlacement { id: placement_id },
    );
    assert!(matches!(
        duplicate.hierarchy.elements.last().unwrap().capability,
        Some(GuiColorCapability::Rgbw)
    ));
    assert_eq!(
        duplicate
            .hierarchy
            .elements
            .last()
            .unwrap()
            .color_component_count,
        Some(4)
    );
    let reused = accepted_layout(
        &state,
        PreviewGuiEdit::PlaceFixture {
            name: "Existing fixture".into(),
            parent: None,
            capability: GuiColorCapability::Rgbw,
            definition: duplicate.fixtures[0].definition_ref.clone(),
            position: point.clone(),
        },
    );
    let reused_placement = reused.fixtures.last().unwrap().id;
    let reused_node = reused.hierarchy.elements.last().unwrap().id;
    let after_duplicate = state.project_session().unwrap();
    let layout =
        &after_duplicate.project.preview_layouts[&after_duplicate.project.setups[&setup].preview];
    assert_eq!(
        layout.props[0].definition,
        layout.props.last().unwrap().definition
    );
    let definition_count = after_duplicate.project.definitions.props.definitions.len();
    assert!(matches!(
        fixture_edit(shape(3)).document,
        GuiDocument::Prop { .. }
    ));
    let smaller = state.project_session().unwrap();
    let layout = &smaller.project.preview_layouts[&smaller.project.setups[&setup].preview];
    for placement in layout
        .props
        .iter()
        .filter(|placement| placement.definition == layout.props[0].definition)
    {
        assert_eq!(placement.bindings.len(), 3);
        assert_eq!(
            smaller.project.element_trees[&layout.element_tree].nodes[&placement.bindings[0].node]
                .kind
                .cell_count(),
            Some(3)
        );
    }
    assert_eq!(
        smaller.project.definitions.props.definitions.len(),
        definition_count
    );
    assert!(matches!(
        fixture_edit(shape(4)).document,
        GuiDocument::Prop { .. }
    ));
    let removed = accepted_layout(
        &state,
        PreviewGuiEdit::RemovePlacement {
            id: reused_placement,
        },
    );
    assert!(
        !removed
            .fixtures
            .iter()
            .any(|placement| placement.id == reused_placement)
    );
    assert!(
        removed
            .hierarchy
            .elements
            .iter()
            .any(|node| node.id == reused_node)
    );
    let mut capability = duplicate
        .hierarchy
        .elements
        .iter()
        .find(|element| element.id == discrete)
        .unwrap()
        .capability
        .clone()
        .unwrap();
    let GuiColorCapability::Discrete { mappings, .. } = &mut capability else {
        unreachable!()
    };
    mappings
        .iter_mut()
        .find(|mapping| mapping.color == white)
        .unwrap()
        .levels[0]
        .value = 0.5;
    accepted_elements(
        &state,
        ElementTreeGuiEdit::UpdateColorCapability {
            id: discrete,
            capability,
            component_order: vec![1, 0],
        },
    );
    state.open_file_path(sequence.0.document().as_str());
    let sequence_edit = |edit| {
        let result = state.apply_gui_edit(
            GuiDocumentRequest {
                project_revision: state.snapshot().project_revision,
                path: sequence.0.document().to_string(),
                view: DocumentViewId::Sequence,
                object_key: Some(sequence.0.object().into()),
            },
            GuiEditCommand::Sequence { edit },
        );
        assert!(
            matches!(result.document, GuiDocument::Sequence { .. }),
            "{:?}",
            result.document
        );
    };
    for node in [rgbw, discrete] {
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
        let id = state.project_session().unwrap().project.sequences[&sequence]
            .effects
            .last()
            .unwrap()
            .id
            .0;
        sequence_edit(SequenceGuiEdit::UpdateEffectParam {
            id,
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
    }
    let final_session = state.project_session().unwrap();
    let output = dawn_elaboration::PreparedSequenceOutput::prepare(
        &final_session.project,
        &setup,
        &sequence,
    )
    .unwrap();
    let frame = output.render_seconds(0.5).unwrap();
    assert_eq!(
        frame
            .controller_frames
            .iter()
            .flat_map(|port| port.slots.iter().copied())
            .collect::<Vec<_>>(),
        vec![
            0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 191, 128
        ]
    );
    let idle = output.render_seconds(60.0).unwrap();
    assert!(
        idle.controller_frames
            .iter()
            .all(|port| port.slots.iter().all(|value| *value == 0))
    );
    let mut missing_black = final_session.project.clone();
    let patch_id = missing_black.setups[&setup].patch.clone();
    for node in missing_black
        .patches
        .get_mut(&patch_id)
        .unwrap()
        .nodes
        .values_mut()
    {
        if let dawn_language::patch::PatchNode::Filter(
            dawn_language::patch::FilterDefinition::ColorBreakdown {
                capability: dawn_language::element::ColorCapability::Discrete { mappings, .. },
                ..
            },
        ) = node
        {
            mappings.retain(|mapping| {
                mapping.color
                    != dawn_language::values::Color::from_hex(&css_color(
                        "--dawn-default-project-color:",
                    ))
                    .unwrap()
            });
        }
    }
    assert!(
        dawn_language::validation::validate_project(&missing_black)
            .unwrap_err()
            .to_string()
            .contains("black mapping")
    );
    state.save_all().unwrap();
    let reloaded = dawn_project_io::load_package(&root).unwrap().session;
    assert_eq!(reloaded.project, final_session.project);
    let reopened =
        dawn_elaboration::PreparedSequenceOutput::prepare(&reloaded.project, &setup, &sequence)
            .unwrap()
            .render_seconds(0.5)
            .unwrap();
    assert_eq!(reopened.controller_frames, frame.controller_frames);

    // Edit the projected graph through the same atomic replacement used by the advanced editor.
    state.open_file_path(setup.0.document().as_str());
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
    let mut reordered = 0;
    for node in &mut nodes {
        if let PatchGuiNodeDefinition::Filter {
            filter: PatchGuiFilter::ComponentReorder { order, .. },
        } = &mut node.definition
        {
            order.reverse();
            reordered += 1;
        }
    }
    assert_eq!(reordered, 3);
    let mut invalid = nodes.clone();
    for node in &mut invalid {
        if let PatchGuiNodeDefinition::Filter {
            filter: PatchGuiFilter::ComponentReorder { order, .. },
        } = &mut node.definition
        {
            order.fill(0);
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
        rendered
            .render_seconds(0.5)
            .unwrap()
            .controller_frames
            .iter()
            .flat_map(|port| port.slots.iter().copied())
            .collect::<Vec<_>>(),
        [
            255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 128, 191
        ]
    );
    state.save_all().unwrap();
    let reloaded = dawn_project_io::load_package(&root).unwrap().session;
    assert_eq!(reloaded.project, advanced.project);
    let reopened =
        dawn_elaboration::PreparedSequenceOutput::prepare(&reloaded.project, &setup, &sequence)
            .unwrap();
    for time in [0.5, 60.0] {
        assert_eq!(
            reopened.render_seconds(time).unwrap().controller_frames,
            rendered.render_seconds(time).unwrap().controller_frames
        );
    }
}

#[test]
fn library_parameter_arrays_preserve_links_when_editing_and_saving() {
    use crate::desktop_foundation_tests::tests::starter_copy;
    use dawn_project_io::load_package;
    use std::fs;
    let (_temporary, root) = starter_copy();
    fs::write(
        root.join("effects/array-values.effect.dawn"),
        r#"
        effect ArrayValues {
            param array<curve> shapes;
            param array<gradient> colors;
            color sample() { return rgb(progress(), progress(), progress()); }
        }
    "#,
    )
    .unwrap();
    let path = "sequences/empty.sequence.dawn";
    let original = fs::read_to_string(root.join(path)).unwrap();
    fs::write(root.join(path), format!("imports:\n- from: {{ documents: [effects/array-values.effect.dawn] }}\n  as: effects\n{original}")).unwrap();
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    state.open_file_path(path);
    let request = || GuiDocumentRequest {
        project_revision: state.snapshot().project_revision,
        path: path.into(),
        view: DocumentViewId::Sequence,
        object_key: Some("empty".into()),
    };
    let GuiDocument::Sequence { document } = state.get_gui_document(request()).document else {
        panic!("sequence unavailable");
    };
    let definition = document
        .effect_definitions
        .iter()
        .find(|item| matches!(&item.effect, SequenceEffectReference::Custom { effect_name, .. } if effect_name == "ArrayValues"))
        .unwrap();
    let edit = |edit| {
        let result = state.apply_gui_edit(request(), GuiEditCommand::Sequence { edit });
        let GuiDocument::Sequence { document } = result.document else {
            panic!("{result:?}");
        };
        document
    };
    let document = edit(SequenceGuiEdit::AddEffect {
        initial_color: document.gradient_library[0].stops[0].value.clone(),
        effect: definition.effect.clone(),
        target: document.lanes[0].target.clone(),
        scope: SequenceEffectScope::PerFixture,
        start_seconds: 0.0,
        mark_collection_key: None,
    });
    let effect_id = document.effects[0].id;
    edit(SequenceGuiEdit::UpdateEffectParam {
        id: effect_id,
        name: "shapes".into(),
        value: SequenceEffectParamValue::CurveArray { values: vec![] },
    });
    let document = edit(SequenceGuiEdit::UpdateEffectParam {
        id: effect_id,
        name: "colors".into(),
        value: SequenceEffectParamValue::GradientArray { values: vec![] },
    });
    let effect = &document.effects[0];
    assert!(
        matches!(&effect.params[0].value, SequenceEffectParamValue::CurveArray { values } if values.is_empty())
    );
    assert!(
        matches!(&effect.params[1].value, SequenceEffectParamValue::GradientArray { values } if values.is_empty())
    );
    let curve = &document.curve_library[0];
    let gradient = &document.gradient_library[0];
    let source = |module_id: &str, path: &str, object_key: &str, display_name: &str| {
        SequenceLibrarySource::Library {
            module_id: module_id.into(),
            path: path.into(),
            object_key: object_key.into(),
            display_name: display_name.into(),
        }
    };
    let curve = SequenceCurveValue {
        points: curve.points.clone(),
        source: source(
            &curve.module_id,
            &curve.path,
            &curve.object_key,
            &curve.display_name,
        ),
    };
    let gradient = SequenceGradientValue {
        stops: gradient.stops.clone(),
        source: source(
            &gradient.module_id,
            &gradient.path,
            &gradient.object_key,
            &gradient.display_name,
        ),
    };
    for (name, value) in [
        (
            "shapes",
            SequenceEffectParamValue::CurveArray {
                values: vec![curve.clone(), curve],
            },
        ),
        (
            "colors",
            SequenceEffectParamValue::GradientArray {
                values: vec![gradient.clone(), gradient],
            },
        ),
    ] {
        let document = edit(SequenceGuiEdit::UpdateEffectParam {
            id: effect.id,
            name: name.into(),
            value,
        });
        let mut value = document.effects[0]
            .params
            .iter()
            .find(|param| param.name == name)
            .unwrap()
            .value
            .clone();
        match &mut value {
            SequenceEffectParamValue::CurveArray { values } => {
                values[0].source = SequenceLibrarySource::Inline;
                values[0].points[0].value = 0.25;
            }
            SequenceEffectParamValue::GradientArray { values } => {
                values[0].source = SequenceLibrarySource::Inline;
                values[0].stops[0].value = values[0].stops.last().unwrap().value.clone();
            }
            _ => panic!("wrong array editor"),
        }
        let document = edit(SequenceGuiEdit::UpdateEffectParam {
            id: effect.id,
            name: name.into(),
            value,
        });
        let value = &document.effects[0]
            .params
            .iter()
            .find(|param| param.name == name)
            .unwrap()
            .value;
        let (inline, linked) = match value {
            SequenceEffectParamValue::CurveArray { values } => {
                (&values[0].source, &values[1].source)
            }
            SequenceEffectParamValue::GradientArray { values } => {
                (&values[0].source, &values[1].source)
            }
            _ => panic!("wrong array editor"),
        };
        assert!(matches!(inline, SequenceLibrarySource::Inline));
        assert!(matches!(linked, SequenceLibrarySource::Library { .. }));
    }
    state.save_all().unwrap();
    assert_eq!(
        load_package(&root).unwrap().session.project,
        state.project_session().unwrap().project
    );
}

use super::DesktopState;
use crate::dto::*;

#[test]
fn library_parameter_arrays_preserve_links_when_editing_and_saving() {
    use crate::desktop_foundation_tests::tests::starter_copy;
    use donder_project_io::load_package;
    use std::fs;
    let (_temporary, root) = starter_copy();
    fs::write(
        root.join("effects/array-values.effect.donder"),
        r#"
        effect ArrayValues {
            param array<curve> shapes;
            param array<gradient> colors;
            color sample() { return rgb(progress(), progress(), progress()); }
        }
    "#,
    )
    .unwrap();
    let path = "sequences/empty.sequence.donder";
    let original = fs::read_to_string(root.join(path)).unwrap();
    fs::write(root.join(path), format!("imports:\n- from: {{ documents: [effects/array-values.effect.donder] }}\n  as: effects\n{original}")).unwrap();
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

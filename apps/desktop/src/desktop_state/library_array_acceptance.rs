use super::DesktopState;
use donder_editor::dto::{
    GuiDocument, GuiDocumentRequest, GuiEdit, GuiField, GuiMember, GuiSite, GuiValue,
    NewClipContent, SequenceClipScope, SequenceCurveValue, SequenceGradientValue, SequenceGuiEdit,
    SequenceLibraryLink, SequenceParamValue,
};

#[test]
fn library_parameter_arrays_preserve_links_when_editing_and_saving() {
    use crate::desktop_foundation_tests::tests::starter_copy;
    use donder_project_io::check_project;
    use fs_err as fs;
    let (_temporary, root) = starter_copy();
    fs::write(
        root.join("effects/array-values.donder"),
        r#"
        effect ArrayValues {
            param shapes: array<curve> in 0.0..1.0;
            param colors: array<gradient>;
            sample { rgb(progress, progress, progress) }
        }
    "#,
    )
    .unwrap();
    let path = "sequences/empty.data.donder";
    let original = fs::read_to_string(root.join(path)).unwrap();
    let authored = original.replacen(
        "import effects from <effects/standard.donder>",
        "import effects from <effects/standard.donder>, <effects/array-values.donder>",
        1,
    );
    assert_ne!(authored, original);
    fs::write(root.join(path), authored).unwrap();
    check_project(&root, &Default::default(), &mut Default::default())
        .into_session()
        .expect("array parameter fixture loads");
    let state = DesktopState::new(|_| {});
    state.open_project_path(root.as_str());
    let mut settings = state.snapshot().settings;
    settings.autosave_project_edits = false;
    state.update_app_settings(settings);
    state.open_file_path(path);
    let request = || GuiDocumentRequest {
        project_revision: state.snapshot().project_revision,
        path: path.into(),
        object_key: "empty".into(),
    };
    let GuiDocument::Sequence { document } = state.get_gui_document(request()).document else {
        panic!("sequence unavailable");
    };
    let definition = document
        .effect_definitions
        .iter()
        .find(|item| item.effect.effect_name == "ArrayValues")
        .unwrap();
    let apply = |edit| {
        let result = state.apply_gui_edit(request(), edit);
        let GuiDocument::Sequence { document } = result.document else {
            panic!("{result:?}");
        };
        document
    };
    let edit = |edit| apply(GuiEdit::Sequence { edit });
    let put_param = |id, name: &str, value| {
        apply(GuiEdit::put(
            GuiSite::of(
                Some(GuiMember::Clip { id }),
                Some(GuiField::Param { name: name.into() }),
            ),
            GuiValue::Param { value },
        ))
    };
    let document = edit(SequenceGuiEdit::AddClip {
        target: document.targets[0].target.clone(),
        start_seconds: 0.0,
        duration_seconds: 1.0,
        content: NewClipContent::Effect {
            initial_color: document.gradient_library[0].gradient.stops[0].value.clone(),
            effect: definition.effect.clone(),
            scope: SequenceClipScope::PerFixture,
        },
    });
    let clip_id = document.clips[0].id;
    let params = |id: u32| {
        state
            .get_sequence_clip_details(request(), vec![id])
            .unwrap()
            .details
            .remove(0)
            .params
    };
    put_param(
        clip_id,
        "shapes",
        SequenceParamValue::Array { values: vec![] },
    );
    let document = put_param(
        clip_id,
        "colors",
        SequenceParamValue::Array { values: vec![] },
    );
    let clip = &document.clips[0];
    let clip_params = params(clip.id);
    assert!(
        matches!(&clip_params[0].value, SequenceParamValue::Array { values } if values.is_empty())
    );
    assert!(
        matches!(&clip_params[1].value, SequenceParamValue::Array { values } if values.is_empty())
    );
    let curve = &document.curve_library[0];
    let gradient = &document.gradient_library[0];
    let source = |path: &str, object_key: &str, display_name: &str| SequenceLibraryLink::Library {
        path: path.into(),
        object_key: object_key.into(),
        display_name: display_name.into(),
    };
    let curve = SequenceCurveValue {
        points: curve.points.clone(),
        link: source(&curve.path, &curve.object_key, &curve.display_name),
    };
    let gradient = SequenceGradientValue {
        gradient: gradient.gradient.clone(),
        link: source(&gradient.path, &gradient.object_key, &gradient.display_name),
    };
    for (name, value) in [
        (
            "shapes",
            SequenceParamValue::Array {
                values: vec![
                    SequenceParamValue::Curve {
                        value: curve.clone(),
                    },
                    SequenceParamValue::Curve { value: curve },
                ],
            },
        ),
        (
            "colors",
            SequenceParamValue::Array {
                values: vec![
                    SequenceParamValue::Gradient {
                        value: gradient.clone(),
                    },
                    SequenceParamValue::Gradient { value: gradient },
                ],
            },
        ),
    ] {
        let document = put_param(clip.id, name, value);
        let mut value = params(document.clips[0].id)
            .iter()
            .find(|param| param.name == name)
            .unwrap()
            .value
            .clone();
        let SequenceParamValue::Array { values } = &mut value else {
            panic!("wrong array editor")
        };
        match &mut values[0] {
            SequenceParamValue::Curve { value } => {
                value.link = SequenceLibraryLink::Inline;
                value.points[0].value = 0.25;
            }
            SequenceParamValue::Gradient { value } => {
                value.link = SequenceLibraryLink::Inline;
                value.gradient.stops[0].value = value.gradient.stops.last().unwrap().value.clone();
            }
            _ => panic!("wrong array editor"),
        }
        let document = put_param(clip.id, name, value);
        let edited = params(document.clips[0].id);
        let value = &edited
            .iter()
            .find(|param| param.name == name)
            .unwrap()
            .value;
        let link = |item: &SequenceParamValue| match item {
            SequenceParamValue::Curve { value } => value.link.clone(),
            SequenceParamValue::Gradient { value } => value.link.clone(),
            _ => panic!("wrong array editor"),
        };
        let SequenceParamValue::Array { values } = value else {
            panic!("wrong array editor")
        };
        let (inline, linked) = (link(&values[0]), link(&values[1]));
        assert!(matches!(inline, SequenceLibraryLink::Inline));
        assert!(matches!(linked, SequenceLibraryLink::Library { .. }));
    }
    state.save_all().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(
        &root,
        &state.project_session().unwrap(),
    );
}

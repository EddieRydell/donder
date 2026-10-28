use super::DesktopState;
use donder_editor::dto::{
    GuiDocument, GuiDocumentRequest, GuiEditCommand, SequenceClipCommonEdit, SequenceClipRef,
    SequenceClipScope, SequenceCurveValue, SequenceGradientValue, SequenceGuiEdit,
    SequenceLibraryLink, SequenceParamValue,
};

#[test]
fn library_parameter_arrays_preserve_links_when_editing_and_saving() {
    use crate::desktop_foundation_tests::tests::starter_copy;
    use donder_project_io::check_project;
    use std::fs;
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
        owned_path: Vec::new(),
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
    let edit = |edit| {
        let result = state.apply_gui_edit(request(), GuiEditCommand::Sequence { edit });
        let GuiDocument::Sequence { document } = result.document else {
            panic!("{result:?}");
        };
        document
    };
    let document = edit(SequenceGuiEdit::AddClip {
        initial_color: document.gradient_library[0].stops[0].value.clone(),
        effect: definition.effect.clone(),
        target: document.lanes[0].target.clone(),
        scope: SequenceClipScope::PerFixture,
        start_seconds: 0.0,
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
    edit(SequenceGuiEdit::EditClips {
        clips: vec![SequenceClipRef::Clip { id: clip_id }],
        edit: SequenceClipCommonEdit::Param {
            name: "shapes".into(),
            value: SequenceParamValue::CurveArray { values: vec![] },
        },
    });
    let document = edit(SequenceGuiEdit::EditClips {
        clips: vec![SequenceClipRef::Clip { id: clip_id }],
        edit: SequenceClipCommonEdit::Param {
            name: "colors".into(),
            value: SequenceParamValue::GradientArray { values: vec![] },
        },
    });
    let clip = &document.clips[0];
    let clip_params = params(clip.id);
    assert!(
        matches!(&clip_params[0].value, SequenceParamValue::CurveArray { values } if values.is_empty())
    );
    assert!(
        matches!(&clip_params[1].value, SequenceParamValue::GradientArray { values } if values.is_empty())
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
        stops: gradient.stops.clone(),
        link: source(&gradient.path, &gradient.object_key, &gradient.display_name),
    };
    for (name, value) in [
        (
            "shapes",
            SequenceParamValue::CurveArray {
                values: vec![curve.clone(), curve],
            },
        ),
        (
            "colors",
            SequenceParamValue::GradientArray {
                values: vec![gradient.clone(), gradient],
            },
        ),
    ] {
        let document = edit(SequenceGuiEdit::EditClips {
            clips: vec![SequenceClipRef::Clip { id: clip.id }],
            edit: SequenceClipCommonEdit::Param {
                name: name.into(),
                value,
            },
        });
        let mut value = params(document.clips[0].id)
            .iter()
            .find(|param| param.name == name)
            .unwrap()
            .value
            .clone();
        match &mut value {
            SequenceParamValue::CurveArray { values } => {
                values[0].link = SequenceLibraryLink::Inline;
                values[0].points[0].value = 0.25;
            }
            SequenceParamValue::GradientArray { values } => {
                values[0].link = SequenceLibraryLink::Inline;
                values[0].stops[0].value = values[0].stops.last().unwrap().value.clone();
            }
            _ => panic!("wrong array editor"),
        }
        let document = edit(SequenceGuiEdit::EditClips {
            clips: vec![SequenceClipRef::Clip { id: clip.id }],
            edit: SequenceClipCommonEdit::Param {
                name: name.into(),
                value,
            },
        });
        let edited = params(document.clips[0].id);
        let value = &edited
            .iter()
            .find(|param| param.name == name)
            .unwrap()
            .value;
        let (inline, linked) = match value {
            SequenceParamValue::CurveArray { values } => (&values[0].link, &values[1].link),
            SequenceParamValue::GradientArray { values } => (&values[0].link, &values[1].link),
            _ => panic!("wrong array editor"),
        };
        assert!(matches!(inline, SequenceLibraryLink::Inline));
        assert!(matches!(linked, SequenceLibraryLink::Library { .. }));
    }
    state.save_all().unwrap();
    crate::desktop_foundation_tests::tests::assert_reloads(
        &root,
        &state.project_session().unwrap(),
    );
}

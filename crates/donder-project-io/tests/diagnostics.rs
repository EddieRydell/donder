mod common;

use camino::{Utf8Path, Utf8PathBuf};
use donder_language::identity::DocumentId;
use donder_language::values::{DonderDuration, DonderTime};
use donder_project_io::{
    IoDiagnosticCode, IoDiagnosticSeverity, TextRange, check_document_text, check_project,
    check_project_document_text,
};
use std::fs;
use std::sync::OnceLock;
use std::time::Duration;

use common::{load_project as load_local_project, write_workspace_metadata};

/// The loaded starter, shared by tests that only validate in-memory edits.
fn starter_session() -> &'static donder_project_io::ProjectSession {
    static SESSION: OnceLock<donder_project_io::ProjectSession> = OnceLock::new();
    SESSION.get_or_init(|| load_local_project(&common::starter_root()))
}

#[test]
fn project_validation_admits_only_timing_representable_by_the_runtime_clock() {
    let session = starter_session();
    let mut sequence = session
        .project
        .reusable_sequences()
        .values()
        .find(|sequence| !sequence.effects.is_empty())
        .unwrap()
        .clone();
    sequence.frame_rate = 1;
    sequence.duration = DonderDuration(Duration::from_secs(4_300));
    let error =
        donder_language::validation::validate_sequence(&session.project, &sequence).unwrap_err();
    assert!(error.message.contains("runtime clock range"), "{error:?}");

    sequence.duration = DonderDuration(Duration::from_micros(u32::MAX as u64));
    sequence.effects[0].start = DonderTime(Duration::from_nanos(500));
    sequence.effects[0].duration = DonderDuration(sequence.duration.0 - Duration::from_nanos(500));
    let error =
        donder_language::validation::validate_sequence(&session.project, &sequence).unwrap_err();
    assert!(error.message.contains("after rounding"), "{error:?}");
}

#[test]
fn project_validation_rejects_invalid_edited_curve_definitions() {
    let mut session = starter_session().clone();
    let (id, mut definition) = session
        .project
        .definitions()
        .curves
        .definitions
        .iter()
        .next()
        .map(|(id, definition)| (id.clone(), definition.clone()))
        .unwrap();
    definition.curve.points[0].position = f32::NAN;
    let error = session
        .project
        .apply_edits([donder_language::model::ProjectEdit::SetCurveDefinition {
            id,
            value: definition,
        }])
        .unwrap_err();
    assert!(error.to_string().contains("Curve"), "{error}");
}

#[test]
fn invalid_gradient_stops_are_rejected_on_load_and_after_edits() {
    let root = common::starter_root();
    let mut sources = donder_project_io::project_source_texts(&root).unwrap();
    let gradient_path = Utf8PathBuf::from("gradients/basic_gradients.gradient.donder");
    let source = sources.get_mut(&gradient_path).unwrap();
    *source = source.replacen("position: 0.3499999940395355", "position: -0.1", 1);
    let report = donder_project_io::check_project_with_overrides(&root, &sources);
    assert!(report.session.is_none());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("invalid gradient")),
        "{:?}",
        report.diagnostics
    );

    let mut session = starter_session().clone();
    let (id, mut definition) = session
        .project
        .definitions()
        .gradients
        .definitions
        .iter()
        .next()
        .map(|(id, definition)| (id.clone(), definition.clone()))
        .unwrap();
    definition.gradient.stops[0].position = f32::NAN;
    let error = session
        .project
        .apply_edits(
            [donder_language::model::ProjectEdit::SetGradientDefinition {
                id,
                value: definition,
            }],
        )
        .unwrap_err();
    assert!(error.to_string().contains("Gradient"), "{error}");
}

#[test]
fn edited_operator_parameters_validate_inline_resources() {
    use donder_language::dsl::{Identifier, compile_operators};
    use donder_language::effect::{EffectParamValue, GradientSource};
    use donder_language::identity::SourceIdentity;
    use donder_language::operator::{
        GraphOperatorNode, OperatorDefinitionId, OperatorRef, custom_operator_definition,
    };
    use donder_language::sequence::{
        CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, GraphNodePosition,
    };
    use donder_language::values::{Color, Gradient, GradientStop};

    let mut session = starter_session().clone();
    let document = session
        .project
        .definitions()
        .operators
        .definitions
        .keys()
        .next()
        .unwrap()
        .0
        .document_id()
        .clone();
    let id = OperatorDefinitionId(SourceIdentity::from_document(
        document,
        "gradient_probe".into(),
    ));
    let compiled = compile_operators(
        "operator GradientProbe { input Signal source; param gradient colors; color sample() { return source.at(seconds()); } }",
    )
    .unwrap()
    .remove(0);
    session
        .project
        .apply_edits(
            [donder_language::model::ProjectEdit::SetOperatorDefinition {
                id: id.clone(),
                value: custom_operator_definition(id.clone(), compiled),
            }],
        )
        .unwrap();
    let sequence_id = session
        .project
        .reusable_sequences()
        .keys()
        .next()
        .unwrap()
        .clone();
    let mut sequence = session.project.sequence(&sequence_id).unwrap().clone();
    let mut operator = GraphOperatorNode {
        operator: OperatorRef::Custom(id),
        params: Default::default(),
    };
    operator.params.insert(
        Identifier::new("colors".into()).unwrap(),
        EffectParamValue::Gradient(GradientSource::Inline(Gradient {
            stops: vec![GradientStop {
                position: f32::NAN,
                color: Color::BLACK,
            }],
        })),
    );
    sequence.composition_graph.nodes.push(CompositionGraphNode {
        id: CompositionGraphNodeId(900_001),
        position: GraphNodePosition { x: 0.0, y: 0.0 },
        kind: CompositionGraphNodeKind::Operator(operator),
    });
    let accepted = session.project.clone();
    let error = session
        .project
        .replace_sequence(&sequence_id, sequence)
        .unwrap_err();
    assert!(error.contains("inline gradient is invalid"), "{error:?}");
    assert_eq!(session.project, accepted);
}

#[test]
fn malformed_multibyte_color_reports_a_diagnostic_without_panicking() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    write_imported_sequence_project(
        &root,
        "  duration: 1s\n  frame_rate: 30\n  mark_collections:\n    - key: beats\n      name: Beats\n      color: '#1é234'\n      marks: []\n",
    );
    let report = check_project(&root);
    assert!(report.session.is_none());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.to_lowercase().contains("color")),
        "{:?}",
        report.diagnostics
    );
}

#[test]
fn all_source_kinds_are_analyzed_from_overrides_without_writing_disk() {
    let workspace = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let root = workspace.join("examples/starter");
    let original = donder_project_io::project_source_texts(&root).unwrap();
    let sequence = Utf8PathBuf::from("sequences/layer_test.sequence.donder");
    let mut overrides = original.clone();
    overrides.insert(
        sequence.clone(),
        original[&sequence].replace("frame_rate: 144", "frame_rate: 90"),
    );
    let report = donder_project_io::check_project_with_overrides(&root, &overrides);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let session = report.session.unwrap();
    assert!(
        session
            .project
            .reusable_sequences()
            .values()
            .any(|sequence| sequence.frame_rate == 90)
    );
    for path in [
        donder_project_io::PROJECT_ROOT_FILE,
        "effects/scan-sweep.effect.donder",
        "operators/gain.operator.donder",
        sequence.as_str(),
    ] {
        let mut invalid = original.clone();
        invalid.insert(path.into(), "[ invalid source".into());
        let report = donder_project_io::check_project_with_overrides(&root, &invalid);
        assert!(report.session.is_none(), "{path} unexpectedly compiled");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path == Utf8Path::new(path)),
            "{path}: {:?}",
            report.diagnostics
        );
    }
    assert_eq!(
        donder_project_io::project_source_texts(&root).unwrap(),
        original
    );
}

#[test]
fn invalid_yaml_reports_parser_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.donder");
    fs::write(
        &entrypoint,
        "broken:\n  type: project\n  setup: [\n  sequences: []\n",
    )
    .unwrap();
    write_workspace_metadata(&root);

    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::YamlParse)
        .unwrap();

    assert!(report.session.is_none());
    assert_eq!(diagnostic.path, Utf8Path::new("project.donder"));
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    assert!(
        diagnostic.range.is_some(),
        "YAML parser diagnostics should include a source range"
    );
}

#[test]
fn invalid_effect_dsl_reports_exact_span() {
    let diagnostics = check_document_text(
        Utf8Path::new("effects/bad.effect.donder"),
        "effect Bad {\n  color sample() {\n    return @;\n  }\n}\n",
    );
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::EffectCompile)
        .unwrap();
    let range = diagnostic.range.as_ref().unwrap();

    assert_eq!(diagnostic.path, Utf8Path::new("effects/bad.effect.donder"));
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    assert_eq!(range.start.line, 2);
    assert_eq!(range.start.character, 11);
    assert_eq!(range.end.line, 2);
    assert_eq!(range.end.character, 12);
}

#[test]
fn invalid_operator_dsl_reports_operator_compile_diagnostic() {
    let diagnostics = check_document_text(
        Utf8Path::new("operators/bad.operator.donder"),
        "operator Bad { input Signal source; color sample() { return source.at(true); } }",
    );
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::OperatorCompile)
        .unwrap();
    assert_eq!(
        diagnostic.path,
        Utf8Path::new("operators/bad.operator.donder")
    );
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    assert!(diagnostic.range.is_some());
}

#[test]
fn invalid_reference_reports_donder_reference_diagnostic() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.donder");
    fs::write(
        &entrypoint,
        "main:\n  type: project\n  setup: missing.setup\n  sequences: []\n",
    )
    .unwrap();
    write_workspace_metadata(&root);

    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::DonderReference)
        .unwrap();

    assert!(report.session.is_none());
    assert_eq!(diagnostic.path, Utf8Path::new("project.donder"));
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    assert_range(diagnostic.range.as_ref().unwrap(), 2, 9, 2, 22);
    assert!(diagnostic.message.contains("missing.setup"));
}

#[test]
fn repeated_reference_text_reports_the_failing_occurrence() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.donder");
    fs::write(
        &entrypoint,
        "imports:\n- from:\n    documents:\n    - setup.donder\n  as: shared\nmain:\n  type: project\n  setup: shared.main\n  sequences: [shared.main]\n",
    )
    .unwrap();
    fs::write(
        root.join("setup.donder"),
        "imports:\n- from:\n    documents:\n    - display.donder\n  as: display\n- from:\n    documents:\n    - patch.donder\n  as: patches\nmain:\n  type: setup\n  layout: display.main\n  patch: patches.main\n  controllers: []\n",
    )
    .unwrap();
    fs::write(root.join("display.donder"), "pixel:\n  type: fixture\n  elements:\n  - id: 1\n    name: Pixel\n    reverse: false\n    shape: {type: pixel}\n    diameter: 0.01\nmain:\n  type: layout\n  fixtures:\n  - id: 1\n    name: Pixel\n    type: fixture\n    definition: pixel\n").unwrap();
    fs::write(
        root.join("patch.donder"),
        "main:\n  type: patch\n  routes: []\n",
    )
    .unwrap();
    write_workspace_metadata(&root);

    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::DonderReference)
        .unwrap();
    let range = diagnostic.range.as_ref().unwrap();
    assert_eq!(range.start.line, 8);
    assert!(range.start.character >= 14);
}

#[test]
fn project_document_override_runs_semantic_validation() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    write_imported_sequence_project(
        &root,
        "  duration: 1s\n  frame_rate: 60\n  audio: null\n  mark_collections: []\n  layers: []\n  effects: []\n  composition_graph:\n    nodes:\n    - id: 1\n      position: { x: 0, y: 0 }\n      type: output\n    edges: []\n  automation_clips: []\n",
    );
    let session = load_local_project(&root);
    let document = DocumentId::new(session.source.project_module_id(), "sequence.donder".into());
    let diagnostics = check_project_document_text(
        &session,
        &document,
        "main:\n  type: sequence\n  duration: invalid\n  frame_rate: 60\n  audio: null\n  mark_collections: []\n  layers: []\n  effects: []\n  composition_graph:\n    nodes:\n    - id: 1\n      position: { x: 0, y: 0 }\n      type: output\n    edges: []\n  automation_clips: []\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path == Utf8Path::new("sequence.donder")
            && diagnostic.code == IoDiagnosticCode::DonderLoad
    }));
}

#[test]
fn missing_required_field_reports_containing_object_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.donder");
    fs::write(&entrypoint, "main:\n  type: project\n  sequences: []\n").unwrap();
    write_workspace_metadata(&root);

    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == IoDiagnosticCode::DonderLoad
                && diagnostic.message.contains("missing field `setup`")
        })
        .unwrap();

    assert_range(diagnostic.range.as_ref().unwrap(), 1, 6, 3, 0);
}

#[test]
fn wrong_field_type_reports_bad_value_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.donder");
    fs::write(
        &entrypoint,
        "main:\n  type: project\n  setup: [bad]\n  sequences: []\n",
    )
    .unwrap();
    write_workspace_metadata(&root);

    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == "setup must be a mapping")
        .unwrap();

    assert_range(diagnostic.range.as_ref().unwrap(), 2, 9, 2, 13);
}

#[test]
fn unsupported_enum_string_reports_that_string_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.donder");
    fs::write(&entrypoint, "main:\n  type: nope\n").unwrap();
    write_workspace_metadata(&root);

    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == "unsupported object type `nope`")
        .unwrap();

    assert_range(diagnostic.range.as_ref().unwrap(), 1, 8, 1, 12);
}

#[test]
fn nested_invalid_color_reports_nested_scalar_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    write_imported_sequence_project(
        &root,
        "  duration: 1s\n  frame_rate: 30\n  mark_collections:\n    - key: beats\n      name: Beats\n      color: bad-color\n      marks: []\n",
    );

    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == "invalid color: bad-color")
        .unwrap();

    assert_range(diagnostic.range.as_ref().unwrap(), 7, 13, 7, 22);
}

#[test]
fn nested_invalid_duration_reports_nested_scalar_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    write_imported_sequence_project(
        &root,
        "  duration: soon\n  frame_rate: 30\n  layers: []\n  effects: []\n  composition_graph:\n    nodes: []\n    edges: []\n",
    );

    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == "duration must end in `s`: soon")
        .unwrap();

    assert_range(diagnostic.range.as_ref().unwrap(), 2, 12, 2, 16);
}

#[test]
fn negative_duration_is_a_diagnostic_not_a_loader_panic() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    write_imported_sequence_project(
        &root,
        "  duration: -1s\n  frame_rate: 60\n  audio: null\n  mark_collections: []\n  layers: []\n  effects: []\n  composition_graph:\n    nodes:\n    - id: 1\n      position: { x: 0, y: 0 }\n      type: output\n    edges: []\n",
    );

    let report = check_project(&root);

    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("duration must not be negative")),
        "{:#?}",
        report.diagnostics
    );
}

#[test]
fn malformed_optional_sequence_field_is_a_diagnostic() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    write_imported_sequence_project(&root, &minimal_sequence_body("  automation_clips: wrong\n"));
    let malformed = check_project(&root);
    assert!(malformed.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("field `automation_clips` must be a sequence")
    }));
}

#[test]
fn imported_effect_errors_keep_exact_spans_without_aggregate_marker() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.donder");
    fs::write(
        &entrypoint,
        "imports:\n  - from:\n      documents:\n      - bad.effect.donder\n    as: fx\nmain:\n  type: project\n  setup: missing.setup\n  sequences: []\n",
    )
    .unwrap();
    fs::write(
        root.join("bad.effect.donder"),
        "effect Bad {\n  color sample() {\n    return @;\n  }\n}\n",
    )
    .unwrap();
    write_workspace_metadata(&root);

    let report = check_project(&root);
    let effect_diagnostics = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == IoDiagnosticCode::EffectCompile)
        .collect::<Vec<_>>();

    assert!(
        effect_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.range.is_some())
    );
    assert!(effect_diagnostics.iter().any(|diagnostic| diagnostic.path
        == Utf8Path::new("bad.effect.donder")
        && diagnostic.range.as_ref().is_some_and(|range| {
            range.start.line == 2
                && range.start.character == 11
                && range.end.line == 2
                && range.end.character == 12
        })));
    assert!(
        effect_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.range.as_ref().is_none_or(|range| {
                range.start.line != 0 || range.start.character != 0 || range.end.character != 1
            }))
    );
}

#[test]
fn missing_root_document_reports_no_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let report = check_project(&root);
    let diagnostic = report.diagnostics.first().unwrap();

    assert_eq!(diagnostic.code, IoDiagnosticCode::IoRead);
    assert_eq!(diagnostic.range, None);
}

#[test]
fn valid_example_project_loads_without_diagnostics() {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    let root = workspace_root.join("examples/starter");

    let report = check_project(&root);

    assert!(report.session.is_some());
    assert_eq!(report.diagnostics, Vec::new());
}

fn write_imported_sequence_project(root: &Utf8Path, sequence_body: &str) {
    fs::write(
        root.join("project.donder"),
        "imports:\n  - from:\n      documents:\n      - setup.donder\n    as: setups\n  - from:\n      documents:\n      - sequence.donder\n    as: sequences\nmain:\n  type: project\n  setup: setups.main\n  sequences: [sequences.main]\n",
    )
    .unwrap();
    fs::write(
        root.join("setup.donder"),
        "imports:\n  - from:\n      documents:\n      - display.donder\n    as: display\n  - from:\n      documents:\n      - patch.donder\n    as: patches\nmain:\n  type: setup\n  layout: display.main\n  patch: patches.main\n  controllers: []\n",
    )
    .unwrap();
    fs::write(root.join("display.donder"), "pixel:\n  type: fixture\n  elements:\n  - id: 1\n    name: Pixel\n    reverse: false\n    shape: {type: pixel}\n    diameter: 0.01\nmain:\n  type: layout\n  fixtures:\n  - id: 1\n    name: Pixel\n    type: fixture\n    definition: pixel\n").unwrap();
    fs::write(
        root.join("patch.donder"),
        "main:\n  type: patch\n  routes: []\n",
    )
    .unwrap();
    fs::write(
        root.join("sequence.donder"),
        format!("main:\n  type: sequence\n{sequence_body}"),
    )
    .unwrap();
    write_workspace_metadata(root);
}

fn minimal_sequence_body(extra: &str) -> String {
    format!(
        "  duration: 1s\n  frame_rate: 60\n  audio: null\n  mark_collections: []\n  layers: []\n  effects: []\n  composition_graph:\n    nodes:\n    - id: 1\n      position: {{ x: 0, y: 0 }}\n      type: output\n    edges: []\n{extra}"
    )
}

fn assert_range(
    range: &TextRange,
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
) {
    assert_eq!(
        (
            range.start.line,
            range.start.character,
            range.end.line,
            range.end.character
        ),
        (start_line, start_character, end_line, end_character)
    );
}

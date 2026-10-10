use crate::common;

use camino::{Utf8Path, Utf8PathBuf};
use donder_language::{DonderDuration, DonderTime};
use donder_model::DocumentId;
use donder_project_io::{
    IoDiagnosticCode, IoDiagnosticSeverity, PROJECT_ROOT_FILE, TextRange, check_document_text,
    check_project, check_project_document_text,
};
use std::fs;
use std::sync::OnceLock;
use std::time::Duration;

use crate::common::load_project as load_local_project;

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
    let error = donder_model::validate_sequence(&session.project, &sequence).unwrap_err();
    assert!(error.message.contains("runtime clock range"), "{error:?}");

    sequence.duration = DonderDuration(Duration::from_micros(u32::MAX as u64));
    let effect = std::sync::Arc::make_mut(&mut sequence.effects[0]);
    effect.start = DonderTime(Duration::from_nanos(500));
    effect.duration = DonderDuration(sequence.duration.0 - Duration::from_nanos(500));
    let error = donder_model::validate_sequence(&session.project, &sequence).unwrap_err();
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
        .apply_edits([donder_model::ProjectEdit::SetCurveDefinition {
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
    let gradient_path = Utf8PathBuf::from("gradients/basic_gradients.data.donder");
    let source = sources.get_mut(&gradient_path).unwrap();
    *source = source.replacen("(0.35, #ffb000)", "(-0.1, #ffb000)", 1);
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
        .apply_edits([donder_model::ProjectEdit::SetGradientDefinition {
            id,
            value: definition,
        }])
        .unwrap_err();
    assert!(error.to_string().contains("Gradient"), "{error}");
}

#[test]
fn edited_operator_parameters_validate_inline_resources() {
    use donder_language::compiler::compile_operators;
    use donder_model::SourceIdentity;
    use donder_model::{
        CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, GraphNodePosition,
    };
    use donder_model::{EffectParamValue, GradientSource};
    use donder_model::{
        GraphOperatorNode, OperatorDefinitionId, OperatorRef, custom_operator_definition,
    };
    use donder_runtime_types::Identifier;
    use donder_runtime_types::{Color, Gradient, GradientStop};

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
        "operator GradientProbe { input source; param colors: gradient; sample { source } }",
    )
    .unwrap()
    .remove(0);
    session
        .project
        .apply_edits([donder_model::ProjectEdit::SetOperatorDefinition {
            id: id.clone(),
            value: custom_operator_definition(id.clone(), compiled),
        }])
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
        name: donder_language::object_name("operator"),
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
    common::write_imported_sequence_project(
        &root,
        &common::MINIMAL_SEQUENCE.replace(
            "marks: []",
            "marks: [MarkCollection { name: beats, description: none, color: #1é234, times: [] }]",
        ),
    );
    let report = check_project(&root);
    assert!(report.session.is_none());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.path == "sequence.data.donder"
                && diagnostic.range.is_some()),
        "{:?}",
        report.diagnostics
    );
}

#[test]
fn all_source_kinds_are_analyzed_from_overrides_without_writing_disk() {
    let root = common::starter_root();
    let original = donder_project_io::project_source_texts(&root).unwrap();
    let sequence = Utf8PathBuf::from("sequences/layer_test.data.donder");
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
        "effects/scan-sweep.donder",
        "operators/gain.donder",
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
fn invalid_data_syntax_reports_parser_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    fs::write(
        root.join(PROJECT_ROOT_FILE),
        common::root_document("", "  setup: setups.main\n  sequences: [],\n"),
    )
    .unwrap();

    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::DataSyntax)
        .unwrap_or_else(|| panic!("{:?}", report.diagnostics));

    assert!(report.session.is_none());
    assert_eq!(diagnostic.path, Utf8Path::new(PROJECT_ROOT_FILE));
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    assert!(
        diagnostic.range.is_some(),
        "syntax diagnostics should include a source range"
    );
}

#[test]
fn unclosed_bracket_inside_a_record_is_one_syntax_error() {
    let text = "Project main {\n  format: 1,\n  setup: [\n  sequences: [],\n}\n";
    let diagnostics = check_document_text(Utf8Path::new(PROJECT_ROOT_FILE), text);
    assert!(
        !diagnostics.is_empty()
            && diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == IoDiagnosticCode::DataSyntax),
        "{diagnostics:?}"
    );
    assert!(diagnostics.len() <= 2, "{diagnostics:?}");
}

#[test]
fn comments_are_syntax_errors_in_data_documents() {
    let diagnostics = check_document_text(
        Utf8Path::new("curves/commented.data.donder"),
        "-- a note\nCurve flat { description: none, points: [] }\n",
    );
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::DataSyntax)
        .unwrap_or_else(|| panic!("{diagnostics:?}"));
    assert_eq!(diagnostic.range.as_ref().unwrap().start.line, 0);
}

/// An unexpected character at line 2, characters 4..5.
const BAD_EFFECT: &str = "effect Bad {\n  sample {\n    @\n  }\n}\n";

#[test]
fn invalid_effect_dsl_reports_exact_span() {
    let diagnostics = check_document_text(Utf8Path::new("effects/bad.donder"), BAD_EFFECT);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::ScriptCompile)
        .unwrap();

    assert_eq!(diagnostic.path, Utf8Path::new("effects/bad.donder"));
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    assert!(
        diagnostic.message.contains("unexpected character"),
        "{diagnostic:?}"
    );
    assert_range(diagnostic.range.as_ref().unwrap(), 2, 4, 2, 5);
}

#[test]
fn invalid_operator_dsl_reports_script_compile_diagnostic() {
    let source = "operator Bad { input source; sample { source.at(true) } }";
    let diagnostics = check_document_text(Utf8Path::new("operators/bad.donder"), source);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::ScriptCompile)
        .unwrap();
    assert_eq!(diagnostic.path, Utf8Path::new("operators/bad.donder"));
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    // The sample time must be a number; the diagnostic covers the bad argument.
    assert!(diagnostic.message.contains("float"), "{diagnostic:?}");
    let start = source.find("true").unwrap() as u32;
    assert_range(diagnostic.range.as_ref().unwrap(), 0, start, 0, start + 4);
}

#[test]
fn invalid_reference_reports_donder_reference_diagnostic() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    fs::write(
        root.join(PROJECT_ROOT_FILE),
        common::root_document("", "  setup: missing.setup,\n  sequences: [],\n"),
    )
    .unwrap();

    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == IoDiagnosticCode::DonderReference)
        .unwrap_or_else(|| panic!("{:?}", report.diagnostics));

    assert!(report.session.is_none());
    assert_eq!(diagnostic.path, Utf8Path::new(PROJECT_ROOT_FILE));
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    assert_range(diagnostic.range.as_ref().unwrap(), 5, 9, 5, 22);
    assert!(diagnostic.message.contains("missing.setup"));
}

#[test]
fn repeated_reference_text_reports_the_failing_occurrence() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    fs::write(
        root.join(PROJECT_ROOT_FILE),
        common::root_document(
            "import shared from <setup.data.donder>;\n",
            "  setup: shared.main,\n  sequences: [shared.main],\n",
        ),
    )
    .unwrap();
    fs::write(root.join("setup.data.donder"), common::SETUP).unwrap();
    fs::write(root.join("display.data.donder"), common::DISPLAY).unwrap();
    fs::write(root.join("patch.data.donder"), common::PATCH).unwrap();

    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("shared.main"))
        .unwrap_or_else(|| panic!("{:?}", report.diagnostics));
    let range = diagnostic.range.as_ref().unwrap();
    assert_eq!(range.start.line, 7);
    assert_eq!(range.start.character, 14);
}

#[test]
fn project_document_override_runs_semantic_validation() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    common::write_imported_sequence_project(&root, common::MINIMAL_SEQUENCE);
    let session = load_local_project(&root);
    let document = DocumentId::new(
        session.source.project_module_id(),
        "sequence.data.donder".into(),
    );
    // Well-formed text whose edge names a node the sequence lacks.
    let text = common::MINIMAL_SEQUENCE
        .replace("edges: []", "edges: [Edge { from: nothing, to: output }]");
    assert!(check_document_text(document.path(), &text).is_empty());
    let diagnostics = check_project_document_text(&session, &document, &text);
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.path == Utf8Path::new("sequence.data.donder")
                && diagnostic.code == IoDiagnosticCode::DonderLoad
                && diagnostic.message.contains("nothing")
        }),
        "{diagnostics:?}"
    );
}

/// The diagnostic for `sequence`, a variant of the minimal sequence.
fn sequence_diagnostic(sequence: &str, message: &str) -> donder_project_io::IoDiagnostic {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    common::write_imported_sequence_project(&root, sequence);
    let report = check_project(&root);
    assert!(report.session.is_none());
    report
        .diagnostics
        .into_iter()
        .find(|diagnostic| {
            diagnostic.path == "sequence.data.donder" && diagnostic.message.contains(message)
        })
        .unwrap_or_else(|| panic!("no `{message}` in {sequence}"))
}

/// The line and character range of the first `needle` in `text`.
fn range_of(text: &str, needle: &str) -> (u32, u32, u32, u32) {
    let start = text.find(needle).unwrap();
    let line = text[..start].matches('\n').count() as u32;
    let character = (start - text[..start].rfind('\n').map_or(0, |index| index + 1)) as u32;
    (line, character, line, character + needle.len() as u32)
}

fn assert_at(diagnostic: &donder_project_io::IoDiagnostic, text: &str, needle: &str) {
    let (start_line, start_character, end_line, end_character) = range_of(text, needle);
    assert_range(
        diagnostic.range.as_ref().unwrap(),
        start_line,
        start_character,
        end_line,
        end_character,
    );
}

#[test]
fn missing_field_is_reported_where_it_belongs() {
    let text = common::MINIMAL_SEQUENCE.replace("  frame_rate: 60,\n", "");
    let diagnostic = sequence_diagnostic(&text, "frame_rate");
    assert_eq!(diagnostic.code, IoDiagnosticCode::DataSyntax);
    assert!(diagnostic.range.is_some());
}

#[test]
fn fields_out_of_schema_order_are_errors() {
    let text = common::MINIMAL_SEQUENCE.replace(
        "  duration: 1s,\n  frame_rate: 60,\n",
        "  frame_rate: 60,\n  duration: 1s,\n",
    );
    let diagnostic = sequence_diagnostic(&text, "duration");
    assert_eq!(diagnostic.code, IoDiagnosticCode::DataSyntax);
}

#[test]
fn unknown_field_reports_its_name_range() {
    let text =
        common::MINIMAL_SEQUENCE.replace("  audio: none,\n", "  audio: none,\n  tempo: 120,\n");
    let diagnostic = sequence_diagnostic(&text, "tempo");
    assert_at(&diagnostic, &text, "tempo");
}

#[test]
fn wrong_field_type_reports_bad_value_range() {
    let text = common::MINIMAL_SEQUENCE.replace("frame_rate: 60", "frame_rate: [bad]");
    let diagnostic = sequence_diagnostic(&text, "a list");
    assert_at(&diagnostic, &text, "[bad]");
}

#[test]
fn integers_are_not_floats_in_data() {
    let text = common::MINIMAL_SEQUENCE.replace("position: (0.0, 0.0)", "position: (0, 0.0)");
    let diagnostic = sequence_diagnostic(&text, "float");
    assert_eq!(
        diagnostic.range.as_ref().unwrap().start.line,
        range_of(&text, "(0, 0.0)").0
    );
}

#[test]
fn non_canonical_literals_name_their_canonical_spelling() {
    for (before, after, canonical) in [
        ("duration: 1s", "duration: 1.50s", "1.5s"),
        ("position: (0.0, 0.0)", "position: (0.50, 0.0)", "0.5"),
    ] {
        let text = common::MINIMAL_SEQUENCE.replace(before, after);
        let diagnostic = sequence_diagnostic(&text, canonical);
        assert_eq!(diagnostic.code, IoDiagnosticCode::DataSyntax);
        assert!(diagnostic.range.is_some());
    }
}

#[test]
fn unknown_declaration_type_reports_that_name_range() {
    let text = "Nope main { description: none }\n";
    let diagnostic = sequence_diagnostic(text, "Nope");
    assert_at(&diagnostic, text, "Nope");
}

#[test]
fn nested_invalid_color_reports_nested_value_range() {
    let text = common::MINIMAL_SEQUENCE.replace(
        "marks: []",
        "marks: [MarkCollection { name: beats, description: none, color: 7, times: [] }]",
    );
    let diagnostic = sequence_diagnostic(&text, "color");
    assert_at(&diagnostic, &text, "7");
}

#[test]
fn negative_duration_is_a_diagnostic_not_a_loader_panic() {
    let text = common::MINIMAL_SEQUENCE.replace("duration: 1s", "duration: -1s");
    let diagnostic = sequence_diagnostic(&text, "");
    assert!(diagnostic.range.is_some(), "{diagnostic:?}");
}

#[test]
fn wrong_reference_kind_names_both_kinds() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    common::write_imported_sequence_project(&root, common::MINIMAL_SEQUENCE);
    let setup = common::SETUP.replace("layout: display.main", "layout: display.pixel");
    fs::write(root.join("setup.data.donder"), &setup).unwrap();
    let report = check_project(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic
                .message
                .contains("a fixture definition, not a layout")
        })
        .unwrap_or_else(|| panic!("{:?}", report.diagnostics));
    assert_eq!(diagnostic.path, "setup.data.donder");
    assert_at(diagnostic, &setup, "display.pixel");
}

#[test]
fn missing_fixture_and_layer_names_are_reported_at_the_name() {
    let clip = "clips: [\n    Clip {\n      name: pulse,\n      description: none,\n      layer: base,\n      start: 0s,\n      duration: 1s,\n      target: display.main.pixel,\n      scope: PerFixture,\n      effect: fx.Flat,\n      params: {},\n    },\n  ]";
    let sequence = format!(
        "import display from <display.data.donder>;\nimport fx from <flat.donder>;\n\n{}",
        common::MINIMAL_SEQUENCE
            .replace(
                "layers: []",
                "layers: [Layer { name: base, description: none, color: #ffffff, enabled: true }]"
            )
            .replace("clips: []", clip)
    );
    for (before, after, needle) in [
        (
            "target: display.main.pixel",
            "target: display.main.gone",
            "gone",
        ),
        (
            "layer: base,\n      start",
            "layer: lost,\n      start",
            "lost",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        common::write_imported_sequence_project(&root, &sequence);
        fs::write(
            root.join("flat.donder"),
            "effect Flat { sample { #ffffff } }",
        )
        .unwrap();
        assert!(check_project(&root).session.is_some(), "{sequence}");
        let text = sequence.replace(before, after);
        let diagnostic = sequence_diagnostic_in(&root, &text, needle);
        assert_at(&diagnostic, &text, needle);
    }
}

fn sequence_diagnostic_in(
    root: &Utf8Path,
    text: &str,
    message: &str,
) -> donder_project_io::IoDiagnostic {
    fs::write(root.join("sequence.data.donder"), text).unwrap();
    let report = check_project(root);
    assert!(report.session.is_none());
    report
        .diagnostics
        .into_iter()
        .find(|diagnostic| {
            diagnostic.path == "sequence.data.donder" && diagnostic.message.contains(message)
        })
        .unwrap_or_else(|| panic!("no `{message}` in {text}"))
}

#[test]
fn imported_effect_errors_keep_exact_spans_without_aggregate_marker() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    fs::write(
        root.join(PROJECT_ROOT_FILE),
        common::root_document(
            "import fx from <bad.donder>;\n",
            "  setup: missing.setup,\n  sequences: [],\n",
        ),
    )
    .unwrap();
    fs::write(root.join("bad.donder"), BAD_EFFECT).unwrap();

    let report = check_project(&root);
    let script_diagnostics = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == IoDiagnosticCode::ScriptCompile)
        .collect::<Vec<_>>();

    assert!(!script_diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert!(
        script_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.range.is_some())
    );
    assert!(script_diagnostics.iter().any(|diagnostic| diagnostic.path
        == Utf8Path::new("bad.donder")
        && diagnostic.range.as_ref().is_some_and(|range| {
            range.start.line == 2
                && range.start.character == 4
                && range.end.line == 2
                && range.end.character == 5
        })));
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
fn root_metadata_is_validated() {
    for (before, after) in [("format: 1", "format: 7"), ("id: \"", "id: \"not-a-uuid")] {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        common::write_imported_sequence_project(&root, common::MINIMAL_SEQUENCE);
        let path = root.join(PROJECT_ROOT_FILE);
        let text = fs::read_to_string(&path)
            .unwrap()
            .replacen(before, after, 1);
        fs::write(&path, text).unwrap();
        let report = check_project(&root);
        assert!(report.session.is_none(), "{after}");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path == PROJECT_ROOT_FILE),
            "{after}: {:?}",
            report.diagnostics
        );
    }
}

#[test]
fn valid_example_project_loads_without_diagnostics() {
    let report = check_project(&common::starter_root());

    assert!(report.session.is_some());
    assert_eq!(report.diagnostics, Vec::new());
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

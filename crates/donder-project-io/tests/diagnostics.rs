mod common;

use camino::{Utf8Path, Utf8PathBuf};
use donder_language::identity::DocumentId;
use donder_project_io::{
    IoDiagnosticCode, IoDiagnosticSeverity, TextRange, check_document_text, check_package,
    check_project_document_text,
};
use std::fs;

use common::{load_project_package, write_project_package};

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
    let report = donder_project_io::check_package_with_overrides(&root, &overrides);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let session = report.session.unwrap();
    assert!(
        session
            .project
            .sequences
            .values()
            .any(|sequence| sequence.frame_rate == 90)
    );
    for path in [
        donder_package::MANIFEST_FILE,
        donder_package::LOCK_FILE,
        "effects/scan-sweep.effect.donder",
        "operators/gain.operator.donder",
        sequence.as_str(),
    ] {
        let mut invalid = original.clone();
        invalid.insert(path.into(), "[ invalid source".into());
        let report = donder_project_io::check_package_with_overrides(&root, &invalid);
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
fn setup_field_typos_in_unsaved_documents_report_exact_locations() {
    let workspace = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let root = workspace.join("examples/starter");
    let original = donder_project_io::project_source_texts(&root).unwrap();
    for (path, anchor, indentation, label) in [
        (
            "layouts/outputs.layout.donder",
            "  type: layout",
            2,
            "layout",
        ),
        (
            "layouts/outputs.layout.donder",
            "    name: All Outputs",
            4,
            "layout fixture",
        ),
        (
            "layouts/outputs.layout.donder",
            "      name: Output 01",
            6,
            "layout fixture",
        ),
        (
            "layouts/outputs.layout.donder",
            "          x: 0.0",
            10,
            "point",
        ),
        ("patches/outputs.patch.donder", "  type: patch", 2, "patch"),
        (
            "patches/outputs.patch.donder",
            "    port: 1",
            4,
            "LED route",
        ),
        (
            "patches/outputs.patch.donder",
            "      fixture: 1",
            6,
            "fixture target",
        ),
        (
            "patches/outputs.patch.donder",
            "      type: rgb",
            6,
            "pixel encoding",
        ),
    ] {
        let path = Utf8PathBuf::from(path);
        let source = &original[&path];
        assert!(source.contains(anchor), "missing anchor {anchor}");
        let replacement = format!(
            "{anchor}\n{}typo_field: unexpected",
            " ".repeat(indentation)
        );
        let edited = source.replacen(anchor, &replacement, 1);
        assert_unknown_setup_field(&root, &original, &path, &edited, label);
    }
    assert_eq!(
        donder_project_io::project_source_texts(&root).unwrap(),
        original
    );
}

#[test]
fn fixture_pixel_field_typos_are_rejected_without_changing_the_saved_project() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    write_imported_sequence_project(&root, &minimal_sequence_body(""));
    let path = Utf8PathBuf::from("display.donder");
    let display = fs::read_to_string(root.join(&path)).unwrap().replace(
        "  - id: 1\n    diameter: 0.01",
        "  - id: 1\n    diameter: 0.01\n    position: { x: 0, y: 0, z: 0 }",
    );
    fs::write(root.join(&path), &display).unwrap();
    let baseline = check_package(&root);
    assert!(
        baseline.diagnostics.is_empty(),
        "{:?}",
        baseline.diagnostics
    );
    let original = donder_project_io::project_source_texts(&root).unwrap();
    for (anchor, indentation, label) in [
        ("  type: fixture", 2, "fixture definition"),
        ("    diameter: 0.01", 4, "pixel"),
    ] {
        let edited = display.replacen(
            anchor,
            &format!(
                "{anchor}\n{}typo_field: unexpected",
                " ".repeat(indentation)
            ),
            1,
        );
        assert_unknown_setup_field(&root, &original, &path, &edited, label);
    }
    let edited = display.replacen(
        "{ x: 0, y: 0, z: 0 }",
        "{ x: 0, y: 0, z: 0, typo_field: unexpected }",
        1,
    );
    assert_unknown_setup_field(&root, &original, &path, &edited, "point");
    assert_eq!(
        donder_project_io::project_source_texts(&root).unwrap(),
        original
    );
}

fn assert_unknown_setup_field(
    root: &Utf8Path,
    original: &std::collections::BTreeMap<Utf8PathBuf, String>,
    path: &Utf8PathBuf,
    edited: &str,
    label: &str,
) {
    let mut overrides = original.clone();
    overrides.insert(path.clone(), edited.to_owned());
    let report = donder_project_io::check_package_with_overrides(root, &overrides);
    assert!(
        report.session.is_none(),
        "{label} silently accepted an unknown field"
    );
    let message = format!("{label} has an unknown field `typo_field`");
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == message)
        .unwrap_or_else(|| panic!("missing {message}: {:?}", report.diagnostics));
    assert_eq!(&diagnostic.path, path);
    assert_eq!(diagnostic.severity, IoDiagnosticSeverity::Error);
    let (line, text) = edited
        .lines()
        .enumerate()
        .find(|(_, text)| text.contains("typo_field:"))
        .unwrap();
    let column = text.find("unexpected").unwrap() as u32;
    assert_range(
        diagnostic.range.as_ref().unwrap(),
        line as u32,
        column,
        line as u32,
        column + 10,
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
    write_project_package(&root);

    let report = check_package(&root);
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
    write_project_package(&root);

    let report = check_package(&root);
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
    fs::write(root.join("display.donder"), "pixel:\n  type: fixture\n  pixels:\n  - id: 1\n    diameter: 0.01\nmain:\n  type: layout\n  fixtures:\n  - id: 1\n    name: Pixel\n    type: fixture\n    definition: pixel\n").unwrap();
    fs::write(
        root.join("patch.donder"),
        "main:\n  type: patch\n  routes: []\n",
    )
    .unwrap();
    write_project_package(&root);

    let report = check_package(&root);
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
    let session = load_project_package(&root);
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
    write_project_package(&root);

    let report = check_package(&root);
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
    write_project_package(&root);

    let report = check_package(&root);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == "field `setup` must be a string")
        .unwrap();

    assert_range(diagnostic.range.as_ref().unwrap(), 2, 9, 2, 13);
}

#[test]
fn unsupported_enum_string_reports_that_string_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let entrypoint = root.join("project.donder");
    fs::write(&entrypoint, "main:\n  type: nope\n").unwrap();
    write_project_package(&root);

    let report = check_package(&root);
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

    let report = check_package(&root);
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

    let report = check_package(&root);
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

    let report = check_package(&root);

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
fn malformed_optional_sequence_and_unknown_sequence_field_are_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    write_imported_sequence_project(&root, &minimal_sequence_body("  automation_clips: wrong\n"));
    let malformed = check_package(&root);
    assert!(malformed.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("field `automation_clips` must be a sequence")
    }));

    write_imported_sequence_project(&root, &minimal_sequence_body("  automtion_clips: []\n"));
    let typo = check_package(&root);
    assert!(typo.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("sequence has an unknown field `automtion_clips`")
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
    write_project_package(&root);

    let report = check_package(&root);
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
fn missing_manifest_reports_no_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let report = check_package(&root);
    let diagnostic = report.diagnostics.first().unwrap();

    assert_eq!(diagnostic.code, IoDiagnosticCode::DonderLoad);
    assert_eq!(diagnostic.range, None);
}

#[test]
fn valid_example_project_loads_without_diagnostics() {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    let root = workspace_root.join("examples/starter");

    let report = check_package(&root);

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
    fs::write(root.join("display.donder"), "pixel:\n  type: fixture\n  pixels:\n  - id: 1\n    diameter: 0.01\nmain:\n  type: layout\n  fixtures:\n  - id: 1\n    name: Pixel\n    type: fixture\n    definition: pixel\n").unwrap();
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
    write_project_package(root);
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

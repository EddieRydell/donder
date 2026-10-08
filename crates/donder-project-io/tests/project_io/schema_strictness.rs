use crate::common;

use camino::Utf8PathBuf;
use donder_language::data::tree::{DataField, DataValue, Spanned};
use donder_project_io::{
    PROJECT_ROOT_FILE, SourceDocumentFormat, check_project_with_overrides, project_source_texts,
    source_document_format,
};
use std::collections::BTreeSet;

#[test]
fn operator_names_require_project_definitions_and_explicit_imports() {
    let root = common::starter_root();
    let original = project_source_texts(&root).unwrap();
    let path = Utf8PathBuf::from("sequences/layer_test.data.donder");
    let source = &original[&path];
    assert!(source.contains("operator: operators.TimeWarp"));
    // Every operator name goes through the same reference resolution.
    let mut overrides = original.clone();
    overrides.insert(
        path.clone(),
        source.replace("operator: operators.TimeWarp", "operator: max"),
    );
    let report = check_project_with_overrides(&root, &overrides);
    assert!(report.session.is_none(), "unimported operator max resolved");
    assert!(!report.diagnostics.is_empty());
    assert_eq!(original, project_source_texts(&root).unwrap());
}

/// The byte offset just inside the braces of one record per record type,
/// with the type's name.
fn record_openings(
    fields: &Spanned<Vec<DataField>>,
    ty: &str,
    seen: &mut BTreeSet<String>,
    output: &mut Vec<(String, usize)>,
) {
    if seen.insert(ty.to_string()) {
        output.push((ty.to_string(), fields.span.start + 1));
    }
    for field in &fields.value {
        value_openings(&field.value, seen, output);
    }
}

fn value_openings(
    value: &Spanned<DataValue>,
    seen: &mut BTreeSet<String>,
    output: &mut Vec<(String, usize)>,
) {
    match &value.value {
        DataValue::Record(ty, fields) | DataValue::Named(ty, _, fields) => {
            record_openings(fields, ty.value.as_str(), seen, output);
        }
        DataValue::List(items) | DataValue::Tuple(items) => {
            for item in items {
                value_openings(item, seen, output);
            }
        }
        _ => {}
    }
}

#[test]
fn every_starter_record_type_rejects_extra_fields_at_the_source_location() {
    let root = common::starter_root();
    let original = project_source_texts(&root).unwrap();
    let mut seen = BTreeSet::new();
    let mut checked = 0;
    for (path, source) in &original {
        if source_document_format(path) != SourceDocumentFormat::Data {
            continue;
        }
        let (document, diagnostics) = donder_language::data::parse(source);
        assert!(diagnostics.is_empty(), "{path}: {diagnostics:?}");
        let mut openings = Vec::new();
        for declaration in &document.declarations {
            record_openings(
                &declaration.fields,
                declaration.ty.value.as_str(),
                &mut seen,
                &mut openings,
            );
        }
        for (ty, offset) in openings {
            let marker = " unexpected_schema_field: none,";
            let text = format!("{}{marker}{}", &source[..offset], &source[offset..]);
            let at = offset + 1;
            let line = text[..at].matches('\n').count() as u32;
            let character = (at - text[..at].rfind('\n').map_or(0, |index| index + 1)) as u32;
            let mut overrides = original.clone();
            overrides.insert(path.clone(), text);
            let report = check_project_with_overrides(&root, &overrides);
            assert!(report.session.is_none(), "accepted {path}: {ty}");
            let diagnostic = report
                .diagnostics
                .iter()
                .find(|d| d.message.contains("unexpected_schema_field"))
                .unwrap_or_else(|| panic!("{path}: {ty}: {:?}", report.diagnostics));
            assert_eq!(&diagnostic.path, path);
            let range = diagnostic.range.as_ref().unwrap();
            assert_eq!(
                (range.start.line, range.start.character),
                (line, character),
                "{path}: {ty}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 20, "only exercised {checked} record types");
    assert_eq!(original, project_source_texts(&root).unwrap());
}

const SMALL_PROJECT: &str = r#"
Setup setup { description: none, layout: layout, patch: patch, controllers: [] }

Layout layout {
  description: none,
  root: [pixel],
  items: [
    Fixture {
      name: pixel,
      description: none,
      definition: pixel,
      transform: Transform { position: (0m, 0m, 0m), rotation: (0.0, 0.0, 0.0), scale: (1.0, 1.0, 1.0) },
    },
  ],
}

FixtureDefinition pixel {
  description: none,
  shapes: [
    Shape {
      name: pixel,
      diameter: 0.01m,
      reverse: false,
      transform: Transform { position: (0m, 0m, 0m), rotation: (0.0, 0.0, 0.0), scale: (1.0, 1.0, 1.0) },
      geometry: Pixel,
    },
  ],
}

Patch patch { description: none, routes: [] }

Sequence show {
  description: none,
  duration: 10s,
  frame_rate: 60,
  audio: none,
  marks: [],
  layers: [Layer { name: main, description: none, color: #ffffff, enabled: true }],
  clips: [
    Clip {
      name: defaults,
      description: none,
      layer: main,
      start: 0s,
      duration: 10s,
      target: layout.pixel,
      scope: PerFixture,
      effect: fx.Defaults,
      params: { level: 0.5 },
    },
  ],
  graph: Graph {
    nodes: [LayerNode { layer: main, position: (0.0, 0.0) }, OutputNode { position: (1.0, 0.0) }],
    edges: [Edge { from: main, to: output }],
  },
  automation: [],
}
"#;

fn small_project() -> (tempfile::TempDir, Utf8PathBuf) {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().to_path_buf()).unwrap();
    std::fs::write(
        root.join(PROJECT_ROOT_FILE),
        common::root_document(
            "import fx from <effect.donder>;\n",
            "  setup: setup,\n  sequences: [show],\n",
        ) + SMALL_PROJECT,
    )
    .unwrap();
    std::fs::write(
        root.join("effect.donder"),
        "effect Defaults { param level: float in 0.0..1.0 = 0.5; param tint: color = #ffffff; sample { rgb(level, level, level) } }",
    )
    .unwrap();
    common::load_project(&root);
    (temporary, root)
}

/// Every edit of the small project's root document is rejected with a
/// diagnostic containing `expected`.
fn assert_rejected(edits: &[(&str, &str, &str)]) {
    let (_temp, root) = small_project();
    let original = project_source_texts(&root).unwrap();
    let path = Utf8PathBuf::from(PROJECT_ROOT_FILE);
    for (before, after, expected) in edits {
        assert!(original[&path].contains(before), "{before}");
        let mut overrides = original.clone();
        overrides.insert(path.clone(), original[&path].replace(before, after));
        let report = check_project_with_overrides(&root, &overrides);
        assert!(report.session.is_none(), "{after}");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.message.contains(expected) && d.range.is_some()),
            "{after}: {:?}",
            report.diagnostics
        );
    }
    assert_eq!(original, project_source_texts(&root).unwrap());
}

#[test]
fn misspelled_params_cannot_load_as_defaults() {
    assert_rejected(&[
        ("params: { level: 0.5 }", "param: { level: 0.5 }", "param"),
        ("params: { level: 0.5 }", "params: { levle: 0.5 }", "levle"),
    ]);
    // Leaving every parameter to its default is written as `{}`.
    let (_temp, root) = small_project();
    let mut overrides = project_source_texts(&root).unwrap();
    let path = Utf8PathBuf::from(PROJECT_ROOT_FILE);
    let text = overrides[&path].replace("params: { level: 0.5 }", "params: {}");
    overrides.insert(path, text);
    let report = check_project_with_overrides(&root, &overrides);
    assert!(report.session.is_some(), "{:?}", report.diagnostics);
}

#[test]
fn parameter_values_are_typed_by_their_definitions() {
    assert_rejected(&[
        ("level: 0.5", "level: 1", "a float"),
        ("level: 0.5", "level: #ffffff", "a float"),
        ("level: 0.5", "level: [0.5]", "a float"),
        ("level: 0.5", "level: 0.5, tint: 0.5", "a color"),
        ("level: 0.5", "level: 0.5, level: 0.4", "once each"),
        (
            "level: 0.5",
            "tint: #000000, level: 0.5",
            "definition's order",
        ),
    ]);
}

#[test]
fn graph_variants_cannot_carry_other_variants_fields() {
    assert_rejected(&[
        (
            "OutputNode { position: (1.0, 0.0) }",
            "OutputNode { params: {}, position: (1.0, 0.0) }",
            "params",
        ),
        (
            "LayerNode { layer: main,",
            "LayerNode { operator: blend, layer: main,",
            "operator",
        ),
        (
            "edges: [Edge { from: main, to: output }]",
            "edges: [Edge { from: main, to: output.input }]",
            "output",
        ),
    ]);
}

#[test]
fn layout_groups_share_members_and_reject_unknown_repeated_cyclic_or_orphaned_items() {
    let shared = "  root: [left, right],\n  items: [\n    Group { name: left, description: none, members: [pixel] },\n    Group { name: right, description: none, members: [left, pixel] },\n";
    let (_temp, root) = small_project();
    let path = Utf8PathBuf::from(PROJECT_ROOT_FILE);
    let mut overrides = project_source_texts(&root).unwrap();
    let text = overrides[&path].replace("  root: [pixel],\n  items: [\n", shared);
    overrides.insert(path, text);
    let report = check_project_with_overrides(&root, &overrides);
    let session = report.session.expect("shared members load");
    let layout = session.project.layouts().next().unwrap();
    let id = |name: &str| {
        layout
            .iter_fixtures()
            .find(|item| item.name.as_str() == name)
            .unwrap()
            .id
    };
    // `pixel` belongs to both groups and keeps its first position in `right`.
    assert_eq!(layout.members(id("left")), [id("pixel")]);
    assert_eq!(layout.members(id("right")), [id("pixel")]);
    assert_eq!(
        layout.parents(id("pixel")),
        [Some(id("left")), Some(id("right"))]
    );
    assert_rejected(&[
        (
            "root: [pixel],",
            "root: [pixle],",
            "unknown layout member `pixle`",
        ),
        (
            "root: [pixel],",
            "root: [pixel, pixel],",
            "`pixel` is listed twice",
        ),
        (
            "  root: [pixel],\n  items: [\n",
            "  root: [pixel, a],\n  items: [\n    Group { name: a, description: none, members: [b] },\n    Group { name: b, description: none, members: [a] },\n",
            "contains itself",
        ),
        (
            "root: [pixel],",
            "root: [],",
            "`pixel` is not in `root` or any group",
        ),
    ]);
}

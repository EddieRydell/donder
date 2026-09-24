mod common;

use camino::{Utf8Path, Utf8PathBuf};
use donder_project_io::{check_package_with_overrides, project_source_texts};
use std::collections::BTreeSet;
use yaml_serde::Value;

#[derive(Clone, Debug)]
enum Step {
    Key(String),
    Index(usize),
}

fn at<'a>(mut value: &'a mut Value, steps: &[Step]) -> &'a mut Value {
    for step in steps {
        value = match step {
            Step::Key(key) => value
                .as_mapping_mut()
                .unwrap()
                .get_mut(Value::String(key.clone()))
                .unwrap(),
            Step::Index(index) => &mut value.as_sequence_mut().unwrap()[*index],
        };
    }
    value
}

// Exercise real authored shapes recursively. Sample one of each shape/context,
// so hundreds of identical fixture pixels do not repeat the same check.
fn mapping_paths(
    value: &Value,
    path: &mut Vec<Step>,
    seen: &mut BTreeSet<String>,
    output: &mut Vec<Vec<Step>>,
) {
    match value {
        Value::Mapping(map) => {
            let context = path.iter().rev().find_map(|step| match step {
                Step::Key(key) => Some(key.as_str()),
                _ => None,
            });
            // These two containers have authored names as keys; their values
            // are still visited and tested below.
            if !path.is_empty() && context != Some("params") {
                let keys = map
                    .keys()
                    .map(|key| key.as_str().unwrap())
                    .collect::<BTreeSet<_>>();
                let kind = map
                    .get(Value::String("type".into()))
                    .and_then(Value::as_str);
                if seen.insert(format!("{context:?}:{kind:?}:{keys:?}")) {
                    output.push(path.clone());
                }
            }
            for (key, child) in map {
                path.push(Step::Key(key.as_str().unwrap().to_owned()));
                mapping_paths(child, path, seen, output);
                path.pop();
            }
        }
        Value::Sequence(items) => {
            for (index, child) in items.iter().enumerate() {
                path.push(Step::Index(index));
                mapping_paths(child, path, seen, output);
                path.pop();
            }
        }
        _ => {}
    }
}

#[test]
fn every_starter_mapping_shape_rejects_extra_fields_at_the_source_location() {
    let root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/starter");
    let original = project_source_texts(&root).unwrap();
    let baseline = check_package_with_overrides(&root, &original);
    assert!(baseline.session.is_some(), "{:?}", baseline.diagnostics);
    let mut seen = BTreeSet::new();
    let mut checked = 0;
    for (path, source) in &original {
        if path.extension() != Some("donder")
            || path.as_str().ends_with(".effect.donder")
            || path.as_str().ends_with(".operator.donder")
        {
            continue;
        }
        let value: Value = yaml_serde::from_str(source).unwrap();
        let mut paths = Vec::new();
        mapping_paths(&value, &mut Vec::new(), &mut seen, &mut paths);
        for steps in paths {
            let mut edited = value.clone();
            at(&mut edited, &steps).as_mapping_mut().unwrap().insert(
                Value::String("unexpected_schema_field".into()),
                Value::String("schema_marker".into()),
            );
            let text = yaml_serde::to_string(&edited).unwrap();
            let (line, source_line) = text
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains("unexpected_schema_field:"))
                .unwrap();
            let mut overrides = original.clone();
            overrides.insert(path.clone(), text.clone());
            let report = check_package_with_overrides(&root, &overrides);
            assert!(report.session.is_none(), "accepted {path}:{steps:?}");
            let diagnostic = report
                .diagnostics
                .iter()
                .find(|d| {
                    d.message.contains("unknown field")
                        && d.message.contains("unexpected_schema_field")
                })
                .unwrap_or_else(|| panic!("{path}:{steps:?}: {:?}", report.diagnostics));
            assert_eq!(&diagnostic.path, path);
            let range = diagnostic.range.as_ref().unwrap();
            assert_eq!(range.start.line, line as u32);
            assert_eq!(
                range.start.character,
                source_line.find("schema_marker").unwrap() as u32
            );
            checked += 1;
        }
    }
    assert!(checked >= 30, "only exercised {checked} mapping shapes");
    assert_eq!(original, project_source_texts(&root).unwrap());
}

fn small_project() -> (tempfile::TempDir, Utf8PathBuf) {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().to_path_buf()).unwrap();
    std::fs::write(
        root.join("project.donder"),
        r#"imports:
- from: { documents: [effect.effect.donder] }
  as: fx
main:
  type: project
  setup: setup
  sequences: [show]
setup:
  type: setup
  layout: layout
  patch: patch
  controllers: []
layout:
  type: layout
  fixtures:
  - id: 1
    name: Pixel
    type: fixture
    definition: pixel
pixel:
  type: fixture
  pixels: [{ id: 1, diameter: 0.01 }]
patch:
  type: patch
  routes: []
show:
  type: sequence
  duration: 10s
  frame_rate: 60
  layers: [{ id: 0, name: Main, color: '#ffffff', enabled: true }]
  effects:
  - id: 1
    layer_id: 0
    start: 0s
    duration: 10s
    target: { layout: layout, fixture: 1 }
    scope: per_fixture
    effect: fx.Defaults
    params: { level: { type: float, value: 0.5 } }
  composition_graph:
    nodes:
    - { id: 1, type: layer, layer_id: 0, position: { x: 0, y: 0 } }
    - { id: 2, type: output, position: { x: 1, y: 0 } }
    edges: [{ from: 1, from_port: output, to: 2, to_port: input }]
  automation_clips: []
"#,
    )
    .unwrap();
    std::fs::write(root.join("effect.effect.donder"), "effect Defaults { param float level = 0.5; color sample() { return rgb(level, level, level); } }").unwrap();
    common::write_project_package(&root);
    common::load_project_package(&root);
    (temporary, root)
}

#[test]
fn misspelled_optional_params_cannot_load_as_defaults_or_be_saved() {
    let (_temp, root) = small_project();
    let original = project_source_texts(&root).unwrap();
    let path = Utf8PathBuf::from("project.donder");
    let mut overrides = original.clone();
    overrides.insert(
        path.clone(),
        original[&path].replace("    params:", "    param:"),
    );
    let report = check_package_with_overrides(&root, &overrides);
    assert!(report.session.is_none());
    assert!(
        report.diagnostics.iter().any(|d| d.message
            == "effect instance has an unknown field `param`"
            && d.range.is_some()),
        "{:?}",
        report.diagnostics
    );
    assert_eq!(original, project_source_texts(&root).unwrap());
    // Omission is still legitimate, and the authored default is preserved.
    overrides.insert(
        path.clone(),
        original[&path].replace("    params: { level: { type: float, value: 0.5 } }\n", ""),
    );
    let report = check_package_with_overrides(&root, &overrides);
    assert!(report.session.is_some(), "{:?}", report.diagnostics);
}

#[test]
fn graph_variants_and_non_string_keys_cannot_be_silently_discarded() {
    let (_temp, root) = small_project();
    let original = project_source_texts(&root).unwrap();
    let path = Utf8PathBuf::from("project.donder");
    for (before, after, expected) in [
        (
            "type: output, position:",
            "type: output, params: {}, position:",
            "graph node has an unknown field `params`",
        ),
        (
            "type: layer, layer_id:",
            "type: layer, operator: blend, layer_id:",
            "graph node has an unknown field `operator`",
        ),
        (
            "frame_rate: 60",
            "frame_rate: 60\n  42: ignored",
            "sequence keys must be strings",
        ),
    ] {
        assert!(original[&path].contains(before));
        let mut overrides = original.clone();
        overrides.insert(path.clone(), original[&path].replace(before, after));
        let report = check_package_with_overrides(&root, &overrides);
        assert!(report.session.is_none());
        assert!(
            report.diagnostics.iter().any(|d| d.message == expected),
            "{:?}",
            report.diagnostics
        );
    }
}

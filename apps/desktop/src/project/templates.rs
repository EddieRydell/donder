use std::fs;

use camino::Utf8Path;
use donder_project_io::{PROJECT_ROOT_FILE, ProjectMetadata};

pub(crate) struct ProjectBoilerplateFile {
    path: &'static str,
    text: String,
}

pub(crate) fn new_project_files(
    project_name: &str,
    initial_color: &str,
) -> Result<Vec<ProjectBoilerplateFile>, String> {
    let initial_color = donder_language::values::Color::from_hex(initial_color)
        .ok_or("Invalid initial project color.")?
        .to_hex();
    let project_id = object_key_from_name(project_name);
    let config = ProjectMetadata::default();
    Ok(vec![
        ProjectBoilerplateFile {
            path: "AGENTS.md",
            text: include_str!("../../../../examples/starter/AGENTS.md").to_string(),
        },
        ProjectBoilerplateFile {
            path: PROJECT_ROOT_FILE,
            text: config.initialize_document(&format!(
                r#"imports:
- from:
    documents:
    - effects/standard.effect.donder
    - effects/impact-burst.effect.donder
    - effects/mark-impact-burst.effect.donder
  as: effects
- from:
    documents:
    - operators/standard.operator.donder
  as: operators
{project_id}:
  type: project
  setup:
    type: setup
    layout:
      type: layout
      fixtures: []
    patch:
      type: patch
      routes: []
    controllers: []
  sequences:
  - id: 1
    type: sequence
    duration: 60s
    frame_rate: 60
    audio: null
    mark_collections:
    - key: marks
      name: Marks
      color: '{initial_color}'
      marks: []
    layers:
    - id: 0
      name: Default
      color: '{initial_color}'
      enabled: true
    effects: []
    composition_graph:
      nodes:
      - id: 1
        position:
          x: 80.0
          y: 80.0
        type: layer
        layer_id: 0
      - id: 2
        position:
          x: 420.0
          y: 80.0
        type: output
      edges:
      - from: 1
        from_port: output
        to: 2
        to_port: input
    automation_clips: []
"#
            ))?,
        },
        ProjectBoilerplateFile {
            path: "effects/standard.effect.donder",
            text: include_str!("../../../../examples/starter/effects/standard.effect.donder")
                .to_string(),
        },
        ProjectBoilerplateFile {
            path: "operators/standard.operator.donder",
            text: include_str!("../../../../examples/starter/operators/standard.operator.donder")
                .to_string(),
        },
        ProjectBoilerplateFile {
            path: "effects/impact-burst.effect.donder",
            text: include_str!("../../../../examples/starter/effects/impact-burst.effect.donder")
                .to_string(),
        },
        ProjectBoilerplateFile {
            path: "effects/mark-impact-burst.effect.donder",
            text: include_str!(
                "../../../../examples/starter/effects/mark-impact-burst.effect.donder"
            )
            .to_string(),
        },
    ])
}

pub(crate) fn write_new_project_files(
    root: &Utf8Path,
    files: &[ProjectBoilerplateFile],
) -> Result<(), String> {
    fs::create_dir(root).map_err(|error| error.to_string())?;
    let result = (|| {
        for file in files {
            let path = root.join(file.path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(path, &file.text).map_err(|error| error.to_string())?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(root);
    }
    result
}

fn object_key_from_name(name: &str) -> String {
    let mut key = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            key.push(character.to_ascii_lowercase());
        } else if !key.ends_with('_') {
            key.push('_');
        }
    }
    let key = key.trim_matches('_').to_string();
    if key.is_empty()
        || matches!(key.as_str(), "workspace" | "imports")
        || key.as_bytes().first().is_some_and(u8::is_ascii_digit)
    {
        format!("project_{key}")
    } else {
        key
    }
}

#[cfg(test)]
pub(crate) fn new_test_project_files(
    project_name: &str,
) -> Result<Vec<ProjectBoilerplateFile>, String> {
    let css = include_str!("../../frontend/src/styles.css");
    let color = css
        .split_once("--donder-default-sequence-color:")
        .expect("Project color CSS token")
        .1
        .split_once(';')
        .expect("CSS token terminator")
        .0
        .trim();
    new_project_files(project_name, color)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use camino::Utf8PathBuf;

    use super::*;

    #[test]
    fn new_project_template_loads_as_empty_authoring_project() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = Utf8PathBuf::from_path_buf(
            std::env::temp_dir().join(format!("donder-template-{nonce}")),
        )
        .unwrap();
        let files = new_test_project_files("Template Test").unwrap();
        assert_eq!(
            files
                .iter()
                .filter(|file| file.path.ends_with(".donder"))
                .count(),
            5
        );
        write_new_project_files(&root, &files).unwrap();
        let session = donder_project_io::load_project(&root).unwrap();
        let setup = session
            .project
            .setup(session.project.root().setup.id())
            .unwrap();
        assert!(session.project.reusable_setups().is_empty());
        assert!(session.project.reusable_layouts().is_empty());
        assert!(session.project.reusable_patches().is_empty());
        assert!(session.project.reusable_sequences().is_empty());
        assert_eq!(session.project.root().sequences.len(), 1);
        assert!(
            session
                .project
                .layout(setup.layout.id())
                .unwrap()
                .fixtures
                .is_empty()
        );
        assert!(
            session
                .project
                .patch(setup.patch.id())
                .unwrap()
                .routes
                .is_empty()
        );
        assert!(setup.controllers.is_empty());
        assert!(
            session
                .project
                .definitions()
                .fixtures
                .definitions
                .is_empty()
        );
        assert_eq!(
            session.project.definitions().operators.definitions.len(),
            10
        );
        for name in [
            "MarkPulse",
            "MarkChase",
            "MarkWipe",
            "MarkImpactBurst",
            "ImpactBurst",
        ] {
            assert!(
                session
                    .project
                    .definitions()
                    .effects
                    .definitions
                    .keys()
                    .any(|id| id.0.object() == name),
                "bundled effect {name} must be reachable from the new project"
            );
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn initial_sequence_can_reference_every_bundled_operator() {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temporary.path().join("project")).unwrap();
        let files = new_test_project_files("Template Test").unwrap();
        write_new_project_files(&root, &files).unwrap();
        let template = &files
            .iter()
            .find(|file| file.path == PROJECT_ROOT_FILE)
            .unwrap()
            .text;
        for (name, inputs) in [
            ("Max", &["a", "b"][..]),
            ("Add", &["a", "b"][..]),
            ("Multiply", &["a", "b"][..]),
            ("IntensityModulate", &["source", "mask"][..]),
            ("Dim", &["input"][..]),
            ("Invert", &["input"][..]),
            ("Colorize", &["input"][..]),
            ("Delay", &["input"][..]),
            ("Echo", &["input"][..]),
            ("HueShift", &["source"][..]),
        ] {
            let mut document: yaml_serde::Value = yaml_serde::from_str(template).unwrap();
            let graph = &mut document["template_test"]["sequences"][0]["composition_graph"];
            graph["nodes"].as_sequence_mut().unwrap().push(
                yaml_serde::from_str(&format!(
                    "id: 3\nposition: {{x: 240.0, y: 80.0}}\ntype: operator\noperator: operators.{name}\n"
                )).unwrap(),
            );
            let edges = graph["edges"].as_sequence_mut().unwrap();
            edges.clear();
            for input in inputs {
                edges.push(
                    yaml_serde::from_str(&format!(
                        "from: 1\nfrom_port: output\nto: 3\nto_port: {input}\n"
                    ))
                    .unwrap(),
                );
            }
            edges.push(
                yaml_serde::from_str("from: 3\nfrom_port: output\nto: 2\nto_port: input\n")
                    .unwrap(),
            );
            fs::write(
                root.join(PROJECT_ROOT_FILE),
                yaml_serde::to_string(&document).unwrap(),
            )
            .unwrap();
            let session = donder_project_io::load_project(&root)
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            let definition = session
                .project
                .definitions()
                .operators
                .definitions
                .values()
                .find(|definition| definition.declaration_name == name)
                .unwrap();
            if name == "HueShift" {
                let shift = &definition.params()[0];
                assert_eq!(shift.name.as_str(), "shift");
                assert_eq!(shift.ty, donder_language::dsl::Type::Float);
                assert!(!shift.fixed);
                assert_eq!(shift.default, Some(donder_language::dsl::Value::Float(0.0)));
            }
        }
    }

    #[test]
    fn stanford_standard_operators_include_canonical_definitions() {
        let canonical = donder_language::dsl::compile_operators(include_str!(
            "../../../../examples/starter/operators/standard.operator.donder"
        ))
        .unwrap();
        let stanford = donder_language::dsl::compile_operators(include_str!(
            "../../../../examples/stanford_room/operators/standard.operator.donder"
        ))
        .unwrap();
        for definition in canonical {
            assert_eq!(
                stanford
                    .iter()
                    .find(|operator| operator.name() == definition.name()),
                Some(&definition),
                "operator {} differs from the stock definition",
                definition.name().as_str()
            );
        }
    }
}

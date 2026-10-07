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
    let project = project_name_from_text(project_name);
    let config = ProjectMetadata::default();
    let root = format!(
        r#"import effects from <effects/standard.donder>, <effects/impact-burst.donder>, <effects/mark-impact-burst.donder>;
import vixen from <effects/vixen.donder>;
import operators from <operators/standard.donder>;

Project {project} {{
  format: {format},
  id: "{id}",
  description: none,
  setup: Setup {{
    description: none,
    layout: Layout {{ description: none, items: [] }},
    patch: Patch {{ description: none, routes: [] }},
    controllers: [],
  }},
  sequences: [
    Sequence main {{
      description: none,
      duration: 60s,
      frame_rate: 60,
      audio: none,
      marks: [MarkCollection {{ name: marks, description: none, color: {initial_color}, times: [] }}],
      layers: [Layer {{ name: default, description: none, color: {initial_color}, enabled: true }}],
      clips: [],
      graph: Graph {{
        nodes: [LayerNode {{ layer: default, position: (80.0, 80.0) }}, OutputNode {{ position: (420.0, 80.0) }}],
        edges: [Edge {{ from: default, to: output }}],
      }},
      automation: [],
    }},
  ],
}}
"#,
        format = config.format_version,
        id = config.project_id,
    );
    let (document, diagnostics) = donder_language::data::parse(&root);
    if let Some(diagnostic) = diagnostics.first() {
        return Err(format!("Invalid project template: {}", diagnostic.message));
    }
    Ok(vec![
        ProjectBoilerplateFile {
            path: "AGENTS.md",
            text: include_str!("../../../../examples/starter/AGENTS.md").to_string(),
        },
        ProjectBoilerplateFile {
            path: PROJECT_ROOT_FILE,
            text: donder_language::data::print(&document),
        },
        ProjectBoilerplateFile {
            path: "effects/standard.donder",
            text: include_str!("../../../../examples/starter/effects/standard.donder").to_string(),
        },
        ProjectBoilerplateFile {
            path: "effects/vixen.donder",
            text: include_str!("../../../../examples/starter/effects/vixen.donder").to_string(),
        },
        ProjectBoilerplateFile {
            path: "operators/standard.donder",
            text: include_str!("../../../../examples/starter/operators/standard.donder")
                .to_string(),
        },
        ProjectBoilerplateFile {
            path: "effects/impact-burst.donder",
            text: include_str!("../../../../examples/starter/effects/impact-burst.donder")
                .to_string(),
        },
        ProjectBoilerplateFile {
            path: "effects/mark-impact-burst.donder",
            text: include_str!("../../../../examples/starter/effects/mark-impact-burst.donder")
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

/// The project's declaration name: its display name in `snake_case`.
fn project_name_from_text(name: &str) -> String {
    let name = donder_language::names::name_from_text(name, "project");
    if matches!(name.as_str(), "import" | "from" | "none" | "true" | "false")
        || name.starts_with('_')
    {
        format!("project_{}", name.trim_start_matches('_'))
    } else {
        name
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
    use camino::Utf8PathBuf;

    use super::*;

    #[test]
    fn new_project_template_loads_as_empty_authoring_project() {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temporary.path().join("project")).unwrap();
        let files = new_test_project_files("Template Test").unwrap();
        assert_eq!(
            files
                .iter()
                .filter(|file| file.path.ends_with(".donder"))
                .count(),
            6
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
            11
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
        let operators = [
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
        ];
        // Chain every operator between the layer node and the output node.
        let mut nodes = vec![
            "LayerNode { layer: default, position: (80.0, 80.0) }".to_string(),
            "OutputNode { position: (420.0, 80.0) }".to_string(),
        ];
        let mut edges = Vec::new();
        let mut previous = "default".to_string();
        for (index, (name, inputs)) in operators.iter().enumerate() {
            let node = format!("node_{index}");
            nodes.push(format!(
                "OperatorNode {{ name: {node}, operator: operators.{name}, params: {{}}, position: (240.0, 80.0) }}"
            ));
            for input in *inputs {
                edges.push(format!("Edge {{ from: {previous}, to: {node}.{input} }}"));
            }
            previous = node;
        }
        edges.push(format!("Edge {{ from: {previous}, to: output }}"));
        let start = template.find("graph: Graph {").unwrap();
        let end = template.find("automation: []").unwrap();
        let text = format!(
            "{}graph: Graph {{ nodes: [{}], edges: [{}] }},
      {}",
            &template[..start],
            nodes.join(", "),
            edges.join(", "),
            &template[end..]
        );
        fs::write(root.join(PROJECT_ROOT_FILE), text).unwrap();
        let session = donder_project_io::load_project(&root).unwrap();
        for (name, _) in operators {
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
                assert_eq!(shift.default, Some(donder_language::dsl::Value::Float(0.0)));
            }
        }
    }

    #[test]
    fn stanford_standard_operators_include_canonical_definitions() {
        let canonical = donder_language::dsl::compile_operators(include_str!(
            "../../../../examples/starter/operators/standard.donder"
        ))
        .unwrap();
        let stanford = donder_language::dsl::compile_operators(include_str!(
            "../../../../examples/stanford_room/operators/standard.donder"
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

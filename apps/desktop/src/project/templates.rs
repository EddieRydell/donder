use std::fs;

use camino::Utf8Path;
use donder_project_io::{PROJECT_CONFIG_FILE, ProjectConfig};

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
    let config = ProjectConfig::new("project.donder".into());
    Ok(vec![
        ProjectBoilerplateFile {
            path: PROJECT_CONFIG_FILE,
            text: config.to_text()?,
        },
        ProjectBoilerplateFile {
            path: "project.donder",
            text: format!(
                r#"{project_id}:
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
            ),
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
    if key.is_empty() || key.as_bytes().first().is_some_and(u8::is_ascii_digit) {
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
            1
        );
        write_new_project_files(&root, &files).unwrap();
        let session = donder_project_io::load_project(&root).unwrap();
        let setup = session
            .project
            .setup(session.project.root.setup.id())
            .unwrap();
        assert!(session.project.setups.is_empty());
        assert!(session.project.layouts.is_empty());
        assert!(session.project.patches.is_empty());
        assert!(session.project.sequences.is_empty());
        assert_eq!(session.project.root.sequences.len(), 1);
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
        assert!(session.project.definitions.fixtures.definitions.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }
}

use std::collections::BTreeMap;
use std::fs;

use camino::Utf8Path;
use dawn_package::{ExportGroup, Lockfile, PackageManifest, ProjectManifest, canonical_json};
use semver::VersionReq;
use uuid::Uuid;

pub(crate) struct ProjectBoilerplateFile {
    path: &'static str,
    text: String,
}

pub(crate) fn new_project_files(project_name: &str) -> Result<Vec<ProjectBoilerplateFile>, String> {
    let project_id = object_key_from_name(project_name);
    let manifest = PackageManifest {
        manifest_version: dawn_package::MANIFEST_VERSION,
        module_id: Uuid::new_v4(),
        language_version: "0.1".to_string(),
        requires_dawn: VersionReq::parse(">=0.1.0, <1.0.0").map_err(|error| error.to_string())?,
        project: Some(ProjectManifest {
            entrypoint: "project.dawn".to_string(),
        }),
        publication: None,
        exports: BTreeMap::from([(
            "project".to_string(),
            ExportGroup {
                documents: vec!["project.dawn".to_string()],
            },
        )]),
        dependencies: BTreeMap::new(),
        assets: BTreeMap::new(),
    };
    let registry = dawn_package::RegistryConfig::read().map_err(|error| error.to_string())?;
    let lockfile =
        Lockfile::new(&manifest, &registry.registry).map_err(|error| error.to_string())?;
    let manifest_text =
        String::from_utf8(canonical_json(&manifest).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let lockfile_text =
        String::from_utf8(canonical_json(&lockfile).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;

    Ok(vec![
        ProjectBoilerplateFile {
            path: dawn_package::MANIFEST_FILE,
            text: manifest_text,
        },
        ProjectBoilerplateFile {
            path: dawn_package::LOCK_FILE,
            text: lockfile_text,
        },
        ProjectBoilerplateFile {
            path: "project.dawn",
            text: format!(
                "imports:\n- from:\n    documents:\n    - setups/main.setup.dawn\n  as: setups\n- from:\n    documents:\n    - sequences/main.sequence.dawn\n  as: sequences\n{project_id}:\n  type: project\n  setup: setups.main\n  sequences:\n  - sequences.main\n"
            ),
        },
        ProjectBoilerplateFile {
            path: "setups/main.setup.dawn",
            text: "imports:\n- from:\n    documents:\n    - layouts/main.layout.dawn\n  as: layout\n- from:\n    documents:\n    - patches/main.patch.dawn\n  as: patches\nmain:\n  type: setup\n  elements: layout.elements\n  preview: layout.preview\n  patch: patches.main\n  controllers: []\n".to_string(),
        },
        ProjectBoilerplateFile {
            path: "layouts/main.layout.dawn",
            text: "elements:\n  type: element_tree\n  roots: []\n  nodes: []\npreview:\n  type: preview_layout\n  element_tree: elements\n  props: []\n".to_string(),
        },
        ProjectBoilerplateFile {
            path: "patches/main.patch.dawn",
            text: "main:\n  type: patch\n  nodes: []\n  edges: []\n".to_string(),
        },
        ProjectBoilerplateFile {
            path: "sequences/main.sequence.dawn",
            text: sequence_boilerplate("main", 60.0, 60),
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

fn sequence_boilerplate(object_key: &str, duration_seconds: f32, frame_rate: u32) -> String {
    format!(
        "{object_key}:\n  type: sequence\n  duration: {}s\n  frame_rate: {frame_rate}\n  audio: null\n  mark_collections:\n  - key: marks\n    name: Marks\n    color: '#38bdf8'\n    marks: []\n  layers:\n  - id: 0\n    name: Default\n    color: '#38bdf8'\n    enabled: true\n  effects: []\n  composition_graph:\n    nodes:\n    - id: 1\n      position:\n        x: 80.0\n        y: 80.0\n      type: layer\n      layer_id: 0\n    - id: 2\n      position:\n        x: 420.0\n        y: 80.0\n      type: output\n    edges:\n    - from: 1\n      from_port: output\n      to: 2\n      to_port: input\n  automation_clips: []\n  control_clips: []\n",
        seconds_literal(duration_seconds)
    )
}

fn seconds_literal(seconds: f32) -> String {
    if seconds.fract() == 0.0 {
        format!("{seconds:.0}")
    } else {
        seconds.to_string()
    }
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
        let root =
            Utf8PathBuf::from_path_buf(std::env::temp_dir().join(format!("dawn-template-{nonce}")))
                .unwrap();
        let files = new_project_files("Template Test").unwrap();
        assert!(
            files
                .iter()
                .any(|file| file.path == "layouts/main.layout.dawn")
        );
        assert!(!files.iter().any(|file| file.path.contains("display")));
        write_new_project_files(&root, &files).unwrap();
        let session = dawn_project_io::load_package(&root).unwrap().session;
        let setup = session
            .project
            .setups
            .get(&session.project.root.setup)
            .unwrap();
        assert!(session.project.element_trees.contains_key(&setup.elements));
        assert!(session.project.preview_layouts.contains_key(&setup.preview));
        assert!(session.project.patches.contains_key(&setup.patch));
        assert!(setup.controllers.is_empty());
        assert!(
            session.project.element_trees[&setup.elements]
                .nodes
                .is_empty()
        );
        assert!(
            session.project.preview_layouts[&setup.preview]
                .props
                .is_empty()
        );
        assert!(session.project.patches[&setup.patch].nodes.is_empty());
        assert!(session.project.definitions.props.definitions.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }
}

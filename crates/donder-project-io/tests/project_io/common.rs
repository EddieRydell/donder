use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

/// A root document declaring `Project main` with a fresh project id. `fields`
/// are written after `description`; imports come first.
pub fn root_document(imports: &str, fields: &str) -> String {
    let metadata = donder_project_io::ProjectMetadata::default();
    format!(
        "{imports}\nProject main {{\n  format: {},\n  id: \"{}\",\n  description: none,\n{fields}}}\n",
        metadata.format_version, metadata.project_id
    )
}

/// A project whose root imports a setup, its display and patch, and
/// `sequence` as `sequence.data.donder`.
pub fn write_imported_sequence_project(root: &Utf8Path, sequence: &str) {
    fs::write(
        root.join(donder_project_io::PROJECT_ROOT_FILE),
        root_document(
            "import setups from <setup.data.donder>;\nimport sequences from <sequence.data.donder>;\n",
            "  setup: setups.main,\n  sequences: [sequences.main],\n",
        ),
    )
    .unwrap();
    fs::write(root.join("setup.data.donder"), SETUP).unwrap();
    fs::write(root.join("display.data.donder"), DISPLAY).unwrap();
    fs::write(root.join("patch.data.donder"), PATCH).unwrap();
    fs::write(root.join("sequence.data.donder"), sequence).unwrap();
}

pub const SETUP: &str = r#"import display from <display.data.donder>;
import patches from <patch.data.donder>;

Setup main {
  description: none,
  layout: display.main,
  patch: patches.main,
  controllers: [],
}
"#;

pub const DISPLAY: &str = r#"FixtureDefinition pixel {
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

Layout main {
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
"#;

pub const PATCH: &str = "Patch main { description: none, routes: [] }\n";

/// A valid sequence named `main` with nothing in it. Tests derive invalid
/// variants by replacing its text.
pub const MINIMAL_SEQUENCE: &str = r#"Sequence main {
  description: none,
  duration: 1s,
  frame_rate: 60,
  audio: none,
  marks: [],
  layers: [],
  clips: [],
  graph: Graph { nodes: [OutputNode { position: (0.0, 0.0) }], edges: [] },
  automation: [],
}
"#;

pub fn load_project(root: &Utf8Path) -> donder_project_io::ProjectSession {
    donder_project_io::load_project(root).unwrap()
}

pub fn starter_root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter")
}

/// Copies the starter's Donder documents into `<temporary>/project` without
/// loading it. Other files, including ignored local audio, are left out so tests
/// do not depend on the checkout. Documents get LF line endings so tests can
/// anchor on multi-line text regardless of how the checkout stores them.
pub fn starter_copy() -> (tempfile::TempDir, Utf8PathBuf) {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path())
        .unwrap()
        .join("project");
    copy_tree(&starter_root(), &root);
    (temporary, root)
}

fn copy_tree(source: &Utf8Path, destination: &Utf8Path) {
    for entry in source.read_dir_utf8().unwrap() {
        let path = entry.unwrap().into_path();
        let target = destination.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_tree(&path, &target);
        } else if path.extension() == Some("donder") {
            fs::create_dir_all(destination).unwrap();
            fs::write(
                target,
                fs::read_to_string(&path).unwrap().replace("\r\n", "\n"),
            )
            .unwrap();
        }
    }
}

use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

/// A root document declaring `Project main` with a fresh project id and no
/// consoles. `fields` are written after `description`; imports come first.
pub(crate) fn root_document(imports: &str, fields: &str) -> String {
    let metadata = donder_project_io::ProjectMetadata::default();
    format!(
        "{imports}\nProject main {{\n  format: {},\n  description: none,\n{fields}  consoles: [],\n}}\n",
        metadata.format_version
    )
}

/// A project whose root imports a setup, its display and patch, and
/// `sequence` as `sequence.data.donder`.
pub(crate) fn write_imported_sequence_project(root: &Utf8Path, sequence: &str) {
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

pub(crate) const SETUP: &str = r#"import display from <display.data.donder>;
import patches from <patch.data.donder>;

Setup main {
  description: none,
  layout: display.main,
  patch: patches.main,
  controllers: [],
  input_devices: [],
}
"#;

pub(crate) const DISPLAY: &str = r#"FixtureDefinition pixel {
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

pub(crate) const PATCH: &str = "Patch main { description: none, routes: [] }\n";

/// A valid sequence named `main` with nothing in it. Tests derive invalid
/// variants by replacing its text.
pub(crate) const MINIMAL_SEQUENCE: &str = r#"Sequence main {
  description: none,
  duration: 1s,
  frame_rate: 60,
  audio: none,
  marks: [],
  layers: [],
  clips: [],
  graph: Graph { nodes: [RootNode { position: (0.0, 0.0) }], edges: [] },
  automation: [],
}
"#;

pub(crate) fn load_project(root: &Utf8Path) -> donder_project_io::ProjectSession {
    donder_project_io::check_project(root, &Default::default(), &mut Default::default())
        .into_session()
        .unwrap()
}

pub(crate) fn starter_root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter")
}

/// Copies the starter's Donder documents into `<temporary>/project` without
/// loading it. Other files, including ignored local audio, are left out so tests
/// do not depend on the checkout. Documents get LF line endings so tests can
/// anchor on multi-line text regardless of how the checkout stores them.
pub(crate) fn starter_copy() -> (tempfile::TempDir, Utf8PathBuf) {
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

/// An ownership slot by its owner and position, as tests name them.
#[derive(Clone, Copy)]
pub(crate) enum Slot {
    ProjectSetup,
    ProjectSequence(usize),
    ProjectConsole(usize),
    SetupLayout(donder_model::SetupId),
    SetupPatch(donder_model::SetupId),
    SetupController {
        setup: donder_model::SetupId,
        index: usize,
    },
    SetupInputDevice {
        setup: donder_model::SetupId,
        index: usize,
    },
    LayoutFixture {
        layout: donder_model::LayoutId,
        fixture: donder_model::FixtureInstanceId,
    },
}

/// The site of the member `slot` holds in `project`.
pub(crate) fn site(
    project: &donder_model::DonderProject,
    slot: Slot,
) -> donder_model::OwnershipSite {
    use donder_model::{ObjectId, OwnershipSite};
    let root = project.root();
    let setup = |id| project.get::<donder_model::Setup>(id).unwrap();
    let (owner, member): (ObjectId, ObjectId) = match slot {
        Slot::ProjectSetup => (root.id.into(), (*root.setup.id()).into()),
        Slot::ProjectSequence(index) => (root.id.into(), (*root.sequences[index].id()).into()),
        Slot::ProjectConsole(index) => (root.id.into(), (*root.consoles[index].id()).into()),
        Slot::SetupLayout(id) => (id.into(), (*setup(id).layout.id()).into()),
        Slot::SetupPatch(id) => (id.into(), (*setup(id).patch.id()).into()),
        Slot::SetupController { setup: id, index } => {
            (id.into(), (*setup(id).controllers[index].id()).into())
        }
        Slot::SetupInputDevice { setup: id, index } => {
            (id.into(), (*setup(id).input_devices[index].id()).into())
        }
        Slot::LayoutFixture { layout, fixture } => {
            return OwnershipSite::Fixture { layout, fixture };
        }
    };
    OwnershipSite::Member { owner, member }
}

/// The diagnostics a check gives the document at `path` when its text is
/// `text`, in a directory with no project to load.
pub(crate) fn document_diagnostics(
    path: &Utf8Path,
    text: &str,
) -> Vec<donder_project_io::IoDiagnostic> {
    let overrides =
        donder_project_io::SourceOverrides::from([(path.to_path_buf(), text.to_string())]);
    let empty = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(empty.path()).unwrap();
    donder_project_io::check_project(root, &overrides, &mut Default::default())
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.path == path)
        .collect()
}

use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

pub fn write_workspace_metadata(root: &Utf8Path) {
    let path = root.join(donder_project_io::PROJECT_ROOT_FILE);
    let source = std::fs::read_to_string(&path).unwrap();
    if source.lines().any(|line| line.starts_with("workspace:")) {
        donder_project_io::ProjectMetadata::parse(&source).unwrap();
        return;
    }
    let metadata = donder_project_io::ProjectMetadata::default();
    let separator = if source.ends_with('\n') { "" } else { "\n" };
    // Append metadata so diagnostic fixtures retain their authored line numbers.
    std::fs::write(
        path,
        format!(
            "{source}{separator}workspace:\n  format_version: {}\n  project_id: {}\n",
            metadata.format_version, metadata.project_id
        ),
    )
    .unwrap();
}

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

use camino::Utf8Path;
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

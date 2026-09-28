use camino::Utf8Path;
pub fn write_project_config(root: &Utf8Path) {
    donder_project_io::ProjectConfig::new("project.donder".into())
        .write(root)
        .unwrap();
}
pub fn load_project(root: &Utf8Path) -> donder_project_io::ProjectSession {
    donder_project_io::load_project(root).unwrap()
}

use camino::{Utf8Path, Utf8PathBuf};
use donder_project_io::{
    PROJECT_ROOT_FILE, ProjectMetadata, check_project, export_editable_project, load_project,
    plan_path_change,
};
use std::fs;

fn starter() -> (tempfile::TempDir, Utf8PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf())
        .unwrap()
        .join("project");
    let source = Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    copy_sources(&source, &root);
    (temp, root)
}
fn copy_sources(source: &Utf8Path, destination: &Utf8Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let path = Utf8PathBuf::from_path_buf(entry.unwrap().path()).unwrap();
        let target = destination.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_sources(&path, &target);
        } else if path.extension() == Some("donder") {
            fs::copy(path, target).unwrap();
        }
    }
}

#[test]
fn starter_loads_offline_without_lock_or_audio_downloads() {
    let (_temp, root) = starter();
    let session = load_project(&root).unwrap();
    assert!(!session.source.documents.is_empty());
    assert!(session.source.referenced_assets.is_empty());
    assert!(!root.join("donder.lock").exists());
}

#[test]
fn local_audio_needs_no_inventory_or_configuration_update() {
    let (_temp, root) = starter();
    let config = fs::read(root.join(PROJECT_ROOT_FILE)).unwrap();
    let sequence = root.join("sequences/layer_test.sequence.donder");
    let text = fs::read_to_string(&sequence)
        .unwrap()
        .replace("audio: null", "audio: audio/test.wav");
    fs::write(&sequence, text).unwrap();
    let missing = check_project(&root);
    assert!(missing.session.is_none());
    assert!(!missing.recovery.documents.is_empty());
    fs::create_dir_all(root.join("audio")).unwrap();
    fs::write(
        root.join("audio/test.wav"),
        b"asset bytes; decoding belongs to playback",
    )
    .unwrap();
    let session = load_project(&root).unwrap();
    assert_eq!(session.source.referenced_assets.len(), 1);
    assert_eq!(fs::read(root.join(PROJECT_ROOT_FILE)).unwrap(), config);
}

#[test]
fn unused_broken_download_does_not_invalidate_the_show() {
    let (_temp, root) = starter();
    fs::write(root.join("download.donder"), "broken: [\n").unwrap();
    let report = check_project(&root);
    assert!(report.session.is_some(), "{:?}", report.diagnostics);
    assert!(
        report
            .recovery
            .documents
            .contains_key(Utf8Path::new("download.donder"))
    );
    assert!(
        !donder_project_io::check_document_text(Utf8Path::new("download.donder"), "broken: [\n")
            .is_empty()
    );
}

#[test]
fn root_stays_fixed_and_copy_preserves_workspace_identity() {
    let (_temp, root) = starter();
    let session = load_project(&root).unwrap();
    let metadata = ProjectMetadata::read(&root).unwrap();
    let error = plan_path_change(
        &session,
        Utf8Path::new("project.donder"),
        Utf8Path::new("show.donder"),
    )
    .unwrap_err();
    assert!(error.contains("must remain at the project root"));
    assert_eq!(load_project(&root).unwrap().project, session.project);
    let destination = root.parent().unwrap().join("copy");
    export_editable_project(&session, &destination).unwrap();
    assert_eq!(load_project(&destination).unwrap().project, session.project);
    assert_eq!(ProjectMetadata::read(&destination).unwrap(), metadata);
    assert!(!destination.join("donder.json").exists());
    assert!(!destination.join("donder.lock").exists());
}

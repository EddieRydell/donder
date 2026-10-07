use crate::common::starter_copy as starter;
use camino::Utf8Path;
use donder_project_io::{
    PROJECT_ROOT_FILE, ProjectMetadata, check_project, export_editable_project, load_project,
    plan_path_change,
};
use std::fs;

#[test]
fn local_audio_needs_no_inventory_or_configuration_update() {
    let (_temp, root) = starter();
    let config = fs::read(root.join(PROJECT_ROOT_FILE)).unwrap();
    let sequence = root.join("sequences/layer_test.data.donder");
    let text = fs::read_to_string(&sequence)
        .unwrap()
        .replace("audio: none", "audio: <audio/test.wav>");
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
    fs::write(root.join("download.data.donder"), "Curve broken {\n").unwrap();
    let report = check_project(&root);
    assert!(report.session.is_some(), "{:?}", report.diagnostics);
    assert!(
        report
            .recovery
            .documents
            .contains_key(Utf8Path::new("download.data.donder"))
    );
    assert!(
        !donder_project_io::check_document_text(
            Utf8Path::new("download.data.donder"),
            "Curve broken {\n"
        )
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
        Utf8Path::new("project.data.donder"),
        Utf8Path::new("show.data.donder"),
    )
    .unwrap_err();
    assert!(error.contains("must remain at the project root"));
    let destination = root.parent().unwrap().join("copy");
    export_editable_project(&session, &destination).unwrap();
    assert_eq!(load_project(&destination).unwrap().project, session.project);
    assert_eq!(ProjectMetadata::read(&destination).unwrap(), metadata);
}

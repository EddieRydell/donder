mod common;

use camino::Utf8PathBuf;
use donder_project_io::{export_editable_project, load_project};
use std::fs;

#[test]
fn copy_preserves_local_imports_and_audio_and_is_independently_editable() {
    let (_temp, root) = common::starter_copy();
    let directory = root.parent().unwrap();
    let sequence = root.join("sequences/layer_test.sequence.donder");
    fs::create_dir(root.join("audio")).unwrap();
    fs::write(root.join("audio/test.wav"), b"test audio").unwrap();
    fs::write(
        &sequence,
        fs::read_to_string(&sequence)
            .unwrap()
            .replace("audio: null", "audio: audio/test.wav"),
    )
    .unwrap();
    let original = load_project(&root).unwrap();
    let destination = directory.join("copy");
    let report = export_editable_project(&original, &destination).unwrap();
    assert_eq!(
        report.copied_assets,
        vec![Utf8PathBuf::from("audio/test.wav")]
    );
    let copied = load_project(&destination).unwrap();
    assert_eq!(copied.project, original.project);
    assert_eq!(
        fs::read(destination.join("audio/test.wav")).unwrap(),
        b"test audio"
    );
    assert!(
        copied
            .source
            .documents
            .keys()
            .all(|id| copied.source.is_project_owned(id))
    );
    fs::write(destination.join("audio/test.wav"), b"edited audio").unwrap();
    assert_eq!(
        fs::read(root.join("audio/test.wav")).unwrap(),
        b"test audio"
    );
    assert!(export_editable_project(&original, &destination).is_err());
}

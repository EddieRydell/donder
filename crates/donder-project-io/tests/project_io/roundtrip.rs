use crate::common;

use camino::{Utf8Path, Utf8PathBuf};
use donder_language::values::DonderDuration;
use donder_project_io::{load_project, save_project};
use std::fs;
use std::time::Duration;

use crate::common::load_project as load_local_project;

#[test]
fn exact_text_writer_preserves_formatting_and_checks_all_disk_preconditions() {
    use donder_project_io::{SourceTextWrite, write_source_texts};
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    fs::write(root.join("one.donder"), "original one").unwrap();
    fs::write(root.join("two.donder"), "external change").unwrap();
    let mut writes = std::collections::BTreeMap::from([
        (
            Utf8PathBuf::from("one.donder"),
            SourceTextWrite {
                text: "-- preserve this comment\neffect Broken {\n".into(),
                expected: Some(b"original one".to_vec()),
            },
        ),
        (
            Utf8PathBuf::from("two.donder"),
            SourceTextWrite {
                text: "two changed".into(),
                expected: Some(b"original two".to_vec()),
            },
        ),
    ]);
    assert!(write_source_texts(root, &writes).is_err());
    assert_eq!(
        fs::read_to_string(root.join("one.donder")).unwrap(),
        "original one"
    );
    writes
        .get_mut(Utf8Path::new("two.donder"))
        .unwrap()
        .expected = Some(b"external change".to_vec());
    write_source_texts(root, &writes).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("one.donder")).unwrap(),
        writes[Utf8Path::new("one.donder")].text
    );
}

#[test]
fn audio_reference_cannot_escape_its_module() {
    let temp = tempfile::tempdir().unwrap();
    let temp_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let project_root = temp_root.join("project");
    fs::create_dir(&project_root).unwrap();
    fs::write(temp_root.join("external.wav"), b"audio").unwrap();
    common::write_imported_sequence_project(
        &project_root,
        &common::MINIMAL_SEQUENCE.replace("audio: none", "audio: <../external.wav>"),
    );
    assert!(load_project(&project_root).is_err());
}

#[test]
fn same_named_definitions_in_different_documents_keep_distinct_identities() {
    let (_temp, root) = common::starter_copy();

    fs::create_dir_all(root.join("identity-a")).unwrap();
    fs::create_dir_all(root.join("identity-b")).unwrap();
    fs::write(
        root.join("identity-a/shared.donder"),
        "effect Shared { sample { #ff0000 } }",
    )
    .unwrap();
    fs::write(
        root.join("identity-b/shared.donder"),
        "effect Shared { sample { #0000ff } }",
    )
    .unwrap();
    let entrypoint = root.join(donder_project_io::PROJECT_ROOT_FILE);
    let project_text = fs::read_to_string(&entrypoint).unwrap();
    fs::write(
        &entrypoint,
        format!(
            "import identity_a from <identity-a/shared.donder>;\nimport identity_b from <identity-b/shared.donder>;\n{project_text}"
        ),
    )
    .unwrap();

    let loaded = load_local_project(&root);
    let identities = loaded
        .project
        .definitions()
        .effects
        .definitions
        .keys()
        .filter(|id| id.0.object() == "Shared")
        .map(|id| id.0.document().to_path_buf())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        identities,
        [
            Utf8PathBuf::from("identity-a/shared.donder"),
            Utf8PathBuf::from("identity-b/shared.donder"),
        ]
        .into_iter()
        .collect()
    );
    save_project(&loaded).unwrap();
    let reloaded = load_local_project(&root);
    assert_eq!(loaded.project, reloaded.project);
}

#[test]
fn typed_sequence_insertion_roundtrips_nested_paths() {
    let (_temp, root) = common::starter_copy();
    let mut session = load_local_project(&root);

    let color = session
        .project
        .reusable_sequences()
        .values()
        .next()
        .unwrap()
        .layers[0]
        .color;
    let index = session.project.root().sequences.len();
    donder_language::ownership::edit::add_sequence(
        &mut session.project,
        DonderDuration(Duration::from_secs(30)),
        60,
        color,
    )
    .unwrap();
    let destination = session
        .source
        .add_data_document(
            "sequences/nested/new.data.donder".into(),
            vec![(
                donder_project_io::SourceObjectKind::Sequence,
                "nested_sequence".into(),
            )],
        )
        .unwrap()
        .remove(0);
    donder_language::ownership::edit::make_reusable(
        &mut session.project,
        &donder_language::ownership::edit::OwnershipSite::ProjectSequence(index),
        destination,
    )
    .unwrap();
    donder_project_io::maintain_ownership_sources(&mut session).unwrap();
    let id = session.project.root().sequences[index].id().clone();
    save_project(&session).unwrap();

    let reloaded = load_local_project(&root);
    assert!(reloaded.project.reusable_sequences().contains_key(&id));
    assert!(
        reloaded
            .project
            .root()
            .sequences
            .iter()
            .any(|source| source.id() == &id)
    );
    assert!(root.join(id.0.document()).is_file());
}

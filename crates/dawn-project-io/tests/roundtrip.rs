mod common;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::values::DawnDuration;
use dawn_project_io::{export_project, load_package, save_project};
use std::fs;
use std::time::Duration;

use common::{load_project_package, write_project_package};

#[test]
fn exact_text_writer_preserves_formatting_and_checks_all_disk_preconditions() {
    use dawn_project_io::{SourceTextWrite, write_source_texts};
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    fs::write(root.join("one.dawn"), "original one").unwrap();
    fs::write(root.join("two.dawn"), "external change").unwrap();
    let mut writes = std::collections::BTreeMap::from([
        (
            Utf8PathBuf::from("one.dawn"),
            SourceTextWrite {
                text: "# preserve this comment\ninvalid: [\n".into(),
                expected: Some(b"original one".to_vec()),
            },
        ),
        (
            Utf8PathBuf::from("two.dawn"),
            SourceTextWrite {
                text: "two changed".into(),
                expected: Some(b"original two".to_vec()),
            },
        ),
    ]);
    assert!(write_source_texts(root, &writes).is_err());
    assert_eq!(
        fs::read_to_string(root.join("one.dawn")).unwrap(),
        "original one"
    );
    writes.get_mut(Utf8Path::new("two.dawn")).unwrap().expected = Some(b"external change".to_vec());
    write_source_texts(root, &writes).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("one.dawn")).unwrap(),
        writes[Utf8Path::new("one.dawn")].text
    );
}

#[test]
fn audio_reference_cannot_escape_its_module() {
    let temp = tempfile::tempdir().unwrap();
    let temp_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let project_root = temp_root.join("project");
    fs::create_dir(&project_root).unwrap();
    fs::write(temp_root.join("external.wav"), b"audio").unwrap();
    fs::write(
        project_root.join("project.dawn"),
        "imports:\n- from:\n    documents:\n    - setup.dawn\n  as: setups\n- from:\n    documents:\n    - sequence.dawn\n  as: sequences\nmain:\n  type: project\n  setup: setups.main\n  sequences: [sequences.main]\n",
    )
    .unwrap();
    fs::write(
        project_root.join("setup.dawn"),
        "imports:\n- from:\n    documents:\n    - display.dawn\n  as: display\n- from:\n    documents:\n    - patch.dawn\n  as: patches\nmain:\n  type: setup\n  layout: display.main\n  patch: patches.main\n  controllers: []\n",
    )
    .unwrap();
    fs::write(
        project_root.join("display.dawn"),
        "pixel:\n  type: fixture\n  pixels:\n  - id: 1\n    diameter: 0.01\nmain:\n  type: layout\n  fixtures:\n  - id: 1\n    name: Pixel\n    type: fixture\n    definition: pixel\n",
    )
    .unwrap();
    fs::write(
        project_root.join("patch.dawn"),
        "main:\n  type: patch\n  routes: []\n",
    )
    .unwrap();
    fs::write(
        project_root.join("sequence.dawn"),
        "main:\n  type: sequence\n  duration: 1s\n  frame_rate: 30\n  audio: ../external.wav\n  mark_collections: []\n  layers: []\n  effects: []\n  composition_graph:\n    nodes:\n    - id: 1\n      position: { x: 0, y: 0 }\n      type: output\n    edges: []\n  automation_clips: []\n",
    )
    .unwrap();
    write_project_package(&project_root);

    assert!(load_package(&project_root).is_err());
}

#[test]
fn same_named_definitions_in_different_documents_keep_distinct_identities() {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    let starter = load_project_package(&workspace_root.join("examples/starter"));
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    export_project(&starter, &root).unwrap();
    write_project_package(&root);

    fs::create_dir_all(root.join("identity-a")).unwrap();
    fs::create_dir_all(root.join("identity-b")).unwrap();
    fs::write(
        root.join("identity-a/shared.effect.dawn"),
        "effect Shared { color sample() { return #ff0000; } }",
    )
    .unwrap();
    fs::write(
        root.join("identity-b/shared.effect.dawn"),
        "effect Shared { color sample() { return #0000ff; } }",
    )
    .unwrap();
    let entrypoint = root.join("project.dawn");
    let project_text = fs::read_to_string(&entrypoint).unwrap();
    fs::write(
        &entrypoint,
        format!(
            "imports:\n- from:\n    documents:\n    - identity-a/shared.effect.dawn\n  as: identity_a\n- from:\n    documents:\n    - identity-b/shared.effect.dawn\n  as: identity_b\n{}",
            project_text.strip_prefix("imports:\n").unwrap()
        ),
    )
    .unwrap();

    let loaded = load_project_package(&root);
    let identities = loaded
        .project
        .definitions
        .effects
        .definitions
        .keys()
        .filter(|id| id.0.object() == "Shared")
        .map(|id| id.0.document().to_path_buf())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        identities,
        [
            Utf8PathBuf::from("identity-a/shared.effect.dawn"),
            Utf8PathBuf::from("identity-b/shared.effect.dawn"),
        ]
        .into_iter()
        .collect()
    );
    save_project(&loaded).unwrap();
    let reloaded = load_project_package(&root);
    assert_eq!(loaded.project, reloaded.project);
}

#[test]
fn typed_sequence_insertion_roundtrips_nested_paths() {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    let starter = load_project_package(&workspace_root.join("examples/starter"));
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    export_project(&starter, &root).unwrap();
    write_project_package(&root);
    let mut session = load_project_package(&root);

    let id = dawn_project_io::insert_sequence(
        &mut session,
        "sequences/nested/new.sequence.dawn".into(),
        "nested_sequence".to_string(),
        DawnDuration(Duration::from_secs(30)),
        60,
    )
    .unwrap();
    save_project(&session).unwrap();

    let reloaded = load_project_package(&root);
    assert!(reloaded.project.sequences.contains_key(&id));
    assert!(reloaded.project.root.sequences.contains(&id));
    assert!(root.join(id.0.document()).is_file());
}

#[test]
fn local_document_import_cannot_escape_module() {
    let workspace_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .unwrap();
    let starter = load_project_package(&workspace_root.join("examples/starter"));
    let temp = tempfile::tempdir().unwrap();
    let temp_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let root = temp_root.join("project");
    export_project(&starter, &root).unwrap();
    write_project_package(&root);
    fs::write(
        temp_root.join("dependency.effect.dawn"),
        "effect Dependency { color sample() { return #ffffff; } }",
    )
    .unwrap();
    let entrypoint = root.join("project.dawn");
    let project_text = fs::read_to_string(&entrypoint).unwrap();
    fs::write(
        &entrypoint,
        format!(
            "imports:\n- from:\n    documents:\n    - ../dependency.effect.dawn\n  as: dependency\n{}",
            project_text.strip_prefix("imports:\n").unwrap()
        ),
    )
    .unwrap();

    assert!(load_package(&root).is_err());
}

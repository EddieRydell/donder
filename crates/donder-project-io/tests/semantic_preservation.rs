mod common;

use camino::{Utf8Path, Utf8PathBuf};
use donder_project_io::{ProjectSession, export_project, save_project, source_document_text};
use std::fs;

fn starter_copy() -> (tempfile::TempDir, Utf8PathBuf, ProjectSession) {
    let starter_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let starter = common::load_project(&starter_root);
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().to_path_buf()).unwrap();
    export_project(&starter, &root).unwrap();
    common::write_workspace_metadata(&root);
    let session = common::load_project(&root);
    (temporary, root, session)
}

#[test]
fn typed_save_preserves_semantics_imports_ownership_assets_and_dsl_not_yaml_presentation() {
    let (_temporary, root, session) = starter_copy();
    let sequence_id = session
        .project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
        .find(|id| !session.project.reusable_sequences()[*id].effects.is_empty())
        .unwrap()
        .clone();
    let path = root.join(sequence_id.0.document());
    let original = fs::read_to_string(&path).unwrap();
    let presentation = format!(
        "# disposable presentation\n{}",
        original.replace("type: sequence", "type: 'sequence'")
    );
    assert_ne!(presentation, original);
    fs::write(&path, presentation).unwrap();
    let mut edited = common::load_project(&root);
    assert_eq!(session.project, edited.project);
    let mut sequence = edited.project.sequence(&sequence_id).unwrap().clone();
    sequence.layers[0].name.push_str(" edited");
    // List order is semantic even though YAML mapping key order is not.
    sequence.effects.reverse();
    edited
        .project
        .replace_sequence(&sequence_id, sequence)
        .unwrap();

    save_project(&edited).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert!(!saved.contains("# disposable presentation"));
    let reloaded = common::load_project(&root);
    assert_eq!(edited.project, reloaded.project);
    assert_eq!(edited.source.entrypoint, reloaded.source.entrypoint);
    assert_eq!(
        edited.source.referenced_assets,
        reloaded.source.referenced_assets
    );
    assert_eq!(
        edited.source.documents.len(),
        reloaded.source.documents.len()
    );
    for (id, before) in &edited.source.documents {
        let after = &reloaded.source.documents[id];
        assert_eq!(before.imports(), after.imports(), "{id:?}");
        assert_eq!(before.objects(), after.objects(), "{id:?}");
        assert_eq!(
            edited.source.is_project_owned(id),
            reloaded.source.is_project_owned(id)
        );
        let before_text = source_document_text(&edited, id).unwrap().unwrap();
        assert_eq!(
            before_text,
            fs::read_to_string(root.join(id.path())).unwrap()
        );
        assert_eq!(
            before_text,
            source_document_text(&reloaded, id).unwrap().unwrap()
        );
    }
    save_project(&reloaded).unwrap();
    assert_eq!(saved, fs::read_to_string(&path).unwrap());
}

#[test]
fn missing_import_is_an_error_not_a_flattened_or_guessed_reference() {
    let (_temporary, _root, mut session) = starter_copy();
    let id = session
        .project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
        .find(|id| !session.project.reusable_sequences()[*id].effects.is_empty())
        .unwrap()
        .0
        .document_id()
        .clone();
    let original = &session.source.documents[&id];
    let without_imports = donder_project_io::SourceDocument::new(
        Vec::new(),
        original.objects().to_vec(),
        original.kind().clone(),
    )
    .unwrap();
    session.source.documents.insert(id.clone(), without_imports);
    assert!(source_document_text(&session, &id).is_err());
}

#[test]
fn unknown_nested_parameter_metadata_is_rejected_without_changing_source() {
    let (_temporary, root, session) = starter_copy();
    let id = session
        .project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
        .find(|id| !session.project.reusable_sequences()[*id].effects.is_empty())
        .unwrap();
    let path = root.join(id.0.document());
    let original = fs::read_to_string(&path).unwrap();
    let with_metadata = original.replacen(
        "type: integer",
        "type: integer\n        unrecognized_metadata: 123",
        1,
    );
    assert_ne!(original, with_metadata);
    fs::write(&path, &with_metadata).unwrap();
    let report = donder_project_io::check_project(&root);
    assert!(report.session.is_none());
    assert!(
        report.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("unrecognized_metadata")
            && diagnostic.range.is_some()),
        "{:?}",
        report.diagnostics
    );
    assert_eq!(fs::read_to_string(path).unwrap(), with_metadata);
}

#[test]
fn removing_only_typed_object_rejects_save_before_any_write() {
    let (_temporary, root, mut session) = starter_copy();
    let id = session.project.root().sequences[0].id().clone();
    let before = donder_project_io::project_source_texts(&root).unwrap();
    let mut project_root = session.project.root().clone();
    project_root.sequences.retain(|source| source.id() != &id);
    session
        .project
        .apply_edits([
            donder_language::model::ProjectEdit::ReplaceRoot(project_root),
            donder_language::model::ProjectEdit::RemoveSequence(id.clone()),
        ])
        .unwrap();
    // Typed edits preserve project validity; source inventory must still agree at save time.
    assert!(save_project(&session).is_err());
    assert!(source_document_text(&session, id.0.document_id()).is_err());
    assert_eq!(
        before,
        donder_project_io::project_source_texts(&root).unwrap()
    );
}

#[test]
fn typed_objects_without_source_inventory_cannot_be_silently_omitted() {
    use donder_language::{
        identity::{DocumentId, SourceIdentity},
        sequence::SequenceId,
    };
    let (_temporary, root, session) = starter_copy();
    let before = donder_project_io::project_source_texts(&root).unwrap();
    let original = &session.project.reusable_sequences()[session.project.root().sequences[0].id()];
    for document in [
        original.id.0.document_id().clone(),
        DocumentId::new(
            session.source.project_module_id(),
            "sequences/unregistered.sequence.donder".into(),
        ),
    ] {
        let mut candidate = session.clone();
        let mut added = original.clone();
        added.id =
            SequenceId(SourceIdentity::from_document(document, "unregistered".into()).into());
        candidate
            .project
            .apply_edits([donder_language::model::ProjectEdit::InsertSequence(added)])
            .unwrap();
        assert!(save_project(&candidate).is_err());
        assert_eq!(
            before,
            donder_project_io::project_source_texts(&root).unwrap()
        );
    }
}

#[test]
fn unused_objects_in_loaded_documents_are_typed_and_roundtrip() {
    let (_temporary, root, _) = starter_copy();
    let path = root.join("sequences/empty.sequence.donder");
    let original = fs::read_to_string(&path).unwrap();
    let mut document: yaml_serde::Value = yaml_serde::from_str(&original).unwrap();
    let unused = document["empty"].clone();
    document
        .as_mapping_mut()
        .unwrap()
        .insert(yaml_serde::Value::String("unused".into()), unused);
    fs::write(&path, yaml_serde::to_string(&document).unwrap()).unwrap();
    let mut session = common::load_project(&root);
    let id = session
        .project
        .reusable_sequences()
        .keys()
        .find(|id| id.0.root_source().object() == "unused")
        .unwrap()
        .clone();
    assert!(
        !session
            .project
            .root()
            .sequences
            .iter()
            .any(|source| source.id() == &id)
    );
    let mut sequence = session.project.sequence(&id).unwrap().clone();
    sequence.frame_rate = 60;
    session.project.replace_sequence(&id, sequence).unwrap();
    save_project(&session).unwrap();
    assert_eq!(session.project, common::load_project(&root).project);
}

#[test]
fn parameter_variants_and_array_shorthands_reject_extra_keys() {
    let (_temporary, root, _) = starter_copy();
    let path = Utf8PathBuf::from("sequences/layer_test.sequence.donder");
    let original = donder_project_io::project_source_texts(&root).unwrap();
    let mut payloads: Vec<_> = [
        ("integer", "value: 6"),
        ("float", "value: 0.5"),
        ("bool", "value: true"),
        ("color", "value: '#ffffff'"),
        ("enum", "value: test"),
        ("marks", "key: marks"),
        ("curve", "curve: curves.ease_down"),
        ("gradient", "gradient: gradients.ember_core_gradient"),
        ("array", "values: []"),
    ]
    .iter()
    .map(|(kind, body)| format!("type: {kind}\n        {body}\n        unexpected: 1"))
    .collect();
    payloads.extend([
        "type: array\n        values:\n        - type: float\n          value: 0.5\n          unexpected: 1".into(),
        "type: array\n        values:\n        - curve: curves.ease_down\n          unexpected: 1".into(),
        "type: array\n        values:\n        - gradient: gradients.ember_core_gradient\n          unexpected: 1".into(),
    ]);
    for payload in payloads {
        let mut overrides = original.clone();
        // Replace the whole integer payload so array errors are actually reached.
        let changed = original[&path].replacen("type: integer\n        value: 6", &payload, 1);
        assert_ne!(original[&path], changed);
        overrides.insert(path.clone(), changed);
        let report = donder_project_io::check_project_with_overrides(&root, &overrides);
        assert!(report.session.is_none(), "{payload}");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("unexpected")
                    && diagnostic.range.is_some()),
            "{payload}: {:?}",
            report.diagnostics
        );
    }
}

use crate::common;

use camino::Utf8PathBuf;
use donder_project_io::{ProjectSession, save_project, source_document_text};
use std::fs;

fn starter_copy() -> (tempfile::TempDir, Utf8PathBuf, ProjectSession) {
    let (temporary, root) = common::starter_copy();
    let session = common::load_project(&root);
    (temporary, root, session)
}

#[test]
fn saving_a_loaded_project_rewrites_no_byte() {
    let (_temporary, root, session) = starter_copy();
    let before = donder_project_io::project_source_texts(&root).unwrap();
    save_project(&session).unwrap();
    assert_eq!(
        before,
        donder_project_io::project_source_texts(&root).unwrap()
    );
    for id in session.source.documents.keys() {
        assert_eq!(
            source_document_text(&session, id).unwrap().unwrap(),
            before[id.path()],
            "{id:?}"
        );
    }
}

#[test]
fn saving_normalizes_whitespace_and_keeps_every_token_and_list_order() {
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
    // Whitespace is the only freedom the text has.
    let spaced = original.replace(": ", " :   ").replace(",\n", " ,\n\n");
    assert_ne!(spaced, original);
    fs::write(&path, spaced).unwrap();
    let mut edited = common::load_project(&root);
    assert_eq!(session.project, edited.project);
    let mut sequence = edited.project.sequence(&sequence_id).unwrap().clone();
    sequence.layers[0].name =
        donder_language::object_name(&format!("{}_edited", sequence.layers[0].name.as_str()));
    sequence.effects.reverse();
    let names = sequence
        .effects
        .iter()
        .map(|effect| effect.name.clone())
        .collect::<Vec<_>>();
    edited
        .project
        .replace_sequence(&sequence_id, sequence)
        .unwrap();

    save_project(&edited).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("_edited"));
    let reloaded = common::load_project(&root);
    // Clip identities follow document order, so compare the order by name.
    assert_eq!(
        reloaded
            .project
            .sequence(&sequence_id)
            .unwrap()
            .effects
            .iter()
            .map(|effect| effect.name.clone())
            .collect::<Vec<_>>(),
        names
    );
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
        assert_eq!(
            source_document_text(&reloaded, id).unwrap().unwrap(),
            fs::read_to_string(root.join(id.path())).unwrap()
        );
    }
    save_project(&reloaded).unwrap();
    assert_eq!(saved, fs::read_to_string(&path).unwrap());
}

#[test]
fn missing_import_is_an_error_not_a_flattened_or_guessed_reference() {
    let mut session = common::load_project(&common::starter_root());
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
fn removing_only_typed_object_rejects_save_before_any_write() {
    let (_temporary, root, mut session) = starter_copy();
    let id = session.project.root().sequences[0].id().clone();
    let before = donder_project_io::project_source_texts(&root).unwrap();
    let mut project_root = session.project.root().clone();
    project_root.sequences.retain(|source| source.id() != &id);
    session
        .project
        .apply_edits([
            donder_model::ProjectEdit::ReplaceRoot(project_root),
            donder_model::ProjectEdit::RemoveSequence(id.clone()),
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
    use donder_model::{DocumentId, SequenceId, SourceIdentity};
    let (_temporary, root, session) = starter_copy();
    let before = donder_project_io::project_source_texts(&root).unwrap();
    let original = &session.project.reusable_sequences()[session.project.root().sequences[0].id()];
    for document in [
        original.id.0.document_id().clone(),
        DocumentId::new(
            session.source.project_module_id(),
            "sequences/unregistered.data.donder".into(),
        ),
    ] {
        let mut candidate = session.clone();
        let mut added = original.clone();
        added.id =
            SequenceId(SourceIdentity::from_document(document, "unregistered".into()).into());
        candidate
            .project
            .apply_edits([donder_model::ProjectEdit::InsertSequence(added)])
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
    let path = root.join("sequences/empty.data.donder");
    let original = fs::read_to_string(&path).unwrap();
    let declaration = &original[original.find("Sequence empty {").unwrap()..];
    fs::write(
        &path,
        format!(
            "{original}\n{}",
            declaration.replacen("Sequence empty", "Sequence unused", 1)
        ),
    )
    .unwrap();
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

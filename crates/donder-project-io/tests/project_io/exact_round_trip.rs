use crate::common;

use camino::Utf8PathBuf;
use donder_project_io::{ProjectSession, save_project, source_document_text};
use fs_err as fs;

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
    for id in session.source().documents().keys() {
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
        .project()
        .reusable::<donder_model::Sequence>()
        .keys()
        .find(|id| {
            !session.project().reusable::<donder_model::Sequence>()[*id]
                .composition
                .clips
                .is_empty()
        })
        .copied()
        .unwrap();
    let path = root.join(session.address_of(sequence_id).document());
    let original = fs::read_to_string(&path).unwrap();
    // Whitespace is the only freedom the text has.
    let spaced = original.replace(": ", " :   ").replace(",\n", " ,\n\n");
    assert_ne!(spaced, original);
    fs::write(&path, spaced).unwrap();
    let mut edited = common::load_project(&root);
    assert_eq!(session.project(), edited.project());
    let mut sequence = edited
        .project()
        .get::<donder_model::Sequence>(sequence_id)
        .unwrap()
        .clone();
    sequence.composition.layers[0].name = donder_language::object_name(&format!(
        "{}_edited",
        sequence.composition.layers[0].name.as_str()
    ));
    sequence.composition.clips.reverse();
    let names = sequence
        .composition
        .clips
        .iter()
        .map(|effect| effect.name.clone())
        .collect::<Vec<_>>();
    edited.put((sequence_id, sequence)).unwrap();

    save_project(&edited).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("_edited"));
    let reloaded = common::load_project(&root);
    // Clip identities follow document order, so compare the order by name.
    assert_eq!(
        reloaded
            .project()
            .get::<donder_model::Sequence>(sequence_id)
            .unwrap()
            .composition
            .clips
            .iter()
            .map(|effect| effect.name.clone())
            .collect::<Vec<_>>(),
        names
    );
    assert_eq!(edited.source().entrypoint(), reloaded.source().entrypoint());
    assert_eq!(
        edited.source().referenced_assets(),
        reloaded.source().referenced_assets()
    );
    assert_eq!(
        edited.source().documents().len(),
        reloaded.source().documents().len()
    );
    for (id, before) in edited.source().documents() {
        let after = &reloaded.source().documents()[id];
        assert_eq!(before.imports(), after.imports(), "{id:?}");
        assert_eq!(before.objects(), after.objects(), "{id:?}");
        assert_eq!(
            source_document_text(&reloaded, id).unwrap().unwrap(),
            fs::read_to_string(root.join(id.path())).unwrap()
        );
    }
    save_project(&reloaded).unwrap();
    assert_eq!(saved, fs::read_to_string(&path).unwrap());
}

#[test]
fn removing_only_typed_object_rejects_save_before_any_write() {
    let (_temporary, root, session) = starter_copy();
    let id = *session
        .project()
        .reusable::<donder_model::Sequence>()
        .keys()
        .next()
        .unwrap();
    let document = session.address_of(id).document_id().clone();
    let before = donder_project_io::project_source_texts(&root).unwrap();
    session
        .clone()
        .apply_edits([donder_model::ProjectEdit::Remove(id.into())])
        .unwrap_err();
    // An edit that removes a declared object without its declaration is refused.
    assert!(source_document_text(&session, &document).unwrap().is_some());
    assert_eq!(
        before,
        donder_project_io::project_source_texts(&root).unwrap()
    );
}

#[test]
fn typed_objects_without_source_inventory_cannot_be_silently_omitted() {
    use donder_model::SequenceId;
    let (_temporary, root, session) = starter_copy();
    let before = donder_project_io::project_source_texts(&root).unwrap();
    let original = session
        .project()
        .reusable::<donder_model::Sequence>()
        .values()
        .next()
        .unwrap();
    let mut candidate = session.clone();
    let added = original.clone();
    let id = SequenceId(candidate.new_id());
    candidate
        .apply_edits([donder_model::ProjectEdit::Put((id, added).into())])
        .unwrap_err();
    assert_eq!(
        before,
        donder_project_io::project_source_texts(&root).unwrap()
    );
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
        .project()
        .reusable::<donder_model::Sequence>()
        .keys()
        .copied()
        .find(|id| session.declaration((*id).into()).unwrap().1.as_str() == "unused")
        .unwrap();
    let mut sequence = session
        .project()
        .get::<donder_model::Sequence>(id)
        .unwrap()
        .clone();
    sequence.frame_rate = 60;
    session.put((id, sequence)).unwrap();
    save_project(&session).unwrap();
    assert_eq!(session.project(), common::load_project(&root).project());
}

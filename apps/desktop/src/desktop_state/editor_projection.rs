use camino::Utf8Path;
use donder_editor::dto::{DocumentDescriptor, DocumentObjectDescriptor};
use donder_project_io::{DocumentTextCache, ProjectSession, source_document_texts};
use std::collections::{BTreeMap, BTreeSet};

/// The canonical text of each project document at `paths`. `cache` keeps
/// printed clips between edits, so unchanged clips are not printed again.
pub(crate) fn generated_source_texts(
    session: &ProjectSession,
    paths: &BTreeSet<String>,
    cache: &mut DocumentTextCache,
) -> Result<BTreeMap<String, String>, String> {
    let documents = paths
        .iter()
        .filter_map(|path| {
            session
                .source
                .document_for_workspace_path(Utf8Path::new(path))
                .map(|id| (path, id))
        })
        .collect::<Vec<_>>();
    let ids = documents
        .iter()
        .map(|(_, id)| id.clone())
        .collect::<Vec<_>>();
    let texts = source_document_texts(session, &ids, cache).map_err(|error| error.to_string())?;
    Ok(documents
        .into_iter()
        .zip(texts)
        .filter_map(|((path, _), text)| text.map(|text| (path.clone(), text)))
        .collect())
}

pub(crate) fn descriptor_for_path(
    session: &ProjectSession,
    path: &Utf8Path,
) -> Option<DocumentDescriptor> {
    let document = donder_editor::document_for_editor_path(session, path)
        .and_then(|id| session.source.documents.get(&id));
    let descriptor = |object: &donder_project_io::DeclaredObject| DocumentObjectDescriptor {
        key: object.name().as_str().to_string(),
        kind: object.kind().into(),
    };
    let objects = document
        .map(|document| document.objects())
        .unwrap_or_default();
    let mut gui_objects: Vec<_> = objects
        .iter()
        .filter_map(|object| {
            donder_editor::gui_editor_order(object.kind()).map(|order| (order, object))
        })
        .collect();
    gui_objects.sort_by_key(|(order, _)| *order);
    Some(DocumentDescriptor {
        path: path.to_string(),
        objects: objects.iter().map(descriptor).collect(),
        gui_objects: gui_objects
            .into_iter()
            .map(|(_, object)| descriptor(object))
            .collect(),
    })
}

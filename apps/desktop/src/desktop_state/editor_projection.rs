use camino::Utf8Path;
use donder_project_io::{DocumentTextCache, ProjectSession, source_document_texts};
use donder_sequence_api::{
    DocumentDefaultObjectKey, DocumentDescriptor, DocumentObjectDescriptor, DocumentViewId,
    ObjectKind,
};
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
    let objects: Vec<_> = document
        .into_iter()
        .flat_map(|document| document.objects())
        .map(|object| DocumentObjectDescriptor {
            key: object.id().to_string(),
            kind: ObjectKind::from(object.kind()),
        })
        .collect();
    let mut default_object_keys: Vec<_> = objects
        .iter()
        .filter_map(|object| {
            object
                .kind
                .document_view()
                .map(|view| DocumentDefaultObjectKey {
                    view,
                    object_key: object.key.clone(),
                })
        })
        .collect();
    default_object_keys.sort_by_key(|object| match object.view {
        DocumentViewId::Project => 0,
        DocumentViewId::Sequence => 1,
        DocumentViewId::Setup => 2,
        DocumentViewId::Layout => 3,
        DocumentViewId::Fixture => 4,
        DocumentViewId::Patch => 5,
        DocumentViewId::Controller => 6,
        DocumentViewId::Curve => 7,
        DocumentViewId::Gradient => 8,
        DocumentViewId::Text => 9,
    });
    let mut available_views = vec![DocumentViewId::Text];
    for object in &default_object_keys {
        if !available_views.contains(&object.view) {
            available_views.push(object.view.clone());
        }
    }
    Some(DocumentDescriptor {
        path: path.to_string(),
        objects,
        available_views,
        default_object_keys,
    })
}

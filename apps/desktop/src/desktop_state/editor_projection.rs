use crate::dto::{
    DocumentDefaultObjectKey, DocumentDescriptor, DocumentObjectDescriptor, DocumentViewId,
    ObjectKind,
};
use camino::Utf8Path;
use donder_project_io::{ProjectSession, source_document_text};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn generated_source_texts(
    session: &ProjectSession,
    paths: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut texts = BTreeMap::new();
    for path in paths {
        let Some(id) = session
            .source
            .document_for_workspace_path(Utf8Path::new(path))
        else {
            continue;
        };
        if let Some(text) = source_document_text(session, &id).map_err(|error| error.to_string())? {
            texts.insert(path.clone(), text);
        }
    }
    Ok(texts)
}

pub(crate) fn descriptor_for_path(
    session: &ProjectSession,
    path: &Utf8Path,
) -> Option<DocumentDescriptor> {
    let document = crate::source_documents::document_for_editor_path(session, path)
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

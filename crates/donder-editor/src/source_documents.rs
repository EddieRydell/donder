use camino::{Utf8Path, Utf8PathBuf};
use donder_model::DocumentId;
use donder_project_io::ProjectSession;

/// All source tabs use project-relative paths.
pub fn editor_path(session: &ProjectSession, document: &DocumentId) -> Option<Utf8PathBuf> {
    session.source.workspace_path_for_document(document)
}

pub fn document_for_editor_path(session: &ProjectSession, path: &Utf8Path) -> Option<DocumentId> {
    if path.is_absolute() {
        session
            .source
            .documents
            .keys()
            .find(|document| session.source.absolute_path(document).as_deref() == Some(path))
            .cloned()
    } else {
        session.source.document_for_workspace_path(path)
    }
}

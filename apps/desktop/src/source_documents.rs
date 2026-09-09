use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::identity::DocumentId;
use dawn_project_io::ProjectSession;

/// Project files use workspace-relative tab paths. External dependency sources
/// use their absolute paths, and can only be resolved from the loaded graph.
pub(crate) fn editor_path(session: &ProjectSession, document: &DocumentId) -> Option<Utf8PathBuf> {
    match session.source.workspace_path_for_document(document) {
        Some(path) => Some(path),
        None => session.source.absolute_path(document),
    }
}

pub(crate) fn document_for_editor_path(
    session: &ProjectSession,
    path: &Utf8Path,
) -> Option<DocumentId> {
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

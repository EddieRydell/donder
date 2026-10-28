use camino::Utf8Path;
use donder_project_io::DocumentId;
use donder_project_io::ProjectSession;

pub fn document_for_editor_path(session: &ProjectSession, path: &Utf8Path) -> Option<DocumentId> {
    if path.is_absolute() {
        session
            .source
            .documents
            .keys()
            .find(|document| session.source.absolute_path(document) == path)
            .cloned()
    } else {
        session.source.document_for_workspace_path(path)
    }
}

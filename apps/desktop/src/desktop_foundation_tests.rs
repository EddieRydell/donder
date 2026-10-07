#[cfg(test)]
pub(crate) mod tests {
    use std::fs;

    use camino::{Utf8Path, Utf8PathBuf};
    use donder_project_io::load_project;

    use crate::dto::{DocumentViewId, GuiDocument, GuiDocumentRequest, WorkspacePathChangeRequest};

    fn starter() -> donder_project_io::ProjectSession {
        let workspace = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Utf8Path::parent)
            .unwrap();
        load_project(&workspace.join("examples/starter")).unwrap()
    }

    /// Saving `session` and reopening `root` gives back the same documents.
    /// Loading assigns fresh session identities, so the printed text, which
    /// holds every authored value, is what must match.
    pub(crate) fn assert_reloads(root: &Utf8Path, session: &donder_project_io::ProjectSession) {
        let loaded = load_project(root).unwrap();
        let documents = |session: &donder_project_io::ProjectSession| {
            session
                .source
                .documents
                .keys()
                .map(|id| {
                    (
                        id.path().to_string(),
                        donder_project_io::source_document_text(session, id)
                            .unwrap()
                            .unwrap(),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        assert_eq!(documents(&loaded), documents(session));
    }

    pub(crate) fn starter_copy() -> (tempfile::TempDir, Utf8PathBuf) {
        let workspace = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Utf8Path::parent)
            .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(temporary.path())
            .unwrap()
            .join("starter");
        copy_tree(&workspace.join("examples/starter"), &root);
        (temporary, root)
    }

    fn copy_tree(source: &Utf8Path, destination: &Utf8Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let source_path = Utf8PathBuf::from_path_buf(entry.path()).unwrap();
            let destination_path = destination.join(entry.file_name().into_string().unwrap());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&source_path, &destination_path);
            } else {
                fs::copy(source_path, destination_path).unwrap();
            }
        }
    }

    #[test]
    fn setup_projection_identifies_its_composed_objects() {
        let session = starter();
        let request = GuiDocumentRequest {
            owned_path: Vec::new(),
            project_revision: 0,
            path: "setups/main.data.donder".to_string(),
            view: DocumentViewId::Setup,
            object_key: Some("main".to_string()),
        };
        let GuiDocument::Setup { document } =
            crate::gui::project_gui_document(Some(&session), &request)
        else {
            panic!("setup projection was blocked");
        };
        assert_eq!(document.layout_ref.object_key, "outputs_layout");
        assert_eq!(document.patch_ref.object_key, "outputs");
        assert!(!document.controllers.is_empty());

        for (reference, view) in [
            (&document.layout_ref, DocumentViewId::Layout),
            (&document.patch_ref, DocumentViewId::Patch),
            (
                &document.controllers[0].source_ref,
                DocumentViewId::Controller,
            ),
        ] {
            let projection = crate::gui::project_gui_document(
                Some(&session),
                &GuiDocumentRequest {
                    owned_path: Vec::new(),
                    project_revision: 0,
                    path: reference.path.clone(),
                    view: view.clone(),
                    object_key: Some(reference.object_key.clone()),
                },
            );
            assert!(matches!(
                (view, projection),
                (DocumentViewId::Layout, GuiDocument::Layout { .. })
                    | (DocumentViewId::Patch, GuiDocument::Patch { .. })
                    | (DocumentViewId::Controller, GuiDocument::Controller { .. })
            ));
        }
    }

    #[test]
    fn path_change_rejects_stale_revision() {
        let (_temporary, root) = starter_copy();
        let state = crate::desktop_state::DesktopState::new(|_| {});
        let snapshot = state.open_project_path(root.as_str());
        let error = state
            .plan_workspace_path_change(WorkspacePathChangeRequest {
                source: "effects/impact-burst.donder".to_string(),
                destination: "effects/impact.donder".to_string(),
                project_revision: snapshot.project_revision.saturating_sub(1),
            })
            .unwrap_err();
        assert!(error.contains("project changed"));
    }

    #[test]
    fn structural_path_change_rejects_dirty_open_text() {
        let (_temporary, root) = starter_copy();
        let state = crate::desktop_state::DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let mut settings = state.snapshot().settings;
        settings.autosave_project_edits = false;
        state.update_app_settings(settings);
        let snapshot = state.open_file_path("sequences/layer_test.data.donder");
        let buffer = snapshot.active_buffer.unwrap();
        state
            .update_document(crate::dto::DocumentUpdate {
                project_epoch: snapshot.project_epoch,
                path: buffer.path,
                expected_document_revision: buffer.document_revision,
                text: buffer.text.replace("frame_rate: 144", "frame_rate: 90"),
            })
            .unwrap();
        let revision = state.snapshot().project_revision;
        let error = state
            .apply_workspace_path_change(WorkspacePathChangeRequest {
                source: "effects/impact-burst.donder".to_string(),
                destination: "effects/impact.donder".to_string(),
                project_revision: revision,
            })
            .unwrap_err();
        assert!(error.contains("saved"));
        assert!(root.join("effects/impact-burst.donder").is_file());
    }
}

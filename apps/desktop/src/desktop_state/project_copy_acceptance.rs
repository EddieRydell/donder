use super::DesktopState;
use crate::dto::*;
use crate::project::{new_project_files, write_new_project_files};
use camino::Utf8PathBuf;

#[test]
fn editable_project_copy_honors_save_discard_cancel_and_failed_destinations() {
    for decision in [
        TransitionDecision::SaveAll,
        TransitionDecision::Discard,
        TransitionDecision::Cancel,
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let parent = Utf8PathBuf::from_path_buf(temporary.path().to_path_buf()).unwrap();
        let original_root = parent.join("original");
        write_new_project_files(
            &original_root,
            &new_project_files("Copy transition").unwrap(),
        )
        .unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(original_root.as_str());
        let mut settings = state.snapshot().settings;
        settings.autosave_project_edits = false;
        state.update_app_settings(settings);
        let original = state.project_session().unwrap();
        let setup = original.project.root.setup.clone();
        state.open_file_path(setup.0.document().as_str());
        let result = super::authoring_acceptance::edit_layout(
            &state,
            LayoutGuiEdit::SetFixtures {
                fixtures: vec![GuiLayoutFixture {
                    id: 1,
                    name: "Unsaved group".into(),
                    kind: GuiLayoutFixtureKind::Group { children: vec![] },
                }],
            },
        );
        assert!(matches!(result.document, GuiDocument::Layout { .. }));
        let draft = state.project_session().unwrap();
        let snapshot = state.snapshot();
        let request = |decision, name: &str| TransitionRequest {
            project_epoch: snapshot.project_epoch,
            project_revision: snapshot.project_revision,
            transition: WorkspaceTransition::CopyProject {
                parent_path: parent.to_string(),
                directory_name: name.into(),
            },
            decision,
        };
        assert!(matches!(
            state.request_transition(request(None, "copy")).unwrap(),
            TransitionResult::NeedsDecision { .. }
        ));
        assert!(!parent.join("copy").exists());
        std::fs::create_dir(parent.join("occupied")).unwrap();
        assert!(
            state
                .request_transition(request(Some(TransitionDecision::Discard), "occupied"))
                .is_err()
        );
        assert!(std::sync::Arc::ptr_eq(
            &draft,
            &state.project_session().unwrap()
        ));
        assert_eq!(state.snapshot().project_epoch, snapshot.project_epoch);
        let result = state
            .request_transition(request(Some(decision.clone()), "copy"))
            .unwrap();
        if matches!(decision, TransitionDecision::Cancel) {
            assert!(matches!(result, TransitionResult::Cancelled { .. }));
            assert!(!parent.join("copy").exists());
            assert_eq!(*state.project_session().unwrap(), *draft);
            continue;
        }
        let TransitionResult::Applied {
            snapshot: opened, ..
        } = result
        else {
            panic!("copy was not opened")
        };
        assert_ne!(opened.project_epoch, snapshot.project_epoch);
        assert_eq!(
            opened.project_root.as_deref(),
            Some(parent.join("copy").as_str())
        );
        assert!(matches!(
            opened.settings.editor_view_mode,
            EditorViewMode::Gui
        ));
        let expected = if matches!(decision, TransitionDecision::SaveAll) {
            &draft.project
        } else {
            &original.project
        };
        assert_eq!(&state.project_session().unwrap().project, expected);
        assert_eq!(
            &dawn_project_io::load_package(&original_root)
                .unwrap()
                .session
                .project,
            expected
        );
        assert_eq!(
            &dawn_project_io::load_package(&parent.join("copy"))
                .unwrap()
                .session
                .project,
            expected
        );
    }
}

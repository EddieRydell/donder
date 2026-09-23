//! Background save/analysis/render schedulers and GUI history storage.

mod filesystem_watcher;
mod history;
mod scheduler;
pub(crate) use filesystem_watcher::watch_project;

pub(crate) use history::*;
pub(crate) use scheduler::*;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use super::*;
    use camino::Utf8Path;
    use donder_project_io::{ProjectSession, load_package};

    fn entry(session: &ProjectSession, status_path: &str) -> GuiHistoryEntry {
        GuiHistoryEntry {
            before: Arc::new(session.clone()),
            after: Arc::new(session.clone()),
            affected_paths: BTreeSet::new(),
            status_path: status_path.to_string(),
        }
    }

    #[test]
    fn new_undo_entries_clear_redo_and_respect_the_limit() {
        let workspace = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Utf8Path::parent)
            .unwrap();
        let session = load_package(&workspace.join("examples/starter"))
            .unwrap()
            .session;
        let mut history = GuiHistory::new(2);
        history.push_redo(entry(&session, "redo"));
        history.push_undo(entry(&session, "one"));
        history.push_undo(entry(&session, "two"));
        history.push_undo(entry(&session, "three"));

        assert!(history.pop_redo().is_none());
        assert_eq!(history.pop_undo().unwrap().status_path, "three");
        assert_eq!(history.pop_undo().unwrap().status_path, "two");
        assert!(history.pop_undo().is_none());
    }
}

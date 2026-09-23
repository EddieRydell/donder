use std::collections::BTreeSet;
use std::sync::Arc;

use donder_project_io::ProjectSession;

#[derive(Clone)]
pub(crate) struct GuiHistoryEntry {
    pub(crate) before: Arc<ProjectSession>,
    pub(crate) after: Arc<ProjectSession>,
    pub(crate) affected_paths: BTreeSet<String>,
    pub(crate) status_path: String,
}

pub(crate) struct GuiHistory {
    undo: Vec<GuiHistoryEntry>,
    redo: Vec<GuiHistoryEntry>,
    limit: usize,
}

impl GuiHistory {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit,
        }
    }

    pub(crate) fn push_undo(&mut self, entry: GuiHistoryEntry) {
        self.undo.push(entry);
        self.trim_undo();
        self.redo.clear();
    }

    pub(crate) fn push_undo_from_redo(&mut self, entry: GuiHistoryEntry) {
        self.undo.push(entry);
        self.trim_undo();
    }

    fn trim_undo(&mut self) {
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    pub(crate) fn peek_undo(&self) -> Option<GuiHistoryEntry> {
        self.undo.last().cloned()
    }

    pub(crate) fn pop_undo(&mut self) -> Option<GuiHistoryEntry> {
        self.undo.pop()
    }

    pub(crate) fn push_redo(&mut self, entry: GuiHistoryEntry) {
        self.redo.push(entry);
    }

    pub(crate) fn peek_redo(&self) -> Option<GuiHistoryEntry> {
        self.redo.last().cloned()
    }

    pub(crate) fn pop_redo(&mut self) -> Option<GuiHistoryEntry> {
        self.redo.pop()
    }

    pub(crate) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

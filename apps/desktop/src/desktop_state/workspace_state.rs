use super::LoadedProject;
use camino::Utf8PathBuf;
use donder_editor::dto::{
    AppSnapshot, DocumentSaveState, DocumentSaveStatus, EditorBuffer, EditorTab, ProjectHealth,
};
use std::collections::BTreeMap;

pub(super) struct WorkingDocument {
    pub buffer: EditorBuffer,
    pub observed: Option<Vec<u8>>,
}

/// The rendered sequence and the setup it renders on, by address: a text
/// reload assigns new ids, so each session resolves them again.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RenderTarget {
    pub sequence: donder_project_io::ObjectAddress,
}

impl RenderTarget {
    pub(super) fn new(
        session: &donder_project_io::ProjectSession,
        sequence: donder_model::SequenceId,
    ) -> Option<Self> {
        Some(Self {
            sequence: session.address_of(sequence.into())?,
        })
    }

    pub(super) fn resolve(
        &self,
        session: &donder_project_io::ProjectSession,
    ) -> Option<donder_model::SequenceId> {
        session.resolve_address(&self.sequence)?.try_into().ok()
    }
}

pub(super) struct WorkspaceState {
    /// The snapshot's view state. Its tabs, active buffer and pending saves
    /// come from `documents` when a snapshot is taken.
    pub view: AppSnapshot,
    pub project: LoadedProject,
    pub documents: BTreeMap<Utf8PathBuf, WorkingDocument>,
    pub tabs: Vec<Utf8PathBuf>,
    pub typed_revision: Option<u32>,
    pub close_authorization: Option<(u32, u32)>,
    pub render_target: Option<RenderTarget>,
    /// Persistence key of the sequence the transport has loaded.
    pub transport_view_key: Option<String>,
}

impl WorkspaceState {
    pub(super) fn new(snapshot: AppSnapshot) -> Self {
        Self {
            view: snapshot,
            project: LoadedProject::Closed,
            documents: BTreeMap::new(),
            tabs: Vec::new(),
            typed_revision: None,
            close_authorization: None,
            render_target: None,
            transport_view_key: None,
        }
    }

    /// The sources changed: the typed project is stale until the next check.
    pub(super) fn mark_checking(&mut self) {
        self.view.state_revision += 1;
        self.view.project_revision += 1;
        self.typed_revision = None;
        self.project = LoadedProject::Checking;
        self.view.project_health = ProjectHealth::Checking;
        self.view.active_document_descriptor = None;
    }

    pub(super) fn snapshot(&self) -> AppSnapshot {
        let active = self
            .view
            .active_file
            .as_ref()
            .and_then(|path| self.documents.get(&Utf8PathBuf::from(path)));
        AppSnapshot {
            pending_saves: self
                .documents
                .values()
                .filter(|document| document.buffer.save_state != DocumentSaveState::Saved)
                .map(|document| DocumentSaveStatus {
                    path: document.buffer.path.clone(),
                    state: document.buffer.save_state.clone(),
                })
                .collect(),
            tabs: self
                .tabs
                .iter()
                .filter_map(|path| {
                    self.documents
                        .get(path)
                        .map(|doc| EditorTab::from(&doc.buffer))
                })
                .collect(),
            active_buffer: active.map(|doc| EditorTab::from(&doc.buffer)),
            ..self.view.clone()
        }
    }

    /// Adopt `snapshot`'s view state. Buffers belong to `documents`; a snapshot
    /// only names the open tabs.
    pub(super) fn apply_view(&mut self, snapshot: AppSnapshot) {
        self.tabs = snapshot
            .tabs
            .iter()
            .map(|tab| Utf8PathBuf::from(&tab.path))
            .collect();
        self.view = snapshot;
    }
}

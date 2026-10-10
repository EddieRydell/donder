use super::LoadedProject;
use camino::Utf8PathBuf;
use donder_sequence_api::{
    AppSettings, AppSnapshot, AudioTransportSnapshot, DocumentDescriptor, DocumentSaveState,
    DocumentSaveStatus, DocumentViewId, DonderDeviceStatus, EditorBuffer, EditorTab,
    EditorViewMode, LiveOutputSnapshot, ProjectDiagnostic, ProjectHealth, WorkspaceEntry,
    WorkspaceExplorerState, WorkspaceLayoutState,
};
use std::collections::BTreeMap;

pub(super) struct WorkspaceView {
    pub state_revision: u32,
    pub project_epoch: u32,
    pub settings: AppSettings,
    pub workspace_layout: WorkspaceLayoutState,
    pub workspace_explorer: WorkspaceExplorerState,
    pub project_root: Option<String>,
    pub project_health: ProjectHealth,
    pub project_revision: u32,
    pub project_entries: Vec<WorkspaceEntry>,
    pub active_file: Option<String>,
    pub active_document_descriptor: Option<DocumentDescriptor>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub status: String,
    pub render_error: Option<String>,
    pub preview_error: Option<String>,
    pub preview_open: bool,
    pub audio_transport: AudioTransportSnapshot,
    pub live_output: LiveOutputSnapshot,
    pub devices: Vec<DonderDeviceStatus>,
}

pub(super) struct WorkingDocument {
    pub buffer: EditorBuffer,
    pub observed: Option<Vec<u8>>,
}

pub(super) struct WorkspaceState {
    pub view: WorkspaceView,
    pub project: LoadedProject,
    pub documents: BTreeMap<Utf8PathBuf, WorkingDocument>,
    pub tabs: Vec<Utf8PathBuf>,
    pub typed_revision: Option<u32>,
    pub close_authorization: Option<(u32, u32)>,
    pub render_target: Option<(donder_model::SetupId, donder_model::SequenceId)>,
    /// Persistence key of the sequence the transport has loaded.
    pub transport_view_key: Option<String>,
}

impl WorkspaceState {
    pub fn new(snapshot: AppSnapshot) -> Self {
        Self {
            view: WorkspaceView::from_snapshot(snapshot),
            project: LoadedProject::Closed,
            documents: BTreeMap::new(),
            tabs: Vec::new(),
            typed_revision: None,
            close_authorization: None,
            render_target: None,
            transport_view_key: None,
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let active = self
            .view
            .active_file
            .as_ref()
            .and_then(|path| self.documents.get(&Utf8PathBuf::from(path)));
        AppSnapshot {
            state_revision: self.view.state_revision,
            project_epoch: self.view.project_epoch,
            settings: self.view.settings.clone(),
            workspace_layout: self.view.workspace_layout.clone(),
            workspace_explorer: self.view.workspace_explorer.clone(),
            project_root: self.view.project_root.clone(),
            project_health: self.view.project_health.clone(),
            project_revision: self.view.project_revision,
            project_entries: self.view.project_entries.clone(),
            active_file: self.view.active_file.clone(),
            active_document_descriptor: self.view.active_document_descriptor.clone(),
            diagnostics: self.view.diagnostics.clone(),
            status: self.view.status.clone(),
            render_error: self.view.render_error.clone(),
            preview_error: self.view.preview_error.clone(),
            preview_open: self.view.preview_open,
            audio_transport: self.view.audio_transport.clone(),
            live_output: self.view.live_output.clone(),
            devices: self.view.devices.clone(),

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
            active_text: active
                .filter(|_| self.shows_active_as_text())
                .map(|doc| doc.buffer.text.clone()),
        }
    }

    /// Whether the editor shows the active document as text rather than in a
    /// GUI view. Matches the frontend's `effectiveEditorViewMode`.
    fn shows_active_as_text(&self) -> bool {
        self.view.project_health != ProjectHealth::Ready
            || matches!(self.view.settings.editor_view_mode, EditorViewMode::Text)
            || !self
                .view
                .active_document_descriptor
                .as_ref()
                .is_some_and(|descriptor| {
                    descriptor
                        .available_views
                        .iter()
                        .any(|view| *view != DocumentViewId::Text)
                })
    }

    /// Adopt `snapshot`'s view state. Buffers belong to `documents`; a snapshot
    /// only names the open tabs.
    pub fn apply_view(&mut self, snapshot: AppSnapshot) {
        self.tabs = snapshot
            .tabs
            .iter()
            .map(|tab| Utf8PathBuf::from(&tab.path))
            .collect();
        self.view = WorkspaceView::from_snapshot(snapshot);
    }
}

impl WorkspaceView {
    fn from_snapshot(snapshot: AppSnapshot) -> Self {
        Self {
            state_revision: snapshot.state_revision,
            project_epoch: snapshot.project_epoch,
            settings: snapshot.settings,
            workspace_layout: snapshot.workspace_layout,
            workspace_explorer: snapshot.workspace_explorer,
            project_root: snapshot.project_root,
            project_health: snapshot.project_health,
            project_revision: snapshot.project_revision,
            project_entries: snapshot.project_entries,
            active_file: snapshot.active_file,
            active_document_descriptor: snapshot.active_document_descriptor,
            diagnostics: snapshot.diagnostics,
            status: snapshot.status,
            render_error: snapshot.render_error,
            preview_error: snapshot.preview_error,
            preview_open: snapshot.preview_open,
            audio_transport: snapshot.audio_transport,
            live_output: snapshot.live_output,
            devices: snapshot.devices,
        }
    }
}

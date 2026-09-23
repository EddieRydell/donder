use super::LoadedProject;
use crate::dto::*;
use camino::Utf8PathBuf;
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
    pub package: PackageStatus,
}

pub(super) struct WorkingDocument {
    pub buffer: EditorBuffer,
    pub observed: Option<Vec<u8>>,
}

pub(super) struct WorkspaceState {
    gui_projection: Option<(u32, GuiDocumentResult)>,
    pub view: WorkspaceView,
    pub project: LoadedProject,
    pub documents: BTreeMap<Utf8PathBuf, WorkingDocument>,
    pub tabs: Vec<Utf8PathBuf>,
    pub typed_revision: Option<u32>,
    pub close_authorization: Option<(u32, u32)>,
    pub render_target: Option<(
        donder_language::setup::SetupId,
        donder_language::sequence::SequenceId,
    )>,
}

impl WorkspaceState {
    pub fn new(snapshot: AppSnapshot) -> Self {
        Self {
            gui_projection: None,
            view: WorkspaceView::from_snapshot(snapshot),
            project: LoadedProject::Closed,
            documents: BTreeMap::new(),
            tabs: Vec::new(),
            typed_revision: None,
            close_authorization: None,
            render_target: None,
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            gui_projection: self
                .gui_projection
                .as_ref()
                .filter(|(epoch, result)| {
                    *epoch == self.view.project_epoch
                        && self.typed_revision == Some(result.project_revision)
                        && result.project_revision == self.view.project_revision
                        && self.view.active_file.as_ref() == Some(&result.request.path)
                })
                .map(|(_, result)| result.clone()),
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
            package: self.view.package.clone(),

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
                .filter_map(|path| self.documents.get(path).map(|doc| doc.buffer.clone()))
                .collect(),
            active_buffer: self.view.active_file.as_ref().and_then(|path| {
                self.documents
                    .get(&Utf8PathBuf::from(path))
                    .map(|doc| doc.buffer.clone())
            }),
        }
    }

    pub fn apply_view(&mut self, mut snapshot: AppSnapshot) {
        self.tabs = snapshot
            .tabs
            .iter()
            .map(|buffer| Utf8PathBuf::from(&buffer.path))
            .collect();
        for buffer in std::mem::take(&mut snapshot.tabs) {
            let path = Utf8PathBuf::from(&buffer.path);
            self.documents
                .entry(path)
                .and_modify(|doc| doc.buffer = buffer.clone())
                .or_insert_with(|| WorkingDocument {
                    observed: Some(buffer.text.as_bytes().to_vec()),
                    buffer,
                });
        }
        self.view = WorkspaceView::from_snapshot(snapshot);
    }

    /// Build once per displayed document/revision, before publishing any snapshot.
    /// Save/render status updates reuse the same projection.
    pub fn refresh_gui_projection(&mut self) {
        let request = self
            .view
            .active_file
            .as_ref()
            .zip(self.view.active_document_descriptor.as_ref())
            .and_then(|(path, descriptor)| {
                [
                    DocumentViewId::Project,
                    DocumentViewId::Sequence,
                    DocumentViewId::Setup,
                    DocumentViewId::Layout,
                    DocumentViewId::Fixture,
                ]
                .into_iter()
                .find_map(|view| {
                    descriptor
                        .default_object_keys
                        .iter()
                        .find(|item| item.view == view)
                })
                .map(|item| GuiDocumentRequest {
                    project_revision: self.view.project_revision,
                    path: path.clone(),
                    view: item.view.clone(),
                    object_key: Some(item.object_key.clone()),
                })
            });
        let Some(request) =
            request.filter(|_| self.typed_revision == Some(self.view.project_revision))
        else {
            self.gui_projection = None;
            return;
        };
        if self.gui_projection.as_ref().is_some_and(|(epoch, result)| {
            *epoch == self.view.project_epoch && result.request == request
        }) {
            return;
        }
        self.gui_projection = match &self.project {
            LoadedProject::Ready(session) => Some((
                self.view.project_epoch,
                GuiDocumentResult {
                    document: crate::gui::project_gui_document(Some(session), &request),
                    project_revision: request.project_revision,
                    request,
                },
            )),
            _ => None,
        };
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
            package: snapshot.package,
        }
    }
}

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub state_revision: u32,
    pub project_epoch: u32,
    pub settings: AppSettings,
    pub workspace_layout: WorkspaceLayoutState,
    pub workspace_explorer: WorkspaceExplorerState,
    pub project_root: Option<String>,
    pub project_health: ProjectHealth,
    pub project_revision: u32,
    pub gui_projection: Option<GuiDocumentResult>,
    pub project_entries: Vec<WorkspaceEntry>,
    pub tabs: Vec<EditorBuffer>,
    pub pending_saves: Vec<DocumentSaveStatus>,
    pub active_file: Option<String>,
    pub active_buffer: Option<EditorBuffer>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProjectHealth {
    Closed,
    Ready,
    Checking,
    Invalid,
}

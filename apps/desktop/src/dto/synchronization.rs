use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DocumentSaveState {
    Saved,
    Dirty,
    Saving,
    Failed { message: String },
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentUpdate {
    pub project_epoch: u32,
    pub path: String,
    pub expected_document_revision: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiDocumentResult {
    pub request: GuiDocumentRequest,
    pub project_revision: u32,
    pub document: GuiDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WorkspaceTransition {
    CloseFile {
        path: String,
    },
    ReloadFile {
        path: String,
    },
    ReloadProject,
    OpenProject {
        path: String,
    },
    CreateProject {
        parent_path: String,
        directory_name: String,
    },
    CopyProject {
        parent_path: String,
        directory_name: String,
    },
    CloseApplication,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TransitionDecision {
    SaveAll,
    Discard,
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TransitionRequest {
    pub transition: WorkspaceTransition,
    pub project_epoch: u32,
    pub project_revision: u32,
    pub decision: Option<TransitionDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TransitionResult {
    Applied {
        snapshot: AppSnapshot,
        close_application: bool,
    },
    NeedsDecision {
        snapshot: AppSnapshot,
        dirty_paths: Vec<String>,
    },
    Cancelled {
        snapshot: AppSnapshot,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExternalConflictDecision {
    Reload,
    KeepWorkingCopy,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSaveStatus {
    pub path: String,
    pub state: DocumentSaveState,
}

use crate::dto::*;
use serde::{Deserialize, Serialize};
use specta::Type;

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
    /// The edits made since that revision, in order; each batch's ranges
    /// refer to the text the batches before it produced.
    pub changes: Vec<Vec<DocumentTextEdit>>,
}

/// A document's working text and the revision it is.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentText {
    pub document_revision: u32,
    pub text: String,
}

/// Language-server edits to one document, which may not be open: a rename
/// that reaches other files.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTextEdits {
    /// The document's `file:` URI.
    pub uri: String,
    pub edits: Vec<DocumentTextEdit>,
}

/// A range replaced with text; positions count UTF-16 code units.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTextEdit {
    pub range: TextRange,
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
        initial_color: String,
    },
    CopyProject {
        parent_path: String,
        directory_name: String,
    },
    /// A new project holding the layout, controllers and patch of the
    /// xLights show in `show_path`.
    ImportXLights {
        parent_path: String,
        directory_name: String,
        initial_color: String,
        show_path: String,
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

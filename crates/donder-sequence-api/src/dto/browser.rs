use crate::*;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BrowserCompileDiagnostic {
    pub start: u32,
    pub end: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BrowserCompileResult {
    pub definitions: Vec<String>,
    pub diagnostics: Vec<BrowserCompileDiagnostic>,
}

/// A page element tree measured by the website. Groups become layout groups and
/// fixtures become inline fixtures. Pixel order is depth-first and matches `render`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BrowserPageNode {
    Group {
        id: u32,
        name: String,
        children: Vec<BrowserPageNode>,
    },
    Fixture {
        id: u32,
        name: String,
        pixels: Vec<[f32; 2]>,
    },
}

/// Everything needed to create a browser session: the measured page and the
/// show that plays on it. They are the starting point, not history entries.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSessionConfig {
    pub page: Vec<BrowserPageNode>,
    pub project: BrowserProject,
}

/// A Donder project held by the website. Its setup's layout names page nodes:
/// the session plays the project's first sequence on the measured page, each
/// target bound to the page node with the same name.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProject {
    pub documents: Vec<BrowserProjectDocument>,
    pub assets: Vec<BrowserProjectAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProjectDocument {
    /// Project-relative, as in an import.
    pub path: String,
    pub text: String,
}

/// An asset the project references, served by the website.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProjectAsset {
    /// Project-relative, as the referencing document writes it.
    pub path: String,
    pub url: String,
}

/// An accepted change to a browser session, in the form a host can save and
/// replay to restore the session later.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BrowserOperation {
    Edit { edit: SequenceGuiEdit },
    Selection { edit: SequenceSelectionEdit },
    Source { path: String, source: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserReplayResult {
    /// How many operations were applied, in order.
    pub applied: u32,
    /// Why the next operation could not be applied, when replay stopped early.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserEditorState {
    pub revision: u32,
    pub request: GuiDocumentRequest,
    pub document: GuiDocument,
    pub settings: AppSettings,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BrowserSourceKind {
    Effect,
    Operator,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSourceDocument {
    pub path: String,
    pub kind: BrowserSourceKind,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSelectionResult {
    pub state: BrowserEditorState,
    pub selection: Option<SequenceSelection>,
    pub copied_count: u32,
    pub skipped_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserClipRaster {
    pub effect_id: u32,
    pub columns: u32,
    pub rows: u32,
    pub start_seconds: f32,
    pub duration_seconds: f32,
    pub pixels_rgba: Vec<u8>,
}

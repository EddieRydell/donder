use super::*;

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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BrowserPixel {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BrowserFrame {
    pub revision: u32,
    pub seconds: f32,
    pub pixels: Vec<BrowserPixel>,
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

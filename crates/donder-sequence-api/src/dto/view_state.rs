use super::*;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedEditorViewState {
    pub cursor_anchor: u32,
    pub cursor_head: u32,
    pub scroll_top: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSequenceViewportState {
    pub px_per_second: f32,
    #[serde(default = "default_audio_strip_height_px")]
    pub audio_strip_height_px: f32,
    pub row_heights: BTreeMap<String, f32>,
    pub scroll_x_seconds: f32,
    pub scroll_y: f32,
    pub active_mark_collection_key: Option<String>,
    pub visible_mark_collection_keys: Vec<String>,
}

fn default_audio_strip_height_px() -> f32 {
    38.0
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedGraphViewport {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedGraphNodeSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedGraphViewState {
    pub viewport: Option<PersistedGraphViewport>,
    pub node_sizes: BTreeMap<String, PersistedGraphNodeSize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedGraphViewStateUpdate {
    pub owned_path: Vec<crate::dto::GuiOwnedStep>,
    pub path: String,
    pub object_key: String,
    pub state: PersistedGraphViewState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedWindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedPreviewWindowState {
    pub open: bool,
    pub geometry: Option<PersistedWindowState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedEditorViewStateUpdate {
    pub path: String,
    pub state: PersistedEditorViewState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSequenceViewportStateUpdate {
    pub owned_path: Vec<crate::dto::GuiOwnedStep>,
    pub path: String,
    pub object_key: String,
    pub state: PersistedSequenceViewportState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRestoreState {
    pub editor_states: BTreeMap<String, PersistedEditorViewState>,
    pub sequence_viewports: BTreeMap<String, PersistedSequenceViewportState>,
    pub spatial_views: BTreeMap<String, PersistedSpatialViewState>,
    pub graph_views: BTreeMap<String, PersistedGraphViewState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SpatialGuideAxis {
    X,
    Y,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpatialGuide {
    pub axis: SpatialGuideAxis,
    pub position_meters: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSpatialViewState {
    pub guides: Vec<SpatialGuide>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSpatialViewStateUpdate {
    pub owned_path: Vec<crate::dto::GuiOwnedStep>,
    pub path: String,
    pub object_key: String,
    pub state: PersistedSpatialViewState,
}

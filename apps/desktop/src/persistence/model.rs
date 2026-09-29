use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::dto::{AppSettings, AppSnapshot, WorkspaceExplorerState, WorkspaceLayoutState};

pub(crate) const VERSION: u32 = 2;

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

#[derive(Debug, Clone)]
pub struct ProjectSessionRestore {
    pub session: PersistedProjectSession,
    pub stale_tabs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PersistedStore {
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) settings: AppSettings,
    #[serde(default)]
    pub(crate) workspace_layout: WorkspaceLayoutState,
    pub(crate) last_project: Option<String>,
    pub(crate) projects: BTreeMap<String, PersistedProjectSession>,
    pub(crate) main_window: Option<PersistedWindowState>,
    pub(crate) preview_window: PersistedPreviewWindowState,
}

impl PersistedStore {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err(format!(
                "Unsupported desktop state version {}",
                self.version
            ));
        }
        Ok(())
    }
}

impl Default for PersistedStore {
    fn default() -> Self {
        Self {
            version: VERSION,
            settings: AppSettings::default(),
            workspace_layout: WorkspaceLayoutState::default(),
            last_project: None,
            projects: BTreeMap::new(),
            main_window: None,
            preview_window: PersistedPreviewWindowState {
                open: false,
                geometry: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedProjectSession {
    pub tabs: Vec<String>,
    pub active_file: Option<String>,
    pub audio_position_seconds: f32,
    pub audio_home_seconds: f32,
    pub editor_states: BTreeMap<String, PersistedEditorViewState>,
    pub sequence_viewports: BTreeMap<String, PersistedSequenceViewportState>,
    pub spatial_views: BTreeMap<String, PersistedSpatialViewState>,
    pub graph_views: BTreeMap<String, PersistedGraphViewState>,
    #[serde(default)]
    pub workspace_explorer: WorkspaceExplorerState,
}

impl PersistedProjectSession {
    pub(crate) fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_file: None,
            audio_position_seconds: 0.0,
            audio_home_seconds: 0.0,
            editor_states: BTreeMap::new(),
            sequence_viewports: BTreeMap::new(),
            spatial_views: BTreeMap::new(),
            graph_views: BTreeMap::new(),
            workspace_explorer: WorkspaceExplorerState::default(),
        }
    }

    pub(crate) fn with_snapshot(mut self, snapshot: &AppSnapshot) -> Self {
        self.tabs = snapshot.tabs.iter().map(|tab| tab.path.clone()).collect();
        self.active_file = snapshot.active_file.clone();
        self.audio_position_seconds = snapshot.audio_transport.position_seconds;
        self.audio_home_seconds = snapshot.audio_transport.home_seconds;
        self.workspace_explorer = snapshot.workspace_explorer.clone();
        self
    }
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

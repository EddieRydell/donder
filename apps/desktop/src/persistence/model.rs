use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::dto::{
    AppSettings, AppSnapshot, PlaybackRange, WorkspaceExplorerState, WorkspaceLayoutState,
};

pub(crate) const VERSION: u32 = 2;

pub use donder_sequence_api::{
    PersistedEditorViewState, PersistedEditorViewStateUpdate, PersistedGraphViewState,
    PersistedGraphViewStateUpdate, PersistedPreviewWindowState, PersistedSequenceViewportState,
    PersistedSequenceViewportStateUpdate, PersistedSpatialViewState,
    PersistedSpatialViewStateUpdate, PersistedWindowState, ProjectRestoreState,
};
#[cfg(test)]
pub use donder_sequence_api::{SpatialGuide, SpatialGuideAxis};
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
    /// Claim tokens for Donder controllers, keyed by device ID. They belong to
    /// this computer, not to a project.
    #[serde(default)]
    pub(crate) device_tokens: BTreeMap<String, String>,
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
            device_tokens: BTreeMap::new(),
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
    pub sequence_transports: BTreeMap<String, PersistedSequenceTransport>,
    #[serde(default)]
    pub workspace_explorer: WorkspaceExplorerState,
}

/// A sequence's playback range and loop toggle, keyed like its viewport.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSequenceTransport {
    pub range: Option<PlaybackRange>,
    pub looping: bool,
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
            sequence_transports: BTreeMap::new(),
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

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use donder_editor::dto::{
    AppSettings, AppSnapshot, PlaybackRange, WorkspaceExplorerState, WorkspaceLayoutState,
};

pub(crate) use donder_editor::dto::{
    PersistedEditorViewState, PersistedEditorViewStateUpdate, PersistedGraphViewState,
    PersistedGraphViewStateUpdate, PersistedPreviewWindowState, PersistedSequenceViewportState,
    PersistedSequenceViewportStateUpdate, PersistedSpatialViewState,
    PersistedSpatialViewStateUpdate, PersistedWindowState, ProjectRestoreState,
};
#[cfg(test)]
pub(crate) use donder_editor::dto::{SpatialGuide, SpatialGuideAxis};
#[derive(Debug, Clone)]
pub(crate) struct ProjectSessionRestore {
    pub session: PersistedProjectSession,
    pub stale_tabs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PersistedStore {
    pub(crate) settings: AppSettings,
    pub(crate) workspace_layout: WorkspaceLayoutState,
    pub(crate) last_project: Option<String>,
    pub(crate) projects: BTreeMap<String, PersistedProjectSession>,
    pub(crate) main_window: Option<PersistedWindowState>,
    pub(crate) preview_window: PersistedPreviewWindowState,
    /// Whether live output is on, for every project.
    pub(crate) live_output: bool,
}

impl Default for PersistedStore {
    fn default() -> Self {
        Self {
            settings: AppSettings::default(),
            workspace_layout: WorkspaceLayoutState::default(),
            last_project: None,
            projects: BTreeMap::new(),
            main_window: None,
            preview_window: PersistedPreviewWindowState {
                open: false,
                geometry: None,
            },
            live_output: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PersistedProjectSession {
    pub tabs: Vec<String>,
    pub active_file: Option<String>,
    pub editor_states: BTreeMap<String, PersistedEditorViewState>,
    pub sequence_viewports: BTreeMap<String, PersistedSequenceViewportState>,
    pub spatial_views: BTreeMap<String, PersistedSpatialViewState>,
    pub graph_views: BTreeMap<String, PersistedGraphViewState>,
    pub sequence_transports: BTreeMap<String, PersistedSequenceTransport>,
    pub workspace_explorer: WorkspaceExplorerState,
}

/// A sequence's playhead, playback range and loop toggle, keyed like its viewport.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PersistedSequenceTransport {
    pub position_seconds: f64,
    pub range: Option<PlaybackRange>,
    pub looping: bool,
}

impl PersistedProjectSession {
    pub(crate) fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_file: None,
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
        self.workspace_explorer = snapshot.workspace_explorer.clone();
        self
    }
}

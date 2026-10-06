use serde::{Deserialize, Serialize};

use crate::dto::{AudioTransportState, PlaybackSpeed, PreviewAppearance};
use crate::persistence::PersistedWindowState;

pub(crate) const PREVIEW_HOST_ARGUMENT: &str = "--donder-preview-host";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewStartup {
    pub(crate) appearance: PreviewAppearance,
    pub(crate) window: Option<PreviewWindowState>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewWindowState {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) maximized: bool,
}

impl From<PersistedWindowState> for PreviewWindowState {
    fn from(value: PersistedWindowState) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
            maximized: value.maximized,
        }
    }
}

impl From<PreviewWindowState> for PersistedWindowState {
    fn from(value: PreviewWindowState) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
            maximized: value.maximized,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PreviewCommand {
    ReplaceContent {
        revision: u64,
        instances: Vec<[f32; 4]>,
        sequence: Option<Vec<u8>>,
    },
    SetClock {
        generation: u32,
        state: AudioTransportState,
        position_seconds: f32,
        start_delay_seconds: f32,
        playback_speed: PlaybackSpeed,
    },
    SetAppearance {
        appearance: PreviewAppearance,
    },
    Focus,
    Close,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PreviewEvent {
    Ready,
    GeometryChanged { window: PreviewWindowState },
    Closed { window: PreviewWindowState },
    Error { message: String },
}

pub(crate) fn write_message(
    writer: &mut impl std::io::Write,
    message: &impl Serialize,
) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, message)
        .map_err(|error| format!("Cannot encode Preview message: {error}"))?;
    writer
        .write_all(b"\n")
        .map_err(|error| format!("Cannot write Preview message: {error}"))?;
    writer
        .flush()
        .map_err(|error| format!("Cannot flush Preview message: {error}"))
}

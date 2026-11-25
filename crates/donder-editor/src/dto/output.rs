use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceExportPort {
    pub index: u32,
    pub label: String,
    pub channels: u32,
    /// Where FSEQ files place its first channel, from 1; none leaves it out.
    pub fseq_channel: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceExportOptions {
    pub ports: Vec<SequenceExportPort>,
    /// The whole-millisecond FSEQ step closest to the authored frame rate.
    pub fseq_step_millis: u8,
    /// The longest FSEQ step that plays as smoothly as live output.
    pub fseq_smooth_step_millis: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LiveOutputSnapshot {
    /// Whether the user turned live output on. It stays on across sequences,
    /// performances and restarts; `state` is what the output does now.
    pub enabled: bool,
    pub state: LiveOutputState,
    pub generation: u32,
    pub active_controller_count: u32,
    pub active_universe_count: u32,
    pub controllers: Vec<LiveOutputControllerSnapshot>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LiveOutputState {
    Disabled,
    Preparing,
    Holding,
    Streaming,
    Testing,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LiveOutputControllerSnapshot {
    pub id: String,
    pub state: LiveOutputControllerState,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LiveOutputControllerState {
    Opening,
    Active,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Point3Meters {
    pub x_meters: f32,
    pub y_meters: f32,
    pub z_meters: f32,
}

/// A Donder controller seen on the network, merged with this editor's
/// live-output connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DonderDeviceStatus {
    pub id: String,
    pub name: String,
    pub address: String,
    pub network: DonderDeviceNetwork,
    /// False when the controller's sequence format differs from this editor's.
    pub firmware_current: bool,
    pub connection: DonderDeviceConnection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DonderDeviceNetwork {
    AccessPoint,
    Station,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DonderDeviceConnection {
    /// Not connected: no controller in the project uses this device, or
    /// live output is off.
    Unused,
    /// Live output is connecting to this device.
    Connecting,
    /// Live output drives this device.
    Connected {
        clock_uncertainty_micros: Option<u32>,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DonderDeviceNetworkRequest {
    pub ssid: String,
    pub password: String,
}

/// What an output test lights, in the open document.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OutputTestTarget {
    /// The open controller's channels, from 0 across all its universes.
    #[serde(rename_all = "camelCase")]
    Channels {
        start_channel: u32,
        channel_count: u32,
    },
    /// The open layout's fixture, or a group's fixtures.
    LayoutItem { item: u32 },
    /// One of the project's routes.
    Route { route: u32 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OutputTestPattern {
    Solid { red: u8, green: u8, blue: u8 },
    Count,
    Order,
    Chase,
    Rise,
}

/// Progress of a sequence's FSEQ or video export, sent while it runs.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "stage", rename_all = "camelCase")]
pub enum ExportProgress {
    /// Preparing the show for the selected outputs.
    Preparing,
    PreparingAudio,
    Rendering {
        completed: u32,
        total: u32,
    },
    Saving,
}

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceExportPort {
    pub index: u32,
    pub label: String,
    pub channels: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveOutputSnapshot {
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveOutputControllerSnapshot {
    pub id: String,
    pub state: LiveOutputControllerState,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCapabilities {
    pub sequence_format: u32,
    pub max_payload_bytes: u32,
    pub max_pixels: u32,
    pub max_graph_nodes: u32,
    pub max_workspace_bytes: u32,
    pub output: DeviceOutputCapabilities,
    pub sequence_storage: DeviceSequenceStorage,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DeviceOutputCapabilities {
    Ws281x {
        lanes: u32,
        channels_per_lane: u32,
        channel_multiple: u32,
        frame_rate: u32,
    },
    EvaluationOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceSequenceStorage {
    Persistent,
}

#[derive(Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionedDevice {
    pub address: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSerialPort {
    pub path: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DevicePlaybackMode {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DevicePlaybackStatus {
    pub mode: DevicePlaybackMode,
    pub position_micros: u32,
    pub duration_micros: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DeviceTransportStatus {
    pub playback: Option<DevicePlaybackStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ControllerOutputTest {
    pub controller_index: u32,
    pub port: u32,
    pub start_slot: u16,
    pub slot_count: u16,
    pub value: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFirmwareInfo {
    pub version: String,
    pub image_bytes: u32,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "stage", rename_all = "camelCase")]
pub enum DeviceInstallProgress {
    Connecting,
    Writing { completed: u32, total: u32 },
    Verifying,
    Restarting,
}

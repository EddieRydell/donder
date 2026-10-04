use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AudioTransportState {
    Unloaded,
    Playing,
    Paused,
    Stopped,
    Ended,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AudioTransportSnapshot {
    pub state: AudioTransportState,
    pub source: Option<SequenceAudio>,
    pub generation: u32,
    pub position_seconds: f32,
    pub start_delay_seconds: f32,
    pub home_seconds: f32,
    pub duration_seconds: f32,
    pub last_error: Option<String>,
}

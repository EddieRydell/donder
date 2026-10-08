use crate::*;
use donder_runtime::{FrameTiming, PlaybackRate};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::num::NonZeroU32;

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
    pub playback_speed: PlaybackSpeed,
    pub range: Option<PlaybackRange>,
    pub looping: bool,
    pub last_error: Option<String>,
}

/// A span of the sequence that playback stops at the end of, or loops when looping.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackRange {
    pub start_seconds: f32,
    pub end_seconds: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackFrameTiming {
    Scaled,
    Constant,
}

/// Show microseconds per wall second; 1_000_000 is normal speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSpeed {
    pub show_micros_per_second: u32,
    pub frame_timing: PlaybackFrameTiming,
}

impl From<PlaybackRate> for PlaybackSpeed {
    fn from(rate: PlaybackRate) -> Self {
        Self {
            show_micros_per_second: rate.show_micros_per_second().get(),
            frame_timing: match rate.frame_timing() {
                FrameTiming::Scaled => PlaybackFrameTiming::Scaled,
                FrameTiming::Constant => PlaybackFrameTiming::Constant,
            },
        }
    }
}

impl TryFrom<PlaybackSpeed> for PlaybackRate {
    type Error = String;

    fn try_from(speed: PlaybackSpeed) -> Result<Self, Self::Error> {
        let show_micros_per_second = NonZeroU32::new(speed.show_micros_per_second)
            .ok_or_else(|| "Playback speed must be greater than zero.".to_string())?;
        Ok(PlaybackRate::new(
            show_micros_per_second,
            match speed.frame_timing {
                PlaybackFrameTiming::Scaled => FrameTiming::Scaled,
                PlaybackFrameTiming::Constant => FrameTiming::Constant,
            },
        ))
    }
}

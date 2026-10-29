//! The wire protocol between the editor and a Donder controller: HTTP paths
//! and the token header, the JSON bodies of commands and replies, and the UDP
//! clock exchange on the controller's HTTP port. The desktop and the firmware both read and write these
//! types, so each message has one layout.
#![no_std]

pub mod clock;

use core::num::NonZeroU32;
use donder_runtime::{FrameTiming, PlaybackRate};
use serde::{Deserialize, Serialize};

/// The header that carries the claim token on every request but a claim.
pub const TOKEN_HEADER: &str = "X-Donder-Token";

/// HTTP paths. A claim replies with the token; a name or network change
/// takes its value as the body; a show upload replies
/// starting with [`LOADED`](super::LOADED).
pub mod path {
    pub const CLAIM: &str = "/claim";
    pub const NAME: &str = "/name";
    pub const NETWORK: &str = "/network";
    pub const SHOW: &str = "/show";
    /// A [`Command`](super::Command), replied to with [`OK`](super::OK).
    pub const CONTROL: &str = "/control";
    /// The [`TransportStatus`](super::TransportStatus), which the transport
    /// commands also reply with.
    pub const TRANSPORT: &str = "/transport";
    pub const PLAY: &str = "/transport/play";
    pub const PAUSE: &str = "/transport/pause";
    pub const STOP: &str = "/transport/stop";
}

/// The reply to an accepted [`Command`].
pub const OK: &str = "OK";
/// How the reply to an admitted upload starts.
pub const LOADED: &str = "LOADED";

/// A playback command on the editor's clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Command {
    /// The editor's clock read `master_micros` when the controller's read
    /// `local_micros`.
    SyncClock {
        boot_id: u32,
        clock_id: u32,
        local_micros: u64,
        master_micros: u64,
        rate_ppb: i32,
        valid_for_micros: u32,
    },
    /// Take `action` at `position_micros` of the loaded show when the
    /// editor's clock reaches `at_micros`.
    Schedule {
        boot_id: u32,
        clock_id: u32,
        command_id: u32,
        at_micros: u64,
        action: Action,
        position_micros: u32,
        #[serde(with = "WirePlaybackRate")]
        rate: PlaybackRate,
        looping: bool,
        show_id: u32,
        show_bytes: u32,
    },
    Cancel {
        command_id: u32,
    },
}

/// A controller's playback state. A show reaches `Ended` on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Playing,
    Paused,
    Stopped,
    Ended,
}

/// What a transport command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Play,
    Pause,
    Stop,
}

impl From<Action> for Mode {
    fn from(action: Action) -> Self {
        match action {
            Action::Play => Self::Playing,
            Action::Pause => Self::Paused,
            Action::Stop => Self::Stopped,
        }
    }
}

/// How a [`PlaybackRate`] travels; a zero speed does not decode.
#[derive(Serialize, Deserialize)]
#[serde(remote = "PlaybackRate", rename_all = "camelCase", deny_unknown_fields)]
struct WirePlaybackRate {
    #[serde(getter = "show_micros_per_second")]
    show_micros_per_second: NonZeroU32,
    #[serde(getter = "frame_timing", with = "WireFrameTiming")]
    frame_timing: FrameTiming,
}

fn show_micros_per_second(rate: &PlaybackRate) -> NonZeroU32 {
    rate.show_micros_per_second()
}

fn frame_timing(rate: &PlaybackRate) -> FrameTiming {
    rate.frame_timing()
}

impl From<WirePlaybackRate> for PlaybackRate {
    fn from(rate: WirePlaybackRate) -> Self {
        Self::new(rate.show_micros_per_second, rate.frame_timing)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "FrameTiming", rename_all = "camelCase")]
enum WireFrameTiming {
    Scaled,
    Constant,
}

/// The loaded show's playback, if a show is loaded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransportStatus {
    pub playback: Option<PlaybackStatus>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatus {
    pub mode: Mode,
    pub position_micros: u32,
    /// The loaded show's identity and length.
    pub show_id: u32,
    pub show_bytes: u32,
    /// The last command accepted; a later one has a greater id.
    pub command_id: u32,
}

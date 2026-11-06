//! The wire protocol between the editor and a Donder controller: HTTP paths,
//! the JSON bodies of commands and replies, and the UDP
//! clock exchange on the controller's HTTP port. The desktop and the firmware both read and write these
//! types, so each message has one layout.
#![no_std]

extern crate alloc;

pub mod clock;

use alloc::string::String;
use core::num::NonZeroU32;
use core::ops::RangeInclusive;
use donder_runtime::{FrameTiming, PlaybackRate};
use serde::{Deserialize, Serialize};

/// The WPA2 password of a controller's own access point.
pub const ACCESS_POINT_PASSWORD: &str = "12345678";

/// How a controller announces itself over mDNS: the `_donder._tcp` service
/// with these TXT keys.
pub mod advertisement {
    pub const SERVICE: &str = "_donder";
    pub const PROTOCOL: &str = "_tcp";
    /// The controller's [`DonderDeviceId`]-style hex id.
    pub const ID: &str = "id";
    pub const NAME: &str = "name";
    /// The show format the controller plays, `donder_runtime::FORMAT_VERSION`.
    pub const FORMAT: &str = "format";
    /// The [`Network`](super::Network) the controller is on.
    pub const NETWORK: &str = "network";
}

/// Which network a controller is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    /// A network it joined.
    Station,
    /// Its own access point.
    AccessPoint,
}

impl Network {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Station => "station",
            Self::AccessPoint => "accessPoint",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        [Self::Station, Self::AccessPoint]
            .into_iter()
            .find(|network| network.label() == label)
    }
}

/// The longest controller name, in bytes.
pub const MAX_NAME_BYTES: usize = 32;

/// A controller name: 1 to [`MAX_NAME_BYTES`] bytes without control characters.
pub fn valid_name(name: &str) -> bool {
    (1..=MAX_NAME_BYTES).contains(&name.len()) && !name.chars().any(char::is_control)
}

/// The body of `PUT /network`: a WPA2 network to join after a restart.
// Deliberately no Debug: the password must not enter logs.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkCredentials {
    pub ssid: String,
    pub password: String,
}

impl NetworkCredentials {
    pub const SSID_BYTES: RangeInclusive<usize> = 1..=32;
    pub const PASSWORD_BYTES: RangeInclusive<usize> = 8..=64;

    pub fn is_valid(&self) -> bool {
        Self::SSID_BYTES.contains(&self.ssid.len())
            && Self::PASSWORD_BYTES.contains(&self.password.len())
    }
}

/// HTTP paths. A name or network change
/// takes its value as the body; a show upload replies
/// starting with [`LOADED`](super::LOADED).
pub mod path {
    pub const NAME: &str = "/name";
    pub const NETWORK: &str = "/network";
    pub const SHOW: &str = "/show";
    /// Evaluates one frame of the loaded show, for measuring.
    pub const FRAME: &str = "/frame";
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
        clock_id: NonZeroU32,
        local_micros: u64,
        master_micros: u64,
        rate_ppb: i32,
        valid_for_micros: u32,
    },
    /// Take `action` at `position_micros` of the loaded show when the
    /// editor's clock reaches `at_micros`.
    Schedule {
        boot_id: u32,
        clock_id: NonZeroU32,
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
#[serde(deny_unknown_fields)]
pub struct TransportStatus {
    pub playback: Option<PlaybackStatus>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaybackStatus {
    pub mode: Mode,
    pub position_micros: u32,
    /// The loaded show's identity and length.
    pub show_id: u32,
    pub show_bytes: u32,
    /// The last command accepted; a later one has a greater id.
    pub command_id: u32,
}

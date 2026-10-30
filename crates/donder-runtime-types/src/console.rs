//! Commands a console's playback takes while it runs. The host routes inputs
//! to commands; every device that plays the console receives the same
//! commands, so a frame depends only on the time and the commands applied
//! before it.

use crate::{SampleDuration, SampleTime};

/// Indices of a prepared console's voices, live mark collections and live
/// automations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConsoleCommand {
    /// Start a voice from its beginning, restarting it if it is playing. It
    /// plays for `duration`, or until stopped when `None`.
    Start {
        voice: u16,
        duration: Option<SampleDuration>,
    },
    /// End a voice; a voice that is not playing is unchanged.
    Stop { voice: u16 },
    /// Add a mark to a live mark collection.
    Mark { collection: u16 },
    /// Set a live automation to a value in `0..=1`.
    Set { automation: u16, value: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConsoleEvent {
    pub time: SampleTime,
    pub command: ConsoleCommand,
}

//! Commands a rig's playback takes while it runs. The host routes inputs to
//! commands; every device that plays the rig receives the same commands, so
//! a frame depends only on the time and the commands applied before it.

use crate::SampleTime;

/// Indices of a prepared rig's voices, live mark collections and controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RigCommand {
    /// Start a voice from its beginning, restarting it if it is playing.
    Start { voice: u16 },
    /// End a voice; a voice that is not playing is unchanged.
    Stop { voice: u16 },
    /// Add a mark to a live mark collection.
    Mark { collection: u16 },
    /// Set a control to a position in `0..=1`.
    Control { control: u16, value: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigEvent {
    pub time: SampleTime,
    pub command: RigCommand,
}

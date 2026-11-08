//! Inputs a console's playback takes while it runs. A host reads its input
//! devices and turns what they send into input events on the console's
//! controls; every device that plays the console routes the same events
//! itself, so a frame depends only on the time and the events applied before
//! it.

use crate::SampleTime;

/// What happened to a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputHappening {
    Press,
    Release,
    Move,
}

/// One input on one of a console's controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputEvent {
    /// The control, by index in the console's controls.
    pub control: u16,
    /// The note of a control reading notes; every note is its own press.
    pub note: Option<u8>,
    pub happening: InputHappening,
    /// A press's velocity, an axis's position, or 0 for a release, in `0..=1`.
    pub value: f32,
}

/// An input at its time on the session clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConsoleEvent {
    pub time: SampleTime,
    pub input: InputEvent,
}

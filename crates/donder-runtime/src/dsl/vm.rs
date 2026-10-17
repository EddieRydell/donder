mod context;
pub(crate) use context::NoSignals;
mod automation;
mod strip;
mod workspace;

pub(crate) use automation::{Override, StateRecord, States, shape as state_shape};
pub(crate) use strip::{Bound, Pixels, STRIP, ScanQuery, SourceWeights, Strip, StripSignals};
pub(crate) use workspace::{OUTSIDE, StripSlots, StripWorkspace};

use donder_runtime_types::SampleDuration;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunContext {
    pub progress: f32,
    pub time: SampleDuration,
    pub duration: SampleDuration,
    /// The sequence's frames per second.
    pub frame_rate: f32,
    pub pixel_count: i32,
}

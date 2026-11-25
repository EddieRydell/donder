mod context;
pub(crate) use context::NoSignals;
mod assets;
mod strip;
mod workspace;

pub(crate) use strip::{
    Bound, Extent, NeighborWeights, Pixels, STRIP, ScanQuery, Strip, StripSignals,
};
pub(crate) use workspace::{OUTSIDE, StripSlots, StripWorkspace};

use donder_runtime_types::{SampleDuration, SampleTime};

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunContext {
    pub time: SampleDuration,
    pub duration: SampleDuration,
    /// The sequence's frames per second.
    pub frame_rate: f32,
    pub pixel_count: i32,
    /// The show time its time counts from.
    pub origin: SampleTime,
}

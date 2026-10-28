mod context;
pub(crate) use context::NoSignals;
mod automation;
mod strip;
mod workspace;

pub(crate) use automation::{
    Override, ParamsRef, Scratch, params as params_at, set_value, shape as scratch_shape,
};
pub(crate) use strip::{Bound, NeighborWeights, Pixels, STRIP, ScanQuery, Strip, StripSignals};
pub(crate) use workspace::{OUTSIDE, StripSlots, StripWorkspace};

use donder_runtime_types::SampleDuration;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunContext {
    pub progress: f32,
    pub time: SampleDuration,
    pub duration: SampleDuration,
    /// The sequence's frames per second.
    pub frame_rate: f32,
    /// Frames since the frame before; one after a seek.
    pub frames_elapsed: u32,
    pub pixel_count: i32,
}

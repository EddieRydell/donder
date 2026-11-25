pub(crate) use donder_runtime_types::bytecode;
mod vm;

#[cfg(feature = "host")]
pub(crate) use vm::NoSignals;
pub(crate) use vm::{
    Bound, Extent, NeighborWeights, OUTSIDE, Pixels, RunContext, STRIP, ScanQuery, Strip,
    StripSignals, StripSlots, StripWorkspace,
};

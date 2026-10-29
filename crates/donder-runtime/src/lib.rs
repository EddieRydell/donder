#![no_std]
#![recursion_limit = "256"]
#![deny(unsafe_code)]
#![deny(private_interfaces, private_bounds)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

extern crate alloc;

// This facade is the complete external API. A prepared show owns its bytes and
// plays it in a buffer it owns; the host feature builds shows. Language values
// and executable definitions belong to donder-language; the VM is an
// implementation detail.
pub use playback::{FixtureFrame, OutputFrame, SequenceFrame};
pub use playback_rate::{FrameTiming, PlaybackRate};
pub use show::{PreparedShow, ShowPlayback};
pub use view::{FORMAT_VERSION, LoadError};

pub use crate::format::{Fixture, Output};

#[cfg(feature = "host")]
pub use host::{
    ClipHandle, ClipRaster, OutputHandle, ShowBuilder, ShowClip, SignalHandle, TargetHandle,
};

use donder_runtime_types::sampling;

mod arena;
mod dsl;
mod evaluation;
#[cfg(feature = "host")]
mod host;
// The show format is written by the host builder; a player alone reads only
// part of it.
#[cfg_attr(not(feature = "host"), allow(dead_code))]
mod format;
mod patch;
mod playback;
mod playback_rate;
mod resources;
mod scan;
mod sections;
mod show;
mod targets;
mod view;
mod workspace;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;

#![no_std]
#![recursion_limit = "256"]
#![deny(unsafe_code)]
#![deny(unreachable_pub)]
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

#[cfg(feature = "host")]
extern crate alloc;

// This facade is the complete external API. The core plays a trusted image
// in a caller's buffer without allocating; the host feature builds images and
// owns them. Language values and executable definitions belong to
// donder-language; the VM is an implementation detail.
pub use image::{FORMAT_VERSION, Image, LoadError, header_bytes, image_len};
pub use playback::{FixtureFrame, OutputFrame, Playback, SequenceFrame};
pub use playback_rate::{FrameTiming, PlaybackRate};
pub use workspace::{LIVE_MARK_CAPACITY, Layout};

pub use crate::format::{Fixture, Output};

#[cfg(feature = "host")]
pub use host::{
    ClipHandle, ClipRaster, ImageClip, OutputHandle, PreparedShow, ShowBuilder, ShowPlayback,
    SignalHandle, TargetHandle,
};

use donder_runtime_types::sampling;

mod arena;
mod dsl;
mod evaluation;
#[cfg(feature = "host")]
mod host;
// The image format is written by the host builder; a player alone reads only
// part of it.
#[cfg_attr(not(feature = "host"), allow(dead_code))]
mod format;
mod image;
mod patch;
mod playback;
mod playback_rate;
mod resources;
mod scan;
mod sections;
mod targets;
mod workspace;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;

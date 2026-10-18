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
pub use playback::{FixtureFrame, OutputFrame, Playback, SequenceFrame};
pub use playback_rate::{FrameTiming, PlaybackRate};
pub use show::{FORMAT_VERSION, LoadError, Show, header_bytes, image_len};
pub use workspace::{LIVE_MARK_CAPACITY, Layout};

pub use donder_runtime_types::image::{Fixture, Output};

#[cfg(feature = "host")]
pub use host::{
    ClipSampler, EffectHandle, FixtureHandle, LookupHandle, OutputHandle, PreparedRig,
    PreparedSequence, RigPlayback, SequenceBuilder, SequenceClip, SequencePlayback, SequenceRoot,
    SignalHandle, TargetHandle, VoiceLength, WindowHandle,
};

use donder_runtime_types::sampling;

mod arena;
mod dsl;
mod evaluation;
#[cfg(feature = "host")]
mod host;
mod patch;
mod playback;
mod playback_rate;
mod resources;
mod scan;
mod sections;
mod show;
mod targets;
mod workspace;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;

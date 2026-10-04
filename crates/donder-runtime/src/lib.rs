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

#[cfg_attr(test, macro_use)]
extern crate alloc;

// This facade is the complete external API. Language values and executable
// definitions belong to donder-language; the VM is an implementation detail.
pub use archive::{
    FORMAT_VERSION, HEADER_BYTES, LoadError, LoadLimits, decode_sequence, encode_sequence,
    payload_length,
};
pub use clip::{ClipSampler, SequenceClip};
pub use sequence::{
    EffectHandle, FixtureHandle, LookupHandle, OutputHandle, SequenceBuilder, SequenceRoot,
    SignalHandle, TargetHandle, WindowHandle,
};
pub use sequence::{
    FixtureFrame, OutputFrame, PreparedOutput, PreparedSequence, SequenceFrame, SequencePlayback,
};
pub use signal::PreparedFixture;

use donder_language::{automation, sampling, values};

mod archive;
mod clip;
mod dsl;
mod evaluation;
mod patch;
mod sections;
mod sequence;
mod signal;
mod targets;

#[cfg(test)]
extern crate self as donder_runtime;
#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;

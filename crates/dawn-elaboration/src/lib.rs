#![deny(unsafe_code)]
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

pub mod fixture;
mod output;
mod sequence;
pub use dawn_language::values::{SampleDuration, SampleTime};
pub use dawn_runtime::signal::{
    EvaluatedFrame as RenderedFrame, EvaluationWorkspace, PreparedSignalGraph, RenderedFixture,
};
pub(crate) use dawn_runtime::signal::{
    PreparedAutomation, PreparedEffect, PreparedEffectAutomation, PreparedEffectImplementation,
    PreparedLayer,
};
pub use output::*;
pub use sequence::raster::{
    EffectRasterPrepareBatch, EffectRasterWorkspace, PreparedEffectRasterRenderer,
    PreparedEffectRasterSample,
};
pub(crate) use sequence::renderer::MAX_GENERATED_EFFECTS;
pub use sequence::renderer::RenderError;
pub use sequence::targets::RenderedTargetPixelAddress;
pub use sequence::{elaborate_sequence, resolve_effect_target_pixel_addresses};

pub(crate) use sequence::effects::parameters::EffectParamTiming;
pub(crate) use sequence::fixtures::PreparedFixture;

pub mod native_effect;
#[cfg(test)]
mod tests;

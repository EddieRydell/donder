//! What Donder playback accepts and what it means: authored values, the strip
//! bytecode, program and invocation descriptions, prepared playback inputs and
//! the sampling math shared by constant folding and evaluation.
//!
//! An item belongs here only if the runtime consumes it and an upstream crate
//! produces it.
#![no_std]
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

extern crate alloc;

// Shared executable values use one pointer policy across compiler and runtime.
#[cfg(not(feature = "atomic"))]
pub use alloc::rc::Rc as Shared;
#[cfg(feature = "atomic")]
pub use alloc::sync::Arc as Shared;

mod automation;
mod bindings;
pub mod bytecode;
mod invocation;
mod operator;
mod prepared;
mod sample;
pub mod sampling;
mod types;
mod values;

pub use automation::{
    AutomationMapping, AutomationValue, automation_value_at_position, curve_window_into,
};
pub use bindings::{BindingError, BoundParams};
pub use bytecode::BytecodeProgram;
pub use invocation::{OperatorInvocation, SampleInvocation};
pub use operator::OperatorProgram;
pub use prepared::{
    AutomatedQuantity, FixtureGeometry, OutputEncoding, PixelEncoding, PreparedAutomation,
    RgbOrder, SequenceTiming, SequenceWindow, SpatialContext, TargetGeometry, TargetPixel,
    TargetScope, WhitePosition,
};
pub use sample::SampleProgram;
pub use types::{Identifier, IdentifierError, Type, Value};
pub use values::{
    Color, Curve, CurvePoint, CurveValidationError, Gradient, GradientStop,
    GradientValidationError, MICROS_PER_SECOND, Marks, Microseconds, SampleDuration, SampleTime,
    SampleTimeError, sample_duration_from_seconds_f32, sample_duration_seconds_f32,
    sample_time_from_frame, sample_time_from_seconds_f32, sample_time_seconds_f32,
    sample_time_with_seconds_offset,
};

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
mod console;
mod prepared;
mod program;
pub mod sampling;
mod types;
mod values;
mod word;

pub use automation::{
    AutomationMapping, AutomationValue, automation_value_at_position, curve_window_into,
    curve_window_points,
};
pub use bindings::{BindingError, BoundParams};
pub use bytecode::BytecodeProgram;
pub use console::{ConsoleCommand, ConsoleEvent};
pub use prepared::{
    AutomatedQuantity, FixtureGeometry, OutputEncoding, PixelEncoding, PreparedAutomation,
    RgbOrder, SequenceTiming, SpatialContext, TargetGeometry, TargetPixel, TargetScope,
    WhitePosition,
};
pub use program::{BoundProgram, Program};
pub use types::{Identifier, IdentifierError, Type, Value};
pub use values::{
    Color, Curve, CurvePoint, CurveValidationError, Gradient, GradientStop,
    GradientValidationError, MICROS_PER_SECOND, MarkWindow, Marks, SECONDS_PER_TICK,
    SampleDuration, SampleTime, SampleTimeError, mul_div, sample_duration_seconds_f32,
    sample_time_from_frame, sample_time_seconds_f32,
};
pub use word::InstructionWord;

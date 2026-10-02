#![no_std]
#![deny(unsafe_code)]
#![deny(unreachable_pub)]
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

// Admitted DSL programs bind their inputs before execution. Raw compiler data
// remains available for construction and serialization, never direct execution.
pub use dsl::bytecode::{
    ArithmeticOp, ArraySlot, BoolSlot, BytecodeProgram, ColorBinary, ColorComponent, ColorSlot,
    CompareOp, ConstantId, ContextRead, CurveSlot, EnumSlot, EnumSlotType, FloatBinary, FloatSlot,
    FloatUnary, GradientSlot, Instruction, IntArithmeticOp, IntSlot, LocalId, MarkOp, MarksSlot,
    NumberSlot, ParamId, ParameterKind, PoolSpan, SignalPixel, SlotLayout, Target, ValueSlot,
};
pub use dsl::types::IdentifierError;
pub use dsl::{
    BoundOperator, BoundParams, BoundSample, DslBindCache, Identifier, MAX_DSL_LOOP_ITERATIONS,
    OperatorProgram, OperatorRunContext, RunContext, RuntimeError, SampleProgram, SignalAccess,
    SignalSampler, SpatialContext, Type, Value, VmWorkspace,
};

// Playback owns both the accepted sequence and every buffer used to evaluate it.
pub use clip::{ClipSampler, SequenceClip};
pub use sequence::{
    FixtureFrame, OutputFrame, PreparedOutput, PreparedSequence, SequenceFrame, SequencePlayback,
};

// Host preparation constructs accepted data through owner-scoped handles.
pub use sequence::{
    EffectHandle, FixtureGeometry, FixtureHandle, LookupHandle, OperatorDefinition,
    OperatorInvocation, OutputEncoding, OutputHandle, RgbOrder, SampleDefinition, SampleInvocation,
    SequenceBuilder, SequenceRoot, SequenceTiming, SequenceWindow, SignalHandle, TargetHandle,
    TargetScope, WhitePosition, WindowHandle,
};

// Values shared with authoring and typed host construction.
pub use patch::PixelEncoding;
pub use signal::{PreparedAutomation, PreparedFixture};
pub use wire::{
    FORMAT_VERSION, HEADER_BYTES, LoadError, LoadLimits, decode_sequence, encode_sequence,
    payload_length,
};

// Shared authoring/evaluation values and the primitives used by host projections.
pub use automation::{
    AutomationMapping, AutomationValue, automation_value_at_position, curve_window_into,
};
pub use sampling::{deterministic_random, hsv, sample_curve};
pub use values::{
    Color, Curve, CurvePoint, CurveValidationError, Gradient, GradientStop,
    GradientValidationError, MICROS_PER_SECOND, Marks, SampleDuration, SampleTime, SampleTimeError,
    sample_duration_from_seconds_f32, sample_duration_seconds_f32, sample_time_from_frame,
    sample_time_from_seconds_f32, sample_time_seconds_f32, sample_time_with_seconds_offset,
};

mod automation;
mod clip;
mod dsl;
mod evaluation;
mod patch;
mod sampling;
mod sections;
mod sequence;
mod signal;
mod values;
mod wire;

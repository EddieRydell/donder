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
    ArithmeticOp, ArraySlot, BoolSlot, BytecodeProgram, CalculationRead, ColorBinary,
    ColorComponent, ColorSlot, CompareOp, ConstantId, ContextRead, CurveSlot, EnumSlot,
    EnumSlotType, FloatBinary, FloatSlot, FloatUnary, GradientSlot, Instruction, IntArithmeticOp,
    IntSlot, LocalId, MarkOp, MarksSlot, NumberSlot, ParamId, ParameterKind, PoolSpan, SignalPixel,
    SlotLayout, Target, TargetItemSlot, TargetItemsOp, TargetItemsSlot, TargetMember, TargetSlot,
    TargetSource, ValueSlot,
};
pub use dsl::generator::{
    BindingSlot, Block, BoundGenerator, Calculation, EmissionExecution, Expression,
    FixedBindingSlot, FixedCalculation, GeneratedEffectSlot, GeneratorBinding,
    GeneratorCalculation, GeneratorContext, GeneratorEmission, GeneratorInput, GeneratorInvocation,
    GeneratorProgram, GeneratorTarget, LinkedGenerator, LinkedSpecialization, SpecializedChild,
    SpecializedGenerator, Statement,
};
pub use dsl::types::IdentifierError;
pub use dsl::{
    BoundCalculation, BoundOperator, BoundParams, BoundSample, CalculationOutput,
    CalculationProgram, CompiledOperator, DslBindCache, Identifier, MAX_DSL_LOOP_ITERATIONS,
    OperatorInputDecl, OperatorProgram, OperatorRunContext, ParamDecl, RunContext, RuntimeError,
    SampleProgram, SignalAccess, SignalSampler, SpatialContext, TargetItemValue, TargetItemsValue,
    TargetValue, Type, Value, VmWorkspace,
};

// Playback owns both the accepted sequence and every buffer used to evaluate it.
pub use clip::{ClipSampler, SequenceClip};
pub use sequence::{
    FixtureFrame, OutputFrame, PreparedOutput, PreparedSequence, SequenceFrame, SequencePlayback,
};

// Host preparation constructs accepted data through owner-scoped handles.
pub use sequence::{
    EffectHandle, FixtureGeometry, FixtureHandle, GeneratedEffect, GeneratorPlayback, LookupHandle,
    OperatorDefinition, OperatorInvocation, OutputEncoding, OutputHandle, RgbOrder,
    SampleDefinition, SampleInvocation, SequenceBuilder, SequenceRoot, SequenceTiming,
    SequenceWindow, SignalHandle, TargetHandle, TargetScope, WhitePosition, WindowHandle,
};

// Raw data is inspectable and serializable, but must pass admission before playback.
pub use bindings::{
    ParameterSource, PreparedParameterBinding, PreparedParameterCalculation,
    PreparedParameterEnvironment,
};
pub use patch::{PixelEncoding, PreparedPatch, PreparedPixelRoute};
pub use signal::{
    PreparedAutomation, PreparedClip, PreparedEffect, PreparedEffectAutomation,
    PreparedEffectImplementation, PreparedFixture, PreparedLayer, PreparedOperator,
    PreparedOperatorNode, PreparedPixel, PreparedSignalGraph, PreparedSignalKind,
    PreparedSignalNode, PreparedTarget, SignalPlan,
};
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
    Color, Curve, CurvePoint, CurveValidationError, Distance, DistanceSpan, Gradient, GradientStop,
    GradientValidationError, MICROS_PER_SECOND, Marks, Point3, Rotation3, SampleDuration,
    SampleTime, SampleTimeError, Scale3, sample_duration_from_seconds_f32,
    sample_duration_seconds_f32, sample_time_from_frame, sample_time_from_seconds_f32,
    sample_time_seconds_f32, sample_time_with_seconds_offset,
};

mod automation;
mod bindings;
mod clip;
mod dsl;
mod evaluation;
mod patch;
mod sampling;
mod sequence;
mod signal;
mod values;
mod wire;

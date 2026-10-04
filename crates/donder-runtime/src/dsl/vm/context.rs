//! Execution capabilities for samples and signal operators.
use super::{RunContext, SpatialContext};
use crate::dsl::bytecode::ContextRead;
use crate::values::sample_duration_seconds_f32;

pub(super) enum Number {
    Int(i32),
    Float(f32),
}

/// Query and sequence time in seconds, converted once per invocation. On ESP32
/// each conversion is a software division; keeping it out of the interpreter
/// also prevents it from being hoisted into every pixel.
#[derive(Clone, Copy)]
pub(super) struct Clock {
    pub(super) seconds: f32,
    pub(super) duration: f32,
}

impl Clock {
    pub(super) fn new(context: &RunContext) -> Self {
        Self {
            seconds: sample_duration_seconds_f32(context.time),
            duration: sample_duration_seconds_f32(context.duration),
        }
    }
}

pub(super) trait ReadContext: Copy {
    type Spatial;
    fn read(self, context: &RunContext, clock: Clock, spatial: &Self::Spatial) -> Number;
}

impl ReadContext for ContextRead {
    type Spatial = SpatialContext;

    fn read(self, context: &RunContext, clock: Clock, spatial: &SpatialContext) -> Number {
        match self {
            Self::Progress => Number::Float(context.progress),
            Self::Seconds => Number::Float(clock.seconds),
            Self::Duration => Number::Float(clock.duration),
            Self::PixelIndex => Number::Int(context.pixel_index),
            Self::PixelCount => Number::Int(context.pixel_count),
            Self::PixelFraction => Number::Float(context.pixel_fraction),
            Self::PixelX => Number::Float(spatial.position[0]),
            Self::PixelY => Number::Float(spatial.position[1]),
            Self::TargetMinX => Number::Float(spatial.min[0]),
            Self::TargetMinY => Number::Float(spatial.min[1]),
            Self::TargetMaxX => Number::Float(spatial.max[0]),
            Self::TargetMaxY => Number::Float(spatial.max[1]),
        }
    }
}

/// Effects never query signals; sample admission rejects every signal
/// instruction, including trusted-wire conversion.
pub(crate) struct NoSignals;

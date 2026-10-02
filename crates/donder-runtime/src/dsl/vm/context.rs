//! Execution capabilities. Calculations have a clock but no pixel or signal
//! access; their capability error is uninhabited, not a discarded runtime error.
use super::{Color, RunContext, RuntimeError, SignalSampler, SpatialContext};
use crate::dsl::bytecode::{CalculationRead, ContextRead, SignalPixel};
use crate::values::{SampleTime, sample_duration_seconds_f32};
use core::convert::Infallible;

pub(super) enum Number {
    Int(i32),
    Float(f32),
}

pub(super) trait ReadContext: Copy {
    type Error;
    fn read(
        self,
        context: &RunContext,
        spatial: Option<&SpatialContext>,
    ) -> Result<Number, Self::Error>;
}

impl ReadContext for CalculationRead {
    type Error = Infallible;

    fn read(self, context: &RunContext, _: Option<&SpatialContext>) -> Result<Number, Infallible> {
        Ok(Number::Float(match self {
            Self::Progress => context.progress,
            Self::Seconds => sample_duration_seconds_f32(context.time),
            Self::Duration => sample_duration_seconds_f32(context.duration),
        }))
    }
}

impl ReadContext for ContextRead {
    type Error = RuntimeError;

    fn read(
        self,
        context: &RunContext,
        spatial: Option<&SpatialContext>,
    ) -> Result<Number, RuntimeError> {
        let spatial =
            || spatial.ok_or_else(|| RuntimeError::new("spatial sampling context is unavailable"));
        Ok(match self {
            Self::Progress => Number::Float(context.progress),
            Self::Seconds => Number::Float(sample_duration_seconds_f32(context.time)),
            Self::Duration => Number::Float(sample_duration_seconds_f32(context.duration)),
            Self::PixelIndex => Number::Int(context.pixel_index),
            Self::PixelCount => Number::Int(context.pixel_count),
            Self::PixelFraction => Number::Float(context.pixel_fraction),
            Self::PixelX => Number::Float(spatial()?.position[0]),
            Self::PixelY => Number::Float(spatial()?.position[1]),
            Self::TargetMinX => Number::Float(spatial()?.min[0]),
            Self::TargetMinY => Number::Float(spatial()?.min[1]),
            Self::TargetMaxX => Number::Float(spatial()?.max[0]),
            Self::TargetMaxY => Number::Float(spatial()?.max[1]),
        })
    }
}

pub(super) trait SampleSignal: Copy {
    type Error;
    fn sample(
        self,
        sampler: Option<&mut dyn SignalSampler>,
        input: usize,
        time: SampleTime,
        pixel: SignalPixel<i32>,
        cache: Option<usize>,
    ) -> Result<Color, Self::Error>;
}

impl SampleSignal for () {
    type Error = RuntimeError;

    fn sample(
        self,
        sampler: Option<&mut dyn SignalSampler>,
        input: usize,
        time: SampleTime,
        pixel: SignalPixel<i32>,
        cache: Option<usize>,
    ) -> Result<Color, RuntimeError> {
        sampler
            .ok_or_else(|| RuntimeError::new("Signal sampler is unavailable"))?
            .sample_signal(input, time, pixel, cache)
    }
}

impl SampleSignal for Infallible {
    type Error = Infallible;

    fn sample(
        self,
        _: Option<&mut dyn SignalSampler>,
        _: usize,
        _: SampleTime,
        _: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Result<Color, Infallible> {
        match self {}
    }
}

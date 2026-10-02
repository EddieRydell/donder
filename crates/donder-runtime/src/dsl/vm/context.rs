//! Execution capabilities for samples and signal operators.
use super::{Color, RunContext, SignalSampler, SpatialContext};
use crate::dsl::bytecode::{ContextRead, SignalPixel};
use crate::values::{SampleTime, sample_duration_seconds_f32};
use core::convert::Infallible;

pub(super) enum Number {
    Int(i32),
    Float(f32),
}

pub(super) trait ReadContext: Copy {
    type Spatial;
    fn read(self, context: &RunContext, spatial: &Self::Spatial) -> Number;
}

impl ReadContext for ContextRead {
    type Spatial = SpatialContext;

    fn read(self, context: &RunContext, spatial: &SpatialContext) -> Number {
        match self {
            Self::Progress => Number::Float(context.progress),
            Self::Seconds => Number::Float(sample_duration_seconds_f32(context.time)),
            Self::Duration => Number::Float(sample_duration_seconds_f32(context.duration)),
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

pub(super) trait SampleSignal<S> {
    type Error;
    fn sample(
        &mut self,
        capability: S,
        input: usize,
        time: SampleTime,
        pixel: SignalPixel<i32>,
        cache: Option<usize>,
    ) -> Result<Color, Self::Error>;
}

impl SampleSignal<Infallible> for () {
    type Error = Infallible;

    fn sample(
        &mut self,
        capability: Infallible,
        _: usize,
        _: SampleTime,
        _: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Result<Color, Infallible> {
        match capability {}
    }
}

impl<E> SampleSignal<crate::dsl::operator::SignalAccess> for &mut dyn SignalSampler<E> {
    type Error = E;

    fn sample(
        &mut self,
        _: crate::dsl::operator::SignalAccess,
        input: usize,
        time: SampleTime,
        pixel: SignalPixel<i32>,
        cache: Option<usize>,
    ) -> Result<Color, E> {
        self.sample_signal(input, time, pixel, cache)
    }
}

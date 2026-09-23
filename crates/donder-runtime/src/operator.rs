//! Native operator semantics shared by frame and pixel traversal.

use crate::BuiltinOperator;
use crate::dsl::BoundParams;
use crate::sampling::{
    add_colors, color_intensity, invert_color, max_colors, multiply_colors, scale_color,
};
use crate::signal::EvaluationError;
use crate::values::{Color, SampleDuration, SampleTime, sample_duration_from_seconds_f32};
use alloc::string::ToString;

#[derive(Clone, Copy)]
pub(crate) enum NativeOperator {
    Unary(UnaryOperator),
    Binary(BinaryOperator),
    Delay(SampleDuration),
    Echo(Echo),
}

#[derive(Clone, Copy)]
pub(crate) enum UnaryOperator {
    Dim(f32),
    Invert,
    Colorize(Color),
}

#[derive(Clone, Copy)]
pub(crate) enum BinaryOperator {
    Max,
    Add,
    Multiply,
    IntensityModulate,
}

#[derive(Clone, Copy)]
pub(crate) struct Echo {
    delay: SampleDuration,
    repeats: u32,
    decay: f32,
}

impl BuiltinOperator {
    pub const fn resamples_time(self) -> bool {
        matches!(self, Self::Delay | Self::Echo)
    }

    pub(crate) const fn scratch_frames(self) -> usize {
        if self.input_count() > 1 || matches!(self, Self::Echo) {
            1
        } else {
            0
        }
    }

    pub(crate) fn bind(self, params: &BoundParams) -> Result<NativeOperator, EvaluationError> {
        Ok(match self {
            Self::Max => NativeOperator::Binary(BinaryOperator::Max),
            Self::Add => NativeOperator::Binary(BinaryOperator::Add),
            Self::Multiply => NativeOperator::Binary(BinaryOperator::Multiply),
            Self::IntensityModulate => NativeOperator::Binary(BinaryOperator::IntensityModulate),
            Self::Dim => {
                NativeOperator::Unary(UnaryOperator::Dim(params.float(0)?.clamp(0.0, 1.0)))
            }
            Self::Invert => NativeOperator::Unary(UnaryOperator::Invert),
            Self::Colorize => NativeOperator::Unary(UnaryOperator::Colorize(params.color(0)?)),
            Self::Delay => NativeOperator::Delay(delay(params)?),
            Self::Echo => NativeOperator::Echo(Echo {
                delay: delay(params)?,
                repeats: params.int(1)?.clamp(1, 32) as u32,
                decay: params.float(2)?.clamp(0.0, 1.0),
            }),
        })
    }
}

impl UnaryOperator {
    #[inline]
    pub(crate) fn apply(self, color: Color) -> Color {
        match self {
            Self::Dim(amount) => scale_color(color, amount),
            Self::Invert => invert_color(color),
            Self::Colorize(tint) => scale_color(tint, color_intensity(color)),
        }
    }
}

impl BinaryOperator {
    #[inline]
    pub(crate) fn apply(self, a: Color, b: Color) -> Color {
        match self {
            Self::Max => max_colors(a, b),
            Self::Add => add_colors(a, b),
            Self::Multiply => multiply_colors(a, b),
            Self::IntensityModulate => scale_color(a, color_intensity(b)),
        }
    }
}

impl Echo {
    pub(crate) fn samples(self, time: SampleTime) -> impl Iterator<Item = (SampleTime, f32)> {
        (0..=self.repeats).filter_map(move |repeat| {
            let time = time.checked_sub_duration(SampleDuration::from_ticks(
                self.delay.as_ticks().saturating_mul(repeat),
            ))?;
            Some((time, powi_nonnegative(self.decay, repeat)))
        })
    }
}

fn delay(params: &BoundParams) -> Result<SampleDuration, EvaluationError> {
    sample_duration_from_seconds_f32(params.float(0)?.max(0.0)).map_err(|_| {
        EvaluationError::InvalidTiming {
            reason: "operator delay exceeds the runtime clock range".to_string(),
        }
    })
}

#[inline]
fn powi_nonnegative(mut base: f32, mut exponent: u32) -> f32 {
    let mut result = 1.0;
    while exponent != 0 {
        if exponent & 1 != 0 {
            result *= base;
        }
        base *= base;
        exponent >>= 1;
    }
    result
}

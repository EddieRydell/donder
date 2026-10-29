//! Playback speed maps elapsed wall time to show time and chooses the show
//! times that are rendered. All hosts share this integer math so desktop,
//! preview and devices agree on position and frame boundaries. Times are
//! 32-bit microseconds, as the show clock is.

use core::num::NonZeroU32;
use donder_runtime_types::{MICROS_PER_SECOND, mul_div};

const NORMAL_SHOW_MICROS_PER_SECOND: NonZeroU32 = match NonZeroU32::new(MICROS_PER_SECOND) {
    Some(value) => value,
    None => unreachable!(),
};

/// How the rendered frame grid responds to a playback speed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameTiming {
    /// Frames stay on the authored grid, so output frames per wall second scale with speed.
    Scaled,
    /// Output frames per wall second stay at the frame rate, sampling between authored frames.
    Constant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackRate {
    show_micros_per_second: NonZeroU32,
    frame_timing: FrameTiming,
}

impl PlaybackRate {
    pub const NORMAL: Self = Self {
        show_micros_per_second: NORMAL_SHOW_MICROS_PER_SECOND,
        frame_timing: FrameTiming::Scaled,
    };

    pub const fn new(show_micros_per_second: NonZeroU32, frame_timing: FrameTiming) -> Self {
        Self {
            show_micros_per_second,
            frame_timing,
        }
    }

    /// Show microseconds that elapse per wall second; 1_000_000 is normal speed.
    pub const fn show_micros_per_second(self) -> NonZeroU32 {
        self.show_micros_per_second
    }

    pub const fn frame_timing(self) -> FrameTiming {
        self.frame_timing
    }

    pub fn speed(self) -> f32 {
        self.show_micros_per_second.get() as f32 / MICROS_PER_SECOND as f32
    }

    /// Show microseconds covered by `wall` elapsed microseconds.
    pub fn show_elapsed(self, wall: u32) -> u32 {
        scale(wall, self.show_micros_per_second.get(), MICROS_PER_SECOND)
    }

    /// Wall microseconds needed to cover `show` microseconds, rounded up.
    pub fn wall_elapsed(self, show: u32) -> u32 {
        scale_ceil(show, MICROS_PER_SECOND, self.show_micros_per_second.get())
    }

    /// Index of the rendered frame containing show time `position`.
    pub fn frame_at(self, position: u32, frame_rate: u32) -> u32 {
        scale(position, frame_rate, self.frame_grid_divisor())
    }

    /// Show time at which rendered frame `frame` begins, rounded up.
    pub fn frame_start(self, frame: u32, frame_rate: u32) -> u32 {
        scale_ceil(frame, self.frame_grid_divisor(), frame_rate.max(1))
    }

    fn frame_grid_divisor(self) -> u32 {
        match self.frame_timing {
            FrameTiming::Scaled => MICROS_PER_SECOND,
            FrameTiming::Constant => self.show_micros_per_second.get(),
        }
    }
}

/// `value * numerator / denominator`, saturating.
fn scale(value: u32, numerator: u32, denominator: u32) -> u32 {
    mul_div(value, numerator, denominator).map_or(u32::MAX, |(quotient, _)| quotient)
}

/// `value * numerator / denominator` rounded up, saturating.
fn scale_ceil(value: u32, numerator: u32, denominator: u32) -> u32 {
    mul_div(value, numerator, denominator).map_or(u32::MAX, |(quotient, rest)| {
        quotient.saturating_add(u32::from(rest != 0))
    })
}

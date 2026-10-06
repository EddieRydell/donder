//! Playback speed maps elapsed wall time to show time and chooses the show
//! times that are rendered. All hosts share this integer math so desktop,
//! preview and devices agree on position and frame boundaries.

use core::num::NonZeroU32;

const MICROS_PER_SECOND: u64 = 1_000_000;
const NORMAL_SHOW_MICROS_PER_SECOND: NonZeroU32 = match NonZeroU32::new(1_000_000) {
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
    pub fn show_elapsed(self, wall: u64) -> u64 {
        scale(
            wall,
            u64::from(self.show_micros_per_second.get()),
            MICROS_PER_SECOND,
        )
    }

    /// Wall microseconds needed to cover `show` microseconds, rounded up.
    pub fn wall_elapsed(self, show: u64) -> u64 {
        scale_ceil(
            show,
            MICROS_PER_SECOND,
            u64::from(self.show_micros_per_second.get()),
        )
    }

    /// Index of the rendered frame containing show time `position`.
    pub fn frame_at(self, position: u64, frame_rate: u32) -> u64 {
        scale(position, u64::from(frame_rate), self.frame_grid_divisor())
    }

    /// Show time at which rendered frame `frame` begins, rounded up.
    pub fn frame_start(self, frame: u64, frame_rate: u32) -> u64 {
        scale_ceil(
            frame,
            self.frame_grid_divisor(),
            u64::from(frame_rate.max(1)),
        )
    }

    fn frame_grid_divisor(self) -> u64 {
        match self.frame_timing {
            FrameTiming::Scaled => MICROS_PER_SECOND,
            FrameTiming::Constant => u64::from(self.show_micros_per_second.get()),
        }
    }
}

fn scale(value: u64, numerator: u64, denominator: u64) -> u64 {
    (u128::from(value) * u128::from(numerator) / u128::from(denominator)).min(u128::from(u64::MAX))
        as u64
}

fn scale_ceil(value: u64, numerator: u64, denominator: u64) -> u64 {
    (u128::from(value) * u128::from(numerator))
        .div_ceil(u128::from(denominator))
        .min(u128::from(u64::MAX)) as u64
}

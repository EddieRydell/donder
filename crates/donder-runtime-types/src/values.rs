mod archive;
pub use archive::Microseconds;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
pub const MICROS_PER_SECOND: u32 = 1_000_000;
const MICROS_PER_SECOND_HZ: u64 = MICROS_PER_SECOND as u64;

/// The clock used by the portable renderer. One tick is one microsecond and all
/// arithmetic is 32-bit, matching the ESP32's native word size.
pub type SampleTime = fugit::MonotonicTimerInstantU32<MICROS_PER_SECOND_HZ>;
pub type SampleDuration = fugit::TimerDurationU32<MICROS_PER_SECOND_HZ>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleTimeError {
    InvalidFrameRate,
    NotFinite,
    Negative,
    OutOfRange,
}

pub fn sample_time_from_frame(frame: u32, frame_rate: u32) -> Result<SampleTime, SampleTimeError> {
    if frame_rate == 0 {
        return Err(SampleTimeError::InvalidFrameRate);
    }
    let whole_ticks = u64::from(frame / frame_rate) * u64::from(MICROS_PER_SECOND);
    let partial_ticks =
        u64::from(frame % frame_rate) * u64::from(MICROS_PER_SECOND) / u64::from(frame_rate);
    let ticks =
        u32::try_from(whole_ticks + partial_ticks).map_err(|_| SampleTimeError::OutOfRange)?;
    Ok(SampleTime::from_ticks(ticks))
}

/// Converts a desktop/audio API value at the boundary of the portable runtime.
pub fn sample_time_from_seconds_f32(seconds: f32) -> Result<SampleTime, SampleTimeError> {
    Ok(SampleTime::from_ticks(positive_seconds_to_ticks(seconds)?))
}

/// Converts a floating-point DSL duration at the VM boundary. Runtime state
/// keeps the resulting 32-bit microsecond value, not the source float.
pub fn sample_duration_from_seconds_f32(seconds: f32) -> Result<SampleDuration, SampleTimeError> {
    Ok(SampleDuration::from_ticks(positive_seconds_to_ticks(
        seconds,
    )?))
}

fn positive_seconds_to_ticks(seconds: f32) -> Result<u32, SampleTimeError> {
    if !seconds.is_finite() {
        return Err(SampleTimeError::NotFinite);
    }
    if seconds < 0.0 {
        return Err(SampleTimeError::Negative);
    }
    let bits = seconds.to_bits();
    let exponent = (bits >> 23) & 0xff;
    // Below 2^-21 seconds even the largest significand rounds to zero ticks.
    // At or above 2^13 seconds every value exceeds the 32-bit clock.
    if exponent <= 105 {
        return Ok(0);
    }
    if exponent >= 140 {
        return Err(SampleTimeError::OutOfRange);
    }
    // A normal f32 is significand * 2^(exponent - 150). Multiplying its
    // 24-bit significand by one million is exact in 44 bits; shifting with a
    // half-unit bias exactly preserves the previous round-to-nearest, ties-up
    // conversion without software double-precision arithmetic on the ESP32.
    let significand = (bits & 0x7f_ffff) | 0x80_0000;
    let micros = u64::from(significand) * u64::from(MICROS_PER_SECOND);
    let shift = 150 - exponent;
    let rounded = (micros + (1u64 << (shift - 1))) >> shift;
    u32::try_from(rounded).map_err(|_| SampleTimeError::OutOfRange)
}

/// Adds a possibly-negative floating-point DSL offset to the portable clock.
/// The conversion and arithmetic both stay within 32-bit tick values.
pub fn sample_time_with_seconds_offset(
    start: SampleTime,
    seconds: f32,
) -> Result<SampleTime, SampleTimeError> {
    if !seconds.is_finite() {
        return Err(SampleTimeError::NotFinite);
    }
    let offset = SampleDuration::from_ticks(positive_seconds_to_ticks(seconds.abs())?);
    if seconds.is_sign_negative() {
        start
            .checked_sub_duration(offset)
            .ok_or(SampleTimeError::OutOfRange)
    } else {
        start
            .checked_add_duration(offset)
            .ok_or(SampleTimeError::OutOfRange)
    }
}

#[cfg(test)]
mod clock_conversion_tests {
    use super::{
        SampleTime, SampleTimeError, sample_duration_from_seconds_f32, sample_time_from_frame,
        sample_time_from_seconds_f32, sample_time_with_seconds_offset,
    };

    #[test]
    fn frame_conversion_uses_wide_intermediates_but_keeps_a_32_bit_clock() {
        assert_eq!(
            sample_time_from_frame(4_500, 5_000).unwrap().as_ticks(),
            900_000
        );
        assert_eq!(
            sample_time_from_frame(u32::MAX, u32::MAX)
                .unwrap()
                .as_ticks(),
            1_000_000
        );
        assert_eq!(
            sample_time_from_frame(u32::MAX, 1),
            Err(SampleTimeError::OutOfRange)
        );
        assert_eq!(
            sample_time_from_frame(1, 0),
            Err(SampleTimeError::InvalidFrameRate)
        );
    }

    #[test]
    fn float_seconds_reject_values_that_round_beyond_the_last_tick() {
        let near_limit = (f64::from(u32::MAX) / 1_000_000.0) as f32;
        let over_limit = f32::from_bits(near_limit.to_bits() + 1);
        assert!(sample_time_from_seconds_f32(near_limit).is_ok());
        assert_eq!(
            sample_time_from_seconds_f32(over_limit),
            Err(SampleTimeError::OutOfRange)
        );
        assert_eq!(
            sample_duration_from_seconds_f32(over_limit),
            Err(SampleTimeError::OutOfRange)
        );
        assert_eq!(
            sample_time_with_seconds_offset(SampleTime::from_ticks(0), over_limit),
            Err(SampleTimeError::OutOfRange)
        );
    }

    #[test]
    fn integer_clock_conversion_matches_double_reference_at_rounding_boundaries() {
        let check = |seconds: f32| {
            let expected = if !seconds.is_finite() {
                Err(SampleTimeError::NotFinite)
            } else if seconds < 0.0 {
                Err(SampleTimeError::Negative)
            } else {
                let ticks = libm::round(f64::from(seconds) * 1_000_000.0);
                if ticks > f64::from(u32::MAX) {
                    Err(SampleTimeError::OutOfRange)
                } else {
                    Ok(SampleTime::from_ticks(ticks as u32))
                }
            };
            assert_eq!(
                sample_time_from_seconds_f32(seconds),
                expected,
                "{seconds:?}"
            );
        };
        for exponent in 0..256u32 {
            for fraction in [0, 1, 0x1f_ffff, 0x40_0000, 0x7f_fffe, 0x7f_ffff] {
                check(f32::from_bits((exponent << 23) | fraction));
                check(f32::from_bits(0x8000_0000 | (exponent << 23) | fraction));
            }
        }
        for tick in (0..u32::MAX).step_by(9973) {
            let midpoint = ((f64::from(tick) + 0.5) / 1_000_000.0) as f32;
            for bits in midpoint.to_bits().saturating_sub(1)..=midpoint.to_bits() + 1 {
                check(f32::from_bits(bits));
            }
        }
    }
}

/// Every tick count becomes seconds by one multiply: the ESP32's FPU
/// multiplies in hardware but divides in software.
pub const SECONDS_PER_TICK: f32 = 1.0 / MICROS_PER_SECOND as f32;

pub fn sample_time_seconds_f32(time: SampleTime) -> f32 {
    time.as_ticks() as f32 * SECONDS_PER_TICK
}

pub fn sample_duration_seconds_f32(duration: SampleDuration) -> f32 {
    duration.as_ticks() as f32 * SECONDS_PER_TICK
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Hash, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Color {
    pub const BLACK: Self = Self {
        red: 0,
        green: 0,
        blue: 0,
    };
    pub fn from_hex(value: &str) -> Option<Self> {
        if value.len() != 7 || !value.starts_with('#') {
            return None;
        }
        Some(Self {
            red: u8::from_str_radix(value.get(1..3)?, 16).ok()?,
            green: u8::from_str_radix(value.get(3..5)?, 16).ok()?,
            blue: u8::from_str_radix(value.get(5..7)?, 16).ok()?,
        })
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Curve {
    pub points: Vec<CurvePoint>,
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct CurvePoint {
    pub position: f32,
    pub value: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurveValidationError {
    NonFinitePoint,
    PositionOutOfRange,
    PositionsNotStrictlyIncreasing,
}

impl Curve {
    /// Bitwise equality of the points.
    pub fn same(&self, other: &Self) -> bool {
        self.points.len() == other.points.len()
            && self.points.iter().zip(&other.points).all(|(a, b)| {
                a.position.to_bits() == b.position.to_bits()
                    && a.value.to_bits() == b.value.to_bits()
            })
    }

    /// A hash consistent with [`Self::same`].
    pub fn hash_same<H: core::hash::Hasher>(&self, state: &mut H) {
        use core::hash::Hash;
        self.points.len().hash(state);
        for point in &self.points {
            point.position.to_bits().hash(state);
            point.value.to_bits().hash(state);
        }
    }

    /// Drop points that cannot affect sampling: exact repeats of the previous
    /// point and the interior of three or more points at one position. A step
    /// needs only the first and last point at its position.
    pub fn collapse_coincident_points(&mut self) {
        let mut points: Vec<CurvePoint> = Vec::with_capacity(self.points.len());
        for point in self.points.drain(..) {
            match points.as_mut_slice() {
                [.., previous] if previous == &point => {}
                [.., first, last]
                    if first.position == point.position && last.position == point.position =>
                {
                    *last = point;
                }
                _ => points.push(point),
            }
        }
        self.points = points;
    }

    pub fn validate(&self) -> Result<(), CurveValidationError> {
        let Some(first) = self.points.first() else {
            return Ok(());
        };
        if !first.position.is_finite() || !first.value.is_finite() {
            return Err(CurveValidationError::NonFinitePoint);
        }
        if !(0.0..=1.0).contains(&first.position) {
            return Err(CurveValidationError::PositionOutOfRange);
        }
        let mut previous = first.position;
        for point in self.points.iter().skip(1) {
            if !point.position.is_finite() || !point.value.is_finite() {
                return Err(CurveValidationError::NonFinitePoint);
            }
            if !(0.0..=1.0).contains(&point.position) {
                return Err(CurveValidationError::PositionOutOfRange);
            }
            if point.position < previous {
                return Err(CurveValidationError::PositionsNotStrictlyIncreasing);
            }
            previous = point.position;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Gradient {
    pub stops: Vec<GradientStop>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GradientValidationError {
    InvalidPosition,
    PositionsOutOfOrder,
}

impl Gradient {
    /// Bitwise equality of the stops.
    pub fn same(&self, other: &Self) -> bool {
        self.stops.len() == other.stops.len()
            && self
                .stops
                .iter()
                .zip(&other.stops)
                .all(|(a, b)| a.position.to_bits() == b.position.to_bits() && a.color == b.color)
    }

    /// A hash consistent with [`Self::same`].
    pub fn hash_same<H: core::hash::Hasher>(&self, state: &mut H) {
        use core::hash::Hash;
        self.stops.len().hash(state);
        for stop in &self.stops {
            stop.position.to_bits().hash(state);
            stop.color.hash(state);
        }
    }

    pub fn validate(&self) -> Result<(), GradientValidationError> {
        let mut previous = 0.0;
        for stop in &self.stops {
            if !stop.position.is_finite() || !(0.0..=1.0).contains(&stop.position) {
                return Err(GradientValidationError::InvalidPosition);
            }
            if stop.position < previous {
                return Err(GradientValidationError::PositionsOutOfOrder);
            }
            previous = stop.position;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct GradientStop {
    pub position: f32,
    pub color: Color,
}

/// Mark times in chronological order: a window of a shared track, measured
/// from the window's origin. Every value bound to one collection shares its
/// track, so a window costs a few words however many marks it holds.
/// Indices mean "nth mark in time", and lookups binary-search the track.
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Marks {
    /// Mark times in ticks, ascending.
    track: crate::Shared<[u32]>,
    /// The window, `track[first..end]`.
    first: u32,
    end: u32,
    /// The tick the window's mark times are measured from.
    origin: u32,
}

impl Marks {
    /// A collection's own times, measured from zero.
    pub fn new(marks: impl IntoIterator<Item = SampleDuration>) -> Self {
        let mut ticks: Vec<u32> = marks.into_iter().map(|mark| mark.as_ticks()).collect();
        ticks.sort_unstable();
        Self {
            end: ticks.len() as u32,
            track: ticks.into(),
            first: 0,
            origin: 0,
        }
    }

    pub fn empty() -> Self {
        Self::new([])
    }

    /// The marks of `track` (ascending ticks) in `start..end`, measured from
    /// `start`.
    pub fn window(track: crate::Shared<[u32]>, start: u32, end: u32) -> Self {
        let first = track.partition_point(|&mark| mark < start) as u32;
        let last = track.partition_point(|&mark| mark < end) as u32;
        Self {
            track,
            first,
            end: last.max(first),
            origin: start,
        }
    }

    /// The shared track.
    pub fn track(&self) -> &crate::Shared<[u32]> {
        &self.track
    }

    /// The same window over `track`, which holds the same times.
    pub fn with_track(&self, track: crate::Shared<[u32]>) -> Self {
        Self {
            track,
            ..self.clone()
        }
    }

    fn ticks(&self) -> &[u32] {
        &self.track[self.first as usize..self.end as usize]
    }

    pub fn len(&self) -> usize {
        (self.end - self.first) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.first == self.end
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = SampleDuration> + '_ {
        self.ticks()
            .iter()
            .map(|&mark| SampleDuration::from_ticks(mark - self.origin))
    }

    /// `sample_duration_seconds_f32` of mark `index`, or NaN for a missing
    /// index.
    pub fn seconds(&self, index: usize) -> f32 {
        self.ticks().get(index).map_or(f32::NAN, |&mark| {
            sample_duration_seconds_f32(SampleDuration::from_ticks(mark - self.origin))
        })
    }

    /// The last mark at or before `seconds`, with its time in seconds.
    /// Seconds rise with ticks, so those marks form a prefix: the search
    /// narrows by ticks near `seconds`, then settles the boundary by seconds,
    /// converting about twice. A NaN query matches no mark.
    pub fn previous(&self, seconds: f32) -> Option<(usize, f32)> {
        let ticks = self.ticks();
        // Saturating: NaN and negative queries start from the origin.
        let near = self
            .origin
            .saturating_add((seconds * MICROS_PER_SECOND as f32) as u32);
        let mut count = ticks.partition_point(|&mark| mark <= near);
        while count < ticks.len() && self.seconds(count) <= seconds {
            count += 1;
        }
        while count > 0 {
            let time = self.seconds(count - 1);
            if time <= seconds {
                return Some((count - 1, time));
            }
            count -= 1;
        }
        None
    }
}

/// Windows are equal when they hold the same times.
impl PartialEq for Marks {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl core::hash::Hash for Marks {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.len().hash(state);
        for mark in self.iter() {
            mark.as_ticks().hash(state);
        }
    }
}

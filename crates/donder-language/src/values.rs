pub mod archive;

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

pub fn sample_time_seconds_f32(time: SampleTime) -> f32 {
    time.as_ticks() as f32 / MICROS_PER_SECOND as f32
}

pub fn sample_duration_seconds_f32(duration: SampleDuration) -> f32 {
    duration.as_ticks() as f32 / MICROS_PER_SECOND as f32
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

/// Mark times in chronological order. Indices therefore mean "nth mark in
/// time", and lookups can binary-search instead of scanning the collection.
/// Each mark's seconds value is converted once here; on ESP32 every conversion
/// is a software division.
#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Marks {
    #[rkyv(with = rkyv::with::Map<crate::values::archive::Microseconds>)]
    marks: Vec<SampleDuration>,
    seconds: Vec<f32>,
}

impl Marks {
    pub const EMPTY: Self = Self {
        marks: Vec::new(),
        seconds: Vec::new(),
    };

    pub fn new(marks: impl IntoIterator<Item = SampleDuration>) -> Self {
        let mut marks: Vec<_> = marks.into_iter().collect();
        marks.sort_unstable();
        let seconds = marks
            .iter()
            .copied()
            .map(sample_duration_seconds_f32)
            .collect();
        Self { marks, seconds }
    }

    pub fn as_slice(&self) -> &[SampleDuration] {
        &self.marks
    }

    /// `sample_duration_seconds_f32` of each mark, in the same order.
    pub fn seconds(&self) -> &[f32] {
        &self.seconds
    }
}

impl core::hash::Hash for Marks {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.marks.len().hash(state);
        for mark in &self.marks {
            mark.as_ticks().hash(state);
        }
    }
}

use core::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecondsError {
    NotFinite,
    Negative,
    OutOfRange,
}

pub const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DonderTime(pub Duration);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DonderDuration(pub Duration);

impl DonderTime {
    pub fn try_from_seconds_f32(seconds: f32) -> Result<Self, SecondsError> {
        if !seconds.is_finite() {
            return Err(SecondsError::NotFinite);
        }
        if seconds < 0.0 {
            return Err(SecondsError::Negative);
        }
        Duration::try_from_secs_f32(seconds)
            .map(Self)
            .map_err(|_| SecondsError::OutOfRange)
    }

    pub const fn from_nanos(nanos: u64) -> Self {
        Self(Duration::from_nanos(nanos))
    }

    pub const fn from_micros(micros: u64) -> Self {
        Self(Duration::from_micros(micros))
    }

    pub fn as_nanos(&self) -> u128 {
        self.0.as_nanos()
    }

    pub fn as_micros_rounded(&self) -> u128 {
        (self.as_nanos() + 500) / 1_000
    }

    pub fn as_seconds_f32(&self) -> f32 {
        self.0.as_secs_f32()
    }
}

impl DonderDuration {
    pub fn try_from_seconds_f32(seconds: f32) -> Result<Self, SecondsError> {
        if !seconds.is_finite() {
            return Err(SecondsError::NotFinite);
        }
        if seconds < 0.0 {
            return Err(SecondsError::Negative);
        }
        Duration::try_from_secs_f32(seconds)
            .map(Self)
            .map_err(|_| SecondsError::OutOfRange)
    }

    pub const fn from_nanos(nanos: u64) -> Self {
        Self(Duration::from_nanos(nanos))
    }

    pub const fn from_micros(micros: u64) -> Self {
        Self(Duration::from_micros(micros))
    }

    pub fn as_nanos(&self) -> u128 {
        self.0.as_nanos()
    }

    pub fn as_micros_rounded(&self) -> u128 {
        (self.as_nanos() + 500) / 1_000
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn as_seconds_f32(&self) -> f32 {
        self.0.as_secs_f32()
    }
}

pub fn sample_time_from_donder_time(time: &DonderTime) -> Result<SampleTime, SampleTimeError> {
    Ok(SampleTime::from_ticks(
        u32::try_from(time.as_micros_rounded()).map_err(|_| SampleTimeError::OutOfRange)?,
    ))
}

pub fn sample_duration_from_donder_duration(
    duration: &DonderDuration,
) -> Result<SampleDuration, SampleTimeError> {
    Ok(SampleDuration::from_ticks(
        u32::try_from(duration.as_micros_rounded()).map_err(|_| SampleTimeError::OutOfRange)?,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distance {
    pub micrometers: i32,
}

impl Distance {
    pub const ZERO: Self = Self { micrometers: 0 };

    pub fn from_meters(value: f32) -> Self {
        Self {
            micrometers: libm::roundf(value * 1_000_000.0) as i32,
        }
    }

    pub fn as_meters_f32(self) -> f32 {
        self.micrometers as f32 / 1_000_000.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DistanceSpan {
    pub micrometers: u32,
}

impl DistanceSpan {
    pub const ZERO: Self = Self { micrometers: 0 };

    pub fn from_meters(value: f32) -> Self {
        Self {
            micrometers: libm::roundf(value * 1_000_000.0) as u32,
        }
    }

    pub fn as_meters_f32(self) -> f32 {
        self.micrometers as f32 / 1_000_000.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point3 {
    pub x: Distance,
    pub y: Distance,
    pub z: Distance,
}

impl Default for Point3 {
    fn default() -> Self {
        Self {
            x: Distance::ZERO,
            y: Distance::ZERO,
            z: Distance::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Default for Rotation3 {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Default for Scale3 {
    fn default() -> Self {
        Self {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        }
    }
}

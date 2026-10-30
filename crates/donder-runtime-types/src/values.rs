use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
pub const MICROS_PER_SECOND: u32 = 1_000_000;

/// A point on the show clock. One tick is one microsecond; the clock is
/// 32-bit, matching the ESP32's native word size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleTime(u32);

/// A span of the show clock, in microsecond ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleDuration(u32);

impl SampleTime {
    pub const fn from_ticks(ticks: u32) -> Self {
        Self(ticks)
    }

    pub const fn as_ticks(self) -> u32 {
        self.0
    }

    pub const fn checked_add_duration(self, duration: SampleDuration) -> Option<Self> {
        match self.0.checked_add(duration.0) {
            Some(ticks) => Some(Self(ticks)),
            None => None,
        }
    }
}

impl SampleDuration {
    pub const fn from_ticks(ticks: u32) -> Self {
        Self(ticks)
    }

    pub const fn as_ticks(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleTimeError {
    InvalidFrameRate,
    NotFinite,
    Negative,
    OutOfRange,
}

/// `a * b` as its high and low 32-bit halves.
#[inline]
const fn wide_mul(a: u32, b: u32) -> (u32, u32) {
    let (a1, a0) = (a >> 16, a & 0xffff);
    let (b1, b0) = (b >> 16, b & 0xffff);
    let low = a0 * b0;
    let (cross1, cross2) = (a0 * b1, a1 * b0);
    let middle = (low >> 16) + (cross1 & 0xffff) + (cross2 & 0xffff);
    let high = a1 * b1 + (cross1 >> 16) + (cross2 >> 16) + (middle >> 16);
    (high, (low & 0xffff) | (middle << 16))
}

/// `value * numerator / denominator` and its remainder, exactly, in 32-bit
/// arithmetic; `None` when the quotient does not fit or `denominator` is 0.
pub const fn mul_div(value: u32, numerator: u32, denominator: u32) -> Option<(u32, u32)> {
    let (mut high, mut low) = wide_mul(value, numerator);
    if denominator == 0 || high >= denominator {
        return None;
    }
    // Long division of high:low, one bit at a time; the remainder stays
    // below the denominator.
    let mut quotient = 0u32;
    let mut bit = 0;
    while bit < 32 {
        let carry = high >> 31;
        high = (high << 1) | (low >> 31);
        low <<= 1;
        quotient <<= 1;
        if carry != 0 || high >= denominator {
            high = high.wrapping_sub(denominator);
            quotient |= 1;
        }
        bit += 1;
    }
    Some((quotient, high))
}

pub fn sample_time_from_frame(frame: u32, frame_rate: u32) -> Result<SampleTime, SampleTimeError> {
    if frame_rate == 0 {
        return Err(SampleTimeError::InvalidFrameRate);
    }
    let (ticks, _) =
        mul_div(frame, MICROS_PER_SECOND, frame_rate).ok_or(SampleTimeError::OutOfRange)?;
    Ok(SampleTime::from_ticks(ticks))
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

/// Light: red, green and blue, where 0 is off and 1 is full brightness.
/// Values outside 0..1 and NaN pass through computation; output clamps them.
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Color {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

// Authored and folded colors are compared and hashed by value; programs never
// author NaN.
impl Eq for Color {}

impl core::hash::Hash for Color {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        [self.red, self.green, self.blue]
            .map(f32::to_bits)
            .hash(state);
    }
}

impl Color {
    pub const BLACK: Self = Self {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
    };

    pub fn from_hex(value: &str) -> Option<Self> {
        if value.len() != 7 || !value.starts_with('#') {
            return None;
        }
        Some(Self::from_bytes([
            u8::from_str_radix(value.get(1..3)?, 16).ok()?,
            u8::from_str_radix(value.get(3..5)?, 16).ok()?,
            u8::from_str_radix(value.get(5..7)?, 16).ok()?,
        ]))
    }

    pub fn to_hex(self) -> String {
        let [red, green, blue] = self.to_bytes();
        format!("#{red:02x}{green:02x}{blue:02x}")
    }

    /// The color of 8-bit channels.
    pub const fn from_bytes([red, green, blue]: [u8; 3]) -> Self {
        Self {
            red: red as f32 / 255.0,
            green: green as f32 / 255.0,
            blue: blue as f32 / 255.0,
        }
    }

    /// The nearest 8-bit channels. The cast saturates, so light below 0 or
    /// NaN is off and light above 1 is full.
    pub fn to_bytes(self) -> [u8; 3] {
        let byte = |channel: f32| (channel * 255.0 + 0.5) as u8;
        [byte(self.red), byte(self.green), byte(self.blue)]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub points: Vec<CurvePoint>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
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
    pub(crate) fn hash_same<H: core::hash::Hasher>(&self, state: &mut H) {
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

#[derive(Clone, Debug, PartialEq)]
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
    pub(crate) fn hash_same<H: core::hash::Hasher>(&self, state: &mut H) {
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

#[derive(Clone, Debug, PartialEq)]
pub struct GradientStop {
    pub position: f32,
    pub color: Color,
}

/// Mark times in chronological order: a window of a shared track, measured
/// from the window's origin. Every value bound to one collection shares its
/// track, so a window costs a few words however many marks it holds.
/// Indices mean "nth mark in time", and lookups binary-search the track.
#[derive(Clone, Debug)]
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

    /// The window, `track[first..end]`, and the tick it is measured from.
    pub fn bounds(&self) -> (u32, u32, u32) {
        (self.first, self.end, self.origin)
    }

    /// The shared track.
    pub fn track(&self) -> &crate::Shared<[u32]> {
        &self.track
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

    fn iter(&self) -> impl ExactSizeIterator<Item = SampleDuration> + '_ {
        self.ticks()
            .iter()
            .map(|&mark| SampleDuration::from_ticks(mark - self.origin))
    }

    /// The window as playback reads it.
    pub fn view(&self) -> MarkWindow<'_> {
        MarkWindow {
            ticks: self.ticks(),
            origin: self.origin,
        }
    }
}

/// Mark times in ascending ticks, measured from `origin`: a window of a track,
/// borrowed from an authored value or an image.
#[derive(Clone, Copy, Debug)]
pub struct MarkWindow<'a> {
    pub ticks: &'a [u32],
    pub origin: u32,
}

impl MarkWindow<'_> {
    pub fn len(&self) -> usize {
        self.ticks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ticks.is_empty()
    }

    /// `sample_duration_seconds_f32` of mark `index`, or NaN for a missing
    /// index.
    pub(crate) fn seconds(&self, index: usize) -> f32 {
        self.ticks.get(index).map_or(f32::NAN, |&mark| {
            sample_duration_seconds_f32(SampleDuration::from_ticks(mark - self.origin))
        })
    }

    /// The last mark at or before `seconds`, with its time in seconds.
    ///
    /// `cursor` holds the boundary the last query found. Queries a strip
    /// makes in turn are usually near one another, so the boundary is checked
    /// there before the track is searched.
    pub(crate) fn previous(
        &self,
        seconds: f32,
        cursor: &core::cell::Cell<usize>,
    ) -> Option<(usize, f32)> {
        let ticks = self.ticks;
        let at = self
            .origin
            .saturating_add((seconds * MICROS_PER_SECOND as f32) as u32);
        let hint = cursor.get();
        let fits = hint <= ticks.len()
            && (hint == 0 || ticks[hint - 1] <= at)
            && (hint == ticks.len() || ticks[hint] > at);
        let count = if fits {
            hint
        } else {
            ticks.partition_point(|&mark| mark <= at)
        };
        cursor.set(count);
        let index = count.checked_sub(1)?;
        Some((index, self.seconds(index)))
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

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

/// The color space a gradient interpolates in between its stops.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GradientSpace {
    /// Straight lines between the stops' channels.
    LinearRgb,
    /// Hue, saturation and value, with hue along the shorter arc.
    Hsv,
    /// Oklab: even lightness between stops.
    Oklab,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub space: GradientSpace,
    pub stops: Vec<GradientStop>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GradientValidationError {
    InvalidPosition,
    PositionsOutOfOrder,
}

impl Gradient {
    /// Bitwise equality of the space and stops.
    pub fn same(&self, other: &Self) -> bool {
        self.space == other.space
            && self.stops.len() == other.stops.len()
            && self
                .stops
                .iter()
                .zip(&other.stops)
                .all(|(a, b)| a.position.to_bits() == b.position.to_bits() && a.color == b.color)
    }

    /// A hash consistent with [`Self::same`].
    pub(crate) fn hash_same<H: core::hash::Hasher>(&self, state: &mut H) {
        use core::hash::Hash;
        self.space.hash(state);
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

/// Marks in chronological order: a window of a shared track, measured from
/// the window's origin. Every value bound to one collection shares its track,
/// so a window costs a few words however many marks it holds. Indices mean
/// "nth mark in time", and lookups binary-search the track.
#[derive(Clone, Debug)]
pub struct Marks {
    /// Mark times in ticks, ascending.
    track: crate::Shared<[u32]>,
    /// Each mark's duration in ticks, by the track's index.
    durations: crate::Shared<[u32]>,
    /// Every label's character codes, one after another.
    label_codes: crate::Shared<[u32]>,
    /// Each mark's label as a start and a length in `label_codes`, two
    /// words by the track's index; empty when no mark has a label.
    label_spans: crate::Shared<[u32]>,
    /// The window, `track[first..end]`.
    first: u32,
    end: u32,
    /// The tick the window's mark times are measured from.
    origin: u32,
}

impl Marks {
    /// A collection's own instantaneous marks, measured from zero.
    pub fn new(marks: impl IntoIterator<Item = SampleDuration>) -> Self {
        Self::spans(
            marks
                .into_iter()
                .map(|mark| (mark, SampleDuration::from_ticks(0))),
        )
    }

    /// A collection's own marks, each a start and a duration, measured from
    /// zero.
    pub fn spans(marks: impl IntoIterator<Item = (SampleDuration, SampleDuration)>) -> Self {
        let mut spans: Vec<(u32, u32)> = marks
            .into_iter()
            .map(|(start, duration)| (start.as_ticks(), duration.as_ticks()))
            .collect();
        spans.sort_unstable();
        let (track, durations): (Vec<u32>, Vec<u32>) = spans.into_iter().unzip();
        Self::window(
            MarkTrack {
                ticks: track.into(),
                durations: durations.into(),
                label_codes: crate::Shared::from([]),
                label_spans: crate::Shared::from([]),
            },
            0,
            u32::MAX,
        )
    }

    pub fn empty() -> Self {
        Self::new([])
    }

    /// The marks of `track` that start in `start..end`, measured from
    /// `start`.
    pub fn window(track: MarkTrack, start: u32, end: u32) -> Self {
        let MarkTrack {
            ticks,
            durations,
            label_codes,
            label_spans,
        } = track;
        debug_assert_eq!(ticks.len(), durations.len());
        let first = ticks.partition_point(|&mark| mark < start) as u32;
        let last = ticks.partition_point(|&mark| mark < end) as u32;
        Self {
            track: ticks,
            durations,
            label_codes,
            label_spans,
            first,
            end: last.max(first),
            origin: start,
        }
    }

    /// Every label's character codes, one after another.
    pub fn label_codes(&self) -> &crate::Shared<[u32]> {
        &self.label_codes
    }

    /// Each mark's label span in [`Self::label_codes`], by the track's index.
    pub fn label_spans(&self) -> &crate::Shared<[u32]> {
        &self.label_spans
    }

    /// Mark `index` of the window's label, as character codes.
    fn label(&self, index: usize) -> &[u32] {
        let at = 2 * (self.first as usize + index);
        match self.label_spans.get(at..at + 2) {
            Some(&[start, len]) => &self.label_codes[start as usize..(start + len) as usize],
            _ => &[],
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

    /// The shared durations, by the track's index.
    pub fn durations(&self) -> &crate::Shared<[u32]> {
        &self.durations
    }

    fn range(&self) -> core::ops::Range<usize> {
        self.first as usize..self.end as usize
    }

    pub fn len(&self) -> usize {
        (self.end - self.first) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.first == self.end
    }

    fn iter(&self) -> impl ExactSizeIterator<Item = (SampleDuration, SampleDuration)> + '_ {
        self.track[self.range()]
            .iter()
            .zip(&self.durations[self.range()])
            .map(|(&mark, &duration)| {
                (
                    SampleDuration::from_ticks(mark - self.origin),
                    SampleDuration::from_ticks(duration),
                )
            })
    }

    /// The window as playback reads it.
    pub fn view(&self) -> MarkWindow<'_> {
        let spans = if self.label_spans.is_empty() {
            &[][..]
        } else {
            &self.label_spans[2 * self.first as usize..2 * self.end as usize]
        };
        MarkWindow {
            ticks: &self.track[self.range()],
            durations: MarkDurations::Spans(&self.durations[self.range()]),
            labels: MarkLabels {
                codes: &self.label_codes,
                spans,
            },
            origin: self.origin,
        }
    }
}

/// A collection's marks, in time order: each one's tick, duration and label.
#[derive(Clone, Debug)]
pub struct MarkTrack {
    pub ticks: crate::Shared<[u32]>,
    pub durations: crate::Shared<[u32]>,
    /// Every label's character codes, one after another.
    pub label_codes: crate::Shared<[u32]>,
    /// Each mark's label as a start and a length in `label_codes`, two
    /// words per mark; empty when no mark has a label.
    pub label_spans: crate::Shared<[u32]>,
}

/// The labels of a window's marks: character codes, and a start and a
/// length into them for each mark, two words by the window's index. A window
/// without spans has no labels.
#[derive(Clone, Copy, Debug)]
pub struct MarkLabels<'a> {
    pub codes: &'a [u32],
    pub spans: &'a [u32],
}

/// How long the marks of a window last.
#[derive(Clone, Copy, Debug)]
pub enum MarkDurations<'a> {
    /// Ticks by the window's index.
    Spans(&'a [u32]),
    /// Every mark is instantaneous: a live collection's, or one with no
    /// lasting marks.
    Points,
}

/// Mark times in ascending ticks, measured from `origin`: a window of a track,
/// borrowed from an authored value or an image.
#[derive(Clone, Copy, Debug)]
pub struct MarkWindow<'a> {
    pub ticks: &'a [u32],
    pub durations: MarkDurations<'a>,
    pub labels: MarkLabels<'a>,
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

    /// Character `char` of mark `index`'s label, or 0 past its end, for a
    /// missing mark, or without labels.
    pub(crate) fn label_char(&self, index: i32, char: i32) -> i32 {
        let (Ok(index), Ok(char)) = (usize::try_from(index), u32::try_from(char)) else {
            return 0;
        };
        match self.labels.spans.get(2 * index..2 * index + 2) {
            Some(&[start, len]) if char < len => self.labels.codes[(start + char) as usize] as i32,
            _ => 0,
        }
    }

    /// How long mark `index` lasts in seconds, or NaN for a missing index.
    pub(crate) fn duration_seconds(&self, index: usize) -> f32 {
        if index >= self.ticks.len() {
            return f32::NAN;
        }
        match self.durations {
            MarkDurations::Spans(durations) => {
                sample_duration_seconds_f32(SampleDuration::from_ticks(durations[index]))
            }
            MarkDurations::Points => 0.0,
        }
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

/// Windows are equal when they hold the same times, durations and labels.
impl PartialEq for Marks {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self.iter().eq(other.iter())
            && (0..self.len()).all(|index| self.label(index) == other.label(index))
    }
}

impl core::hash::Hash for Marks {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.len().hash(state);
        for (index, (mark, duration)) in self.iter().enumerate() {
            mark.as_ticks().hash(state);
            duration.as_ticks().hash(state);
            self.label(index).hash(state);
        }
    }
}

/// An audio track's values over time, as a clip binds it: `channels` values
/// per sample (one for a level, a band each for a spectrum), `rate` samples
/// a second. Each value is a byte: 0..1, or -1..1 when `signed`. Every value
/// bound to one analysis shares its samples.
#[derive(Clone, Debug)]
pub struct Track {
    /// Channel `c` of sample `i` at `i * channels + c`.
    samples: crate::Shared<[u8]>,
    channels: u16,
    signed: bool,
    rate: u32,
    /// The tick of the track that the binding's time zero reads.
    origin: u32,
}

impl Track {
    pub fn new(samples: crate::Shared<[u8]>, channels: u16, signed: bool, rate: u32) -> Self {
        Self {
            samples,
            channels,
            signed,
            rate,
            origin: 0,
        }
    }

    /// A track with no channels: every read is silence.
    pub fn empty() -> Self {
        Self::new(crate::Shared::from([]), 0, false, 1)
    }

    /// The track read from tick `origin` of its own time.
    pub fn window(&self, origin: u32) -> Self {
        Self {
            origin,
            ..self.clone()
        }
    }

    pub fn samples(&self) -> &crate::Shared<[u8]> {
        &self.samples
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn signed(&self) -> bool {
        self.signed
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn origin(&self) -> u32 {
        self.origin
    }

    /// The track as playback reads it.
    pub fn view(&self) -> TrackView<'_> {
        TrackView {
            samples: &self.samples,
            channels: self.channels,
            signed: self.signed,
            rate: self.rate as f32,
            origin: sample_duration_seconds_f32(SampleDuration::from_ticks(self.origin)),
        }
    }
}

/// Tracks are equal when they read the same samples from the same tick.
impl PartialEq for Track {
    fn eq(&self, other: &Self) -> bool {
        (self.channels, self.signed, self.rate, self.origin)
            == (other.channels, other.signed, other.rate, other.origin)
            && (crate::Shared::ptr_eq(&self.samples, &other.samples)
                || self.samples == other.samples)
    }
}

/// Hashes the track's shape and a sparse sample of its bytes: equal tracks
/// hash alike without hashing every byte of a long song.
impl core::hash::Hash for Track {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        (self.channels, self.signed, self.rate, self.origin).hash(state);
        self.samples.len().hash(state);
        for byte in self.samples.iter().step_by(4096) {
            byte.hash(state);
        }
    }
}

/// A track's samples, borrowed from an authored value or an image, with
/// its origin in seconds.
#[derive(Clone, Copy, Debug)]
pub struct TrackView<'a> {
    pub samples: &'a [u8],
    pub channels: u16,
    pub signed: bool,
    pub rate: f32,
    pub origin: f32,
}

impl TrackView<'_> {
    /// Channel `channel` at `seconds` after the origin, interpolated between
    /// samples; silence outside the track or past its channels.
    pub(crate) fn at(&self, channel: i32, seconds: f32) -> f32 {
        let channels = usize::from(self.channels);
        if channels == 0 || !(0..i32::from(self.channels)).contains(&channel) {
            return 0.0;
        }
        let count = self.samples.len() / channels;
        let position = (self.origin + seconds) * self.rate;
        if position.is_nan() {
            return f32::NAN;
        }
        let last = count.saturating_sub(1) as f32;
        if !(0.0..=last).contains(&position) {
            return 0.0;
        }
        let index = position as usize;
        let fraction = position - index as f32;
        let at = |sample: usize| {
            let byte = f32::from(self.samples[sample.min(count - 1) * channels + channel as usize]);
            if self.signed {
                byte / 127.5 - 1.0
            } else {
                byte / 255.0
            }
        };
        let (a, b) = (at(index), at(index + 1));
        a + (b - a) * fraction
    }
}

/// One glyph of a font: its character, how far it moves the pen, and its
/// coverage bitmap of `width` by `height` bytes at `offset`, placed `left`
/// pixels right of the pen and `top` rows below the top of the line.
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Glyph {
    pub code: u32,
    pub advance: f32,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub offset: u32,
}

/// A font rasterized at one pixel height: its glyphs by ascending character
/// code, and their coverage bytes, 0 to 255.
#[derive(Clone, Debug)]
pub struct Font {
    /// The line's height in pixels, from its top to its bottom.
    pub height: f32,
    pub glyphs: crate::Shared<[Glyph]>,
    pub coverage: crate::Shared<[u8]>,
}

impl Font {
    /// A font with no glyphs: every character is blank and takes no space.
    pub fn empty() -> Self {
        Self {
            height: 0.0,
            glyphs: crate::Shared::from([]),
            coverage: crate::Shared::from([]),
        }
    }

    pub fn view(&self) -> FontView<'_> {
        FontView {
            height: self.height,
            glyphs: &self.glyphs,
            coverage: &self.coverage,
        }
    }
}

/// Fonts are equal when they draw alike.
impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.height.to_bits() == other.height.to_bits()
            && (crate::Shared::ptr_eq(&self.glyphs, &other.glyphs) || self.glyphs == other.glyphs)
            && (crate::Shared::ptr_eq(&self.coverage, &other.coverage)
                || self.coverage == other.coverage)
    }
}

impl core::hash::Hash for Font {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.height.to_bits().hash(state);
        self.glyphs.len().hash(state);
        for glyph in self.glyphs.iter() {
            (glyph.code, glyph.advance.to_bits()).hash(state);
        }
        self.coverage.len().hash(state);
    }
}

/// A font as playback reads it.
#[derive(Clone, Copy, Debug)]
pub struct FontView<'a> {
    pub height: f32,
    pub glyphs: &'a [Glyph],
    pub coverage: &'a [u8],
}

impl FontView<'_> {
    fn glyph(&self, code: i32) -> Option<&Glyph> {
        let code = u32::try_from(code).ok()?;
        let index = self
            .glyphs
            .binary_search_by_key(&code, |glyph| glyph.code)
            .ok()?;
        Some(&self.glyphs[index])
    }

    /// How far character `code` moves the pen, in pixels; 0 for a character
    /// the font lacks.
    pub(crate) fn advance(&self, code: i32) -> f32 {
        self.glyph(code).map_or(0.0, |glyph| glyph.advance)
    }

    /// Character `code`'s coverage, 0..1, at `x` pixels right of its pen and
    /// `y` up from the line's bottom, interpolated between its pixels.
    pub(crate) fn coverage(&self, code: i32, x: f32, y: f32) -> f32 {
        let Some(glyph) = self.glyph(code) else {
            return 0.0;
        };
        let column = x - glyph.left as f32 - 0.5;
        let row = (self.height - y) - glyph.top as f32 - 0.5;
        if !(column > -1.0 && row > -1.0) {
            return 0.0;
        }
        let (left, top) = (
            micromath::F32Ext::floor(column),
            micromath::F32Ext::floor(row),
        );
        let (across, down) = (column - left, row - top);
        let at = |c: f32, r: f32| {
            if c < 0.0 || r < 0.0 || c >= glyph.width as f32 || r >= glyph.height as f32 {
                return 0.0;
            }
            let index = glyph.offset as usize + r as usize * glyph.width as usize + c as usize;
            f32::from(self.coverage[index]) / 255.0
        };
        let top_row = at(left, top) + (at(left + 1.0, top) - at(left, top)) * across;
        let bottom_row =
            at(left, top + 1.0) + (at(left + 1.0, top + 1.0) - at(left, top + 1.0)) * across;
        top_row + (bottom_row - top_row) * down
    }
}

/// A picture, or the frames of an animation shown `rate` times a second:
/// `width` by `height` pixels of 8-bit red, green and blue, row by row from
/// the top, frame after frame.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    /// Frames a second; 0 for a still picture.
    pub rate: f32,
    pub pixels: crate::Shared<[u8]>,
}

impl Image {
    /// An image with no pixels: every point is black.
    pub fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            frames: 0,
            rate: 0.0,
            pixels: crate::Shared::from([]),
        }
    }

    pub fn view(&self) -> ImageView<'_> {
        ImageView {
            width: self.width,
            height: self.height,
            frames: self.frames,
            rate: self.rate,
            pixels: &self.pixels,
        }
    }
}

/// Images are equal when they show alike.
impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        (self.width, self.height, self.frames, self.rate.to_bits())
            == (
                other.width,
                other.height,
                other.frames,
                other.rate.to_bits(),
            )
            && (crate::Shared::ptr_eq(&self.pixels, &other.pixels) || self.pixels == other.pixels)
    }
}

impl core::hash::Hash for Image {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        (self.width, self.height, self.frames, self.rate.to_bits()).hash(state);
        self.pixels.len().hash(state);
    }
}

/// An image as playback reads it.
#[derive(Clone, Copy, Debug)]
pub struct ImageView<'a> {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub rate: f32,
    pub pixels: &'a [u8],
}

impl ImageView<'_> {
    /// Frame `frame`'s color at `x` pixels right of its left edge and `y`
    /// up from its bottom, interpolated between its pixels' centers and
    /// black outside it. Frames repeat: frame `frames` is frame 0 again.
    pub(crate) fn color(&self, frame: i32, x: f32, y: f32) -> Color {
        if self.frames == 0 {
            return Color::BLACK;
        }
        let frame = frame.rem_euclid(self.frames as i32) as usize;
        let column = x - 0.5;
        let row = self.height as f32 - y - 0.5;
        if !(column > -1.0 && row > -1.0) {
            return Color::BLACK;
        }
        let (left, top) = (
            micromath::F32Ext::floor(column),
            micromath::F32Ext::floor(row),
        );
        let (across, down) = (column - left, row - top);
        let (width, height) = (self.width as usize, self.height as usize);
        let base = frame * width * height * 3;
        let at = |c: f32, r: f32, channel: usize| {
            if c < 0.0 || r < 0.0 || c >= width as f32 || r >= height as f32 {
                return 0.0;
            }
            let index = base + (r as usize * width + c as usize) * 3 + channel;
            f32::from(self.pixels[index]) / 255.0
        };
        let channel = |channel: usize| {
            let top_row = at(left, top, channel)
                + (at(left + 1.0, top, channel) - at(left, top, channel)) * across;
            let bottom_row = at(left, top + 1.0, channel)
                + (at(left + 1.0, top + 1.0, channel) - at(left, top + 1.0, channel)) * across;
            top_row + (bottom_row - top_row) * down
        };
        Color {
            red: channel(0),
            green: channel(1),
            blue: channel(2),
        }
    }
}

//! The arithmetic programs run. Floats follow IEEE 754: NaN and infinities
//! propagate through the hardware, so nothing here tests for them.
use crate::values::{Color, Curve, CurvePoint, Gradient, GradientStop};

#[cfg(test)]
mod tests;

/// The left float unless it is NaN, otherwise the right. NaN is the missing
/// value: this is its one test.
#[inline(always)]
pub fn value_or(left: f32, right: f32) -> f32 {
    if left.is_nan() { right } else { left }
}

/// The smaller of two floats by one compare.
#[inline(always)]
pub fn min_float(left: f32, right: f32) -> f32 {
    if left < right { left } else { right }
}

/// The larger of two floats by one compare.
#[inline(always)]
pub fn max_float(left: f32, right: f32) -> f32 {
    if left > right { left } else { right }
}

#[inline(always)]
pub fn clamp_float(value: f32, min: f32, max: f32) -> f32 {
    min_float(max_float(value, min), max)
}

#[inline(always)]
pub fn int_binary(op: crate::bytecode::IntBinary, left: i32, right: i32) -> i32 {
    use crate::bytecode::IntBinary;
    match op {
        IntBinary::Add => left.wrapping_add(right),
        IntBinary::Subtract => left.wrapping_sub(right),
        IntBinary::Multiply => left.wrapping_mul(right),
        IntBinary::Remainder => int_remainder(left, right),
        IntBinary::FloorDivide => int_floor_divide(left, right),
        IntBinary::Min => left.min(right),
        IntBinary::Max => left.max(right),
    }
}

/// Floored quotient, consistent with [`int_remainder`]. Integer division by
/// zero traps in hardware, so it is zero.
pub fn int_floor_divide(value: i32, divisor: i32) -> i32 {
    if divisor == 0 {
        return 0;
    }
    let quotient = value.wrapping_div(divisor);
    if value.wrapping_rem(divisor) != 0 && (value < 0) != (divisor < 0) {
        quotient - 1
    } else {
        quotient
    }
}

/// Floored remainder: a nonzero result takes the divisor's sign. Remainder by
/// zero traps in hardware, so it is zero.
pub fn int_remainder(value: i32, divisor: i32) -> i32 {
    if divisor == 0 {
        return 0;
    }
    let remainder = value.wrapping_rem(divisor);
    if remainder != 0 && (remainder < 0) != (divisor < 0) {
        remainder + divisor
    } else {
        remainder
    }
}

/// Floored remainder: a nonzero result takes the divisor's sign.
#[inline(always)]
pub fn float_remainder(value: f32, divisor: f32) -> f32 {
    let remainder = value % divisor;
    if remainder != 0.0 && (remainder < 0.0) != (divisor < 0.0) {
        remainder + divisor
    } else {
        remainder
    }
}

#[inline(always)]
pub fn rgb(red: f32, green: f32, blue: f32) -> Color {
    Color { red, green, blue }
}

#[inline(always)]
pub fn sample_curve(curve: &Curve, position: f32) -> f32 {
    // An authored curve may be empty, which is missing; images never are.
    if curve.points.is_empty() {
        return f32::NAN;
    }
    sample_curve_points(&curve.points, position)
}

/// The slope of the segment from `start` to `finish`.
#[inline(always)]
pub fn segment_slope(start: &CurvePoint, finish: &CurvePoint) -> f32 {
    (finish.value - start.value) / (finish.position - start.position)
}

/// Each point's slope from the point before it, for [`sample_curve_sloped`];
/// the first point's is zero.
pub fn curve_slopes(points: &[CurvePoint], slopes: &mut [f32]) {
    slopes[0] = 0.0;
    for (index, slope) in slopes.iter_mut().enumerate().take(points.len()).skip(1) {
        *slope = segment_slope(&points[index - 1], &points[index]);
    }
}

/// A curve's value at `position`, holding its end values beyond its points.
/// A curve has at least one point.
#[inline(always)]
pub fn sample_curve_points(points: &[CurvePoint], position: f32) -> f32 {
    sample_curve_sloped(
        points,
        |index| segment_slope(&points[index - 1], &points[index]),
        position,
    )
}

/// [`sample_curve_points`] with each segment's slope given by the index of
/// its last point: a search, then a multiply-add.
#[inline(always)]
pub fn sample_curve_sloped(
    points: &[CurvePoint],
    slope: impl Fn(usize) -> f32,
    position: f32,
) -> f32 {
    let first = &points[0];
    if points.len() == 1 || position < first.position {
        return first.value;
    }
    let last = &points[points.len() - 1];
    if position >= last.position {
        return last.value;
    }
    let index = 1 + points[1..points.len() - 1].partition_point(|point| point.position <= position);
    let previous = &points[index - 1];
    previous.value + (position - previous.position) * slope(index)
}

/// The area under the curve from position 0 to `position`. End values are
/// held beyond the authored points, as in sampling.
pub fn curve_integral(points: &[CurvePoint], position: f32) -> f32 {
    curve_area(points, 0.0, position)
}

/// Each point's area from the first point, for [`curve_integral_table`].
/// Accumulated in the order [`curve_area`] adds segments, so the two agree.
pub fn curve_prefix_areas(points: &[CurvePoint], areas: &mut [f32]) {
    let mut area = 0.0;
    for (index, slot) in areas.iter_mut().enumerate().take(points.len()) {
        *slot = area;
        if let Some(finish) = points.get(index + 1) {
            let start = &points[index];
            area += (finish.position - start.position) * (start.value + finish.value) * 0.5;
        }
    }
}

/// [`curve_integral`] from the points' prefix areas: a search for the
/// segment, not a walk over the segments before it.
pub fn curve_integral_table(points: &[CurvePoint], areas: &[f32], position: f32) -> f32 {
    area_from_start_table(points, areas, position) - area_from_start_table(points, areas, 0.0)
}

fn area_from_start_table(points: &[CurvePoint], areas: &[f32], position: f32) -> f32 {
    let first = &points[0];
    if position <= first.position {
        return (position - first.position) * first.value;
    }
    let segment = points[1..].partition_point(|point| point.position <= position);
    if let Some(finish) = points.get(segment + 1) {
        let start = &points[segment];
        let fraction = (position - start.position) / (finish.position - start.position);
        let value = start.value + (finish.value - start.value) * fraction;
        return areas[segment] + (position - start.position) * (start.value + value) * 0.5;
    }
    let last = &points[points.len() - 1];
    areas[points.len() - 1] + (position - last.position) * last.value
}

/// The signed area under the curve between two positions, holding end values
/// beyond the authored points.
pub fn curve_area(points: &[CurvePoint], from: f32, to: f32) -> f32 {
    curve_area_from_start(points, to) - curve_area_from_start(points, from)
}

/// The signed area from the first point's position to `position`.
fn curve_area_from_start(points: &[CurvePoint], position: f32) -> f32 {
    let first = &points[0];
    if position <= first.position {
        return (position - first.position) * first.value;
    }
    let mut area = 0.0;
    for pair in points.windows(2) {
        let (start, finish) = (&pair[0], &pair[1]);
        if position < finish.position {
            let fraction = (position - start.position) / (finish.position - start.position);
            let value = start.value + (finish.value - start.value) * fraction;
            return area + (position - start.position) * (start.value + value) * 0.5;
        }
        area += (finish.position - start.position) * (start.value + finish.value) * 0.5;
    }
    let last = &points[points.len() - 1];
    area + (position - last.position) * last.value
}

/// The first position where the curve reaches `value`, or NaN; a flat
/// segment at `value` is reached at its start.
pub fn curve_first_crossing(points: &[CurvePoint], value: f32) -> f32 {
    if let [point] = points {
        return if point.value == value {
            point.position
        } else {
            f32::NAN
        };
    }
    for pair in points.windows(2) {
        let (start, finish) = (&pair[0], &pair[1]);
        if value >= start.value.min(finish.value) && value <= start.value.max(finish.value) {
            let span = finish.value - start.value;
            if span == 0.0 {
                return start.position;
            }
            return start.position
                + (finish.position - start.position) * (value - start.value) / span;
        }
    }
    f32::NAN
}

/// Latest arrival at `value`, including an exact touch, or NaN. A plateau
/// triggers on arrival, not continuously while held or again when leaving
/// it. Endpoint extension is a sampling rule and does not create crossing
/// events.
pub fn curve_last_crossing(points: &[CurvePoint], value: f32, before: f32) -> f32 {
    let end = points.partition_point(|point| point.position <= before);
    // Include the segment containing the query position, if there is one.
    for pair in points[..(end + 1).min(points.len())].windows(2).rev() {
        let (start, finish) = (&pair[0], &pair[1]);
        if value == start.value
            || value < start.value.min(finish.value)
            || value > start.value.max(finish.value)
        {
            continue;
        }
        // Equal-position points form a step. Only landing on the requested
        // value counts, not values skipped by the discontinuity.
        if start.position == finish.position && value != finish.value {
            continue;
        }
        let position = if value == finish.value {
            finish.position
        } else {
            start.position
                + (finish.position - start.position) * (value - start.value)
                    / (finish.value - start.value)
        };
        if position <= before {
            return position;
        }
    }
    let first = &points[0];
    if first.value == value && first.position <= before {
        first.position
    } else {
        f32::NAN
    }
}

#[inline]
pub fn sample_gradient(gradient: &Gradient, position: f32) -> Color {
    // An authored gradient may be empty, which is black; images never are.
    if gradient.stops.is_empty() {
        return Color::BLACK;
    }
    sample_gradient_stops(&gradient.stops, position)
}

/// A gradient stop as an image or an authored value holds it.
pub trait Stop: Sized {
    fn position(&self) -> f32;
    fn color(&self) -> Color;
    /// One over the width of the span from this stop to `next`.
    #[inline(always)]
    fn inverse(&self, next: &Self) -> f32 {
        1.0 / (next.position() - self.position())
    }
}

impl Stop for GradientStop {
    #[inline(always)]
    fn position(&self) -> f32 {
        self.position
    }
    #[inline(always)]
    fn color(&self) -> Color {
        self.color
    }
}

/// A gradient's color at `position`. Equal-position stops form a step: the
/// last stop wins at the exact position. A gradient has at least one stop.
pub fn sample_gradient_stops<S: Stop>(stops: &[S], position: f32) -> Color {
    let first = &stops[0];
    if stops.len() == 1 || position < first.position() {
        return first.color();
    }
    let last = &stops[stops.len() - 1];
    if position >= last.position() {
        return last.color();
    }
    // Gradients hold a few stops: a scan finds the span. It ends before the
    // last stop, which is past `position`.
    let mut index = 1;
    while stops[index].position() <= position {
        index += 1;
    }
    let previous = &stops[index - 1];
    let stop = &stops[index];
    mix_colors(
        previous.color(),
        stop.color(),
        (position - previous.position()) * previous.inverse(stop),
    )
}

#[inline(always)]
fn per_channel(a: Color, b: Color, f: impl Fn(f32, f32) -> f32) -> Color {
    Color {
        red: f(a.red, b.red),
        green: f(a.green, b.green),
        blue: f(a.blue, b.blue),
    }
}

/// `left` to `right` by `t`.
#[inline(always)]
pub fn mix_colors(left: Color, right: Color, t: f32) -> Color {
    per_channel(left, right, |a, b| a + (b - a) * t)
}

#[inline(always)]
pub fn scale_color(color: Color, scale: f32) -> Color {
    Color {
        red: color.red * scale,
        green: color.green * scale,
        blue: color.blue * scale,
    }
}

#[inline(always)]
pub fn add_colors(left: Color, right: Color) -> Color {
    per_channel(left, right, |a, b| a + b)
}

#[inline(always)]
pub fn multiply_colors(left: Color, right: Color) -> Color {
    per_channel(left, right, |a, b| a * b)
}

#[inline(always)]
pub fn max_colors(left: Color, right: Color) -> Color {
    per_channel(left, right, max_float)
}

#[inline(always)]
pub fn invert_color(color: Color) -> Color {
    Color {
        red: 1.0 - color.red,
        green: 1.0 - color.green,
        blue: 1.0 - color.blue,
    }
}

#[inline(always)]
fn brightest(color: Color) -> f32 {
    max_float(max_float(color.red, color.green), color.blue)
}

#[inline(always)]
fn dimmest(color: Color) -> f32 {
    min_float(min_float(color.red, color.green), color.blue)
}

#[inline(always)]
pub fn color_intensity(color: Color) -> f32 {
    brightest(color)
}

/// HSV hue in turns, in [0, 1). Achromatic colors have hue zero.
#[inline]
pub fn color_hue(color: Color) -> f32 {
    let (max, min) = (brightest(color), dimmest(color));
    if max == min {
        return 0.0;
    }
    let chroma = max - min;
    let sector = if max == color.red {
        (color.green - color.blue) / chroma
    } else if max == color.green {
        (color.blue - color.red) / chroma + 2.0
    } else {
        (color.red - color.green) / chroma + 4.0
    };
    let hue = sector * (1.0 / 6.0);
    if hue < 0.0 { hue + 1.0 } else { hue }
}

/// HSV saturation in [0, 1]. Black has saturation zero.
#[inline]
pub fn color_saturation(color: Color) -> f32 {
    let (max, min) = (brightest(color), dimmest(color));
    if max == 0.0 { 0.0 } else { (max - min) / max }
}

/// `color` with its hue replaced by `hue`, or shifted by it when `shift`,
/// keeping its saturation and intensity: the same chroma over the same
/// dimmest channel.
pub fn recolor(color: Color, hue: f32, shift: bool) -> Color {
    let (max, min) = (brightest(color), dimmest(color));
    let hue = if shift { color_hue(color) + hue } else { hue };
    hue_color(hue, max - min, min)
}

/// The color of `hue` with `chroma` above the dimmest channel `base`.
fn hue_color(hue: f32, chroma: f32, base: f32) -> Color {
    let sector = (hue - libm::floorf(hue)) * 6.0;
    let whole = sector as i32;
    let rise = sector - whole as f32;
    let (up, down) = (chroma * rise, chroma * (1.0 - rise));
    let (red, green, blue) = match whole {
        0 => (chroma, up, 0.0),
        1 => (down, chroma, 0.0),
        2 => (0.0, chroma, up),
        3 => (0.0, down, chroma),
        4 => (up, 0.0, chroma),
        _ => (chroma, 0.0, down),
    };
    rgb(red + base, green + base, blue + base)
}

/// HSV from hue in turns, and saturation and value.
#[inline]
pub fn hsv(h: f32, s: f32, v: f32) -> Color {
    let chroma = v * s;
    hue_color(h, chroma, v - chroma)
}

#[inline(always)]
pub fn deterministic_random(values: impl Iterator<Item = f32>) -> f32 {
    let seed = values.fold(0.0, |seed, value| seed * 31.0 + value);
    deterministic_random_seed(seed)
}

/// MurmurHash3's 32-bit avalanche finalizer over the seed's bits: stateless,
/// allocation-free and 32-bit.
#[inline(always)]
pub fn deterministic_random_seed(seed: f32) -> f32 {
    let mut value = seed.to_bits();
    value ^= value >> 16;
    value = value.wrapping_mul(0x85eb_ca6b);
    value ^= value >> 13;
    value = value.wrapping_mul(0xc2b2_ae35);
    value ^= value >> 16;
    // The upper 24 bits convert exactly to f32 and cannot round up to 1.
    (value >> 8) as f32 * (1.0 / 16_777_216.0)
}

#[inline(always)]
pub fn float_unary(op: crate::bytecode::FloatUnary, value: f32) -> f32 {
    use crate::bytecode::FloatUnary;
    match op {
        FloatUnary::Negate => -value,
        FloatUnary::Smoothstep => smoothstep(value),
        FloatUnary::Rand => deterministic_random_seed(value),
        FloatUnary::Sin => micromath::F32Ext::sin(value),
        FloatUnary::Cos => micromath::F32Ext::cos(value),
        FloatUnary::Tan => tan(value),
        FloatUnary::Exp => exp(value),
        FloatUnary::Log => log(value),
        FloatUnary::Abs => f32::abs(value),
        FloatUnary::Floor => libm::floorf(value),
        FloatUnary::Ceil => libm::ceilf(value),
        FloatUnary::Trunc => libm::truncf(value),
        FloatUnary::Sqrt => libm::sqrtf(value),
    }
}

pub fn float_binary(op: crate::bytecode::FloatBinary, left: f32, right: f32) -> f32 {
    use crate::bytecode::FloatBinary;
    match op {
        FloatBinary::Add => left + right,
        FloatBinary::Subtract => left - right,
        FloatBinary::Multiply => left * right,
        FloatBinary::Divide => left / right,
        FloatBinary::Remainder => float_remainder(left, right),
        FloatBinary::ValueOr => value_or(left, right),
        FloatBinary::Min => min_float(left, right),
        FloatBinary::Max => max_float(left, right),
        FloatBinary::Atan2 => libm::atan2f(left, right),
        FloatBinary::Power => power(left, right),
    }
}

// Large math routines stay out of line, so firmware keeps them in flash
// rather than copying them into every caller in instruction RAM.
// f32 throughout: libm's `tanf` computes in double precision.
#[inline(never)]
fn tan(value: f32) -> f32 {
    micromath::F32Ext::tan(value)
}

#[inline(never)]
fn exp(value: f32) -> f32 {
    libm::expf(value)
}

#[inline(never)]
fn log(value: f32) -> f32 {
    libm::logf(value)
}

#[inline(never)]
fn power(base: f32, exponent: f32) -> f32 {
    libm::powf(base, exponent)
}

/// Cubic interpolation of a position clamped to 0..1.
#[inline(always)]
pub fn smoothstep(value: f32) -> f32 {
    let t = clamp_float(value, 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Position within a section of `width` pixels, given the width's reciprocal.
#[inline(always)]
pub fn section_position(pixel_index: i32, width: f32, inverse: f32) -> f32 {
    let index = pixel_index as f32;
    (index - libm::floorf(index * inverse) * width) * inverse
}

#[inline(always)]
pub fn gradient_color_scaled<S: Stop>(stops: &[S], position: f32, scale: f32) -> Color {
    scale_color(
        sample_gradient_stops(stops, position),
        clamp_float(scale, 0.0, 1.0),
    )
}

/// Time of the mark at `index`, or NaN for a missing index.
pub fn mark_at(marks: &crate::values::MarkWindow<'_>, index: i32) -> f32 {
    marks.seconds(index as usize)
}

/// The last mark at or before `seconds`: its index and its time in seconds.
#[inline(never)]
pub fn previous_mark(
    marks: &crate::values::MarkWindow<'_>,
    seconds: f32,
    cursor: &core::cell::Cell<usize>,
) -> Option<(usize, f32)> {
    marks.previous(seconds, cursor)
}

#[inline(never)]
pub fn previous_mark_index(
    marks: &crate::values::MarkWindow<'_>,
    seconds: f32,
    cursor: &core::cell::Cell<usize>,
) -> i32 {
    previous_mark(marks, seconds, cursor).map_or(-1, |(index, _)| index as i32)
}

/// Array indices clamp to the first or last element of a nonempty array.
#[inline(always)]
pub fn clamp_array_index(index: i32, nonempty_length: usize) -> usize {
    (index.max(0) as usize).min(nonempty_length - 1)
}

use crate::values::{Color, Curve, CurvePoint, Gradient, GradientStop};

#[cfg(test)]
mod tests;

#[inline(always)]
pub fn clamp_float(value: f32, min: f32, max: f32) -> f32 {
    if min.is_nan() || max.is_nan() || min > max {
        f32::NAN
    } else {
        value.clamp(min, max)
    }
}

/// Floored remainder: a nonzero result takes the divisor's sign. Remainder by
/// zero and `i32::MIN % -1` are zero.
#[inline(always)]
pub fn int_remainder(value: i32, divisor: i32) -> i32 {
    let remainder = value.checked_rem(divisor).unwrap_or(0);
    if remainder != 0 && (remainder < 0) != (divisor < 0) {
        remainder + divisor
    } else {
        remainder
    }
}

/// Floored remainder, within `[0, divisor)` for a positive divisor. Remainder
/// by zero is NaN.
#[inline(always)]
pub fn float_remainder(value: f32, divisor: f32) -> f32 {
    let remainder = value % divisor;
    if remainder != 0.0 && (remainder < 0.0) != (divisor < 0.0) {
        // A tiny negative remainder can round onto the divisor itself.
        let wrapped = remainder + divisor;
        if wrapped == divisor { 0.0 } else { wrapped }
    } else {
        remainder
    }
}

/// Round a byte-domain value to a channel, saturating to 0..=255; NaN becomes 0.
/// Equal to `(value.clamp(0.0, 255.0) + 0.5) as u8` for every f32, but clamps
/// the converted integer, which avoids float compares on Xtensa.
#[inline(always)]
pub fn byte_channel(value: f32) -> u8 {
    ((value + 0.5) as i32).clamp(0, 255) as u8
}

#[inline(always)]
pub fn rgb(red: f32, green: f32, blue: f32) -> Color {
    if red.is_nan() || green.is_nan() || blue.is_nan() {
        Color::BLACK
    } else {
        let channel = |value: f32| byte_channel(value * 255.0);
        Color {
            red: channel(red),
            green: channel(green),
            blue: channel(blue),
        }
    }
}

#[inline(always)]
pub fn sample_curve(curve: &Curve, position: f32) -> f32 {
    sample_curve_points(&curve.points, position)
}

#[inline(always)]
pub fn sample_curve_points(points: &[CurvePoint], position: f32) -> f32 {
    if position.is_nan() {
        return f32::NAN;
    }
    let Some(first) = points.first() else {
        return f32::NAN;
    };
    if points.len() == 1 {
        return first.value;
    }
    if position < first.position {
        return first.value;
    }
    let last = &points[points.len() - 1];
    if position >= last.position {
        return last.value;
    }
    let index = 1 + points[1..points.len() - 1].partition_point(|point| point.position <= position);
    let previous = &points[index - 1];
    let point = &points[index];
    let span = (point.position - previous.position).max(1e-9);
    let t = unit_span_fraction(position - previous.position, span).clamp(0.0, 1.0);
    previous.value + (point.value - previous.value) * t
}

#[inline]
pub fn curve_crossing(curve: &Curve, value: f32, fallback: f32) -> f32 {
    let Some(first) = curve.points.first() else {
        return fallback;
    };
    let mut previous = first;
    for point in &curve.points {
        let min = previous.value.min(point.value);
        let max = previous.value.max(point.value);
        if value >= min && value <= max {
            let span = point.value - previous.value;
            if span.abs() <= 1e-9 {
                return previous.position;
            }
            let t = if span.is_finite() {
                unit_span_fraction(value - previous.value, span)
            } else {
                ((f64::from(value) - f64::from(previous.value))
                    / (f64::from(point.value) - f64::from(previous.value))) as f32
            }
            .clamp(0.0, 1.0);
            return previous.position + (point.position - previous.position) * t;
        }
        previous = point;
    }
    fallback
}

/// Latest arrival at `value`, including an exact touch. A plateau triggers on
/// arrival, not continuously while held or again when leaving it. Endpoint
/// extension is a sampling rule and does not create crossing events.
pub fn curve_last_crossing(curve: &Curve, value: f32, before: f32) -> f32 {
    if value.is_nan() || before.is_nan() {
        return f32::NAN;
    }
    let points = &curve.points;
    let end = points.partition_point(|point| point.position <= before);
    // Include the segment containing the query position, if there is one.
    for pair in points[..end.saturating_add(1).min(points.len())]
        .windows(2)
        .rev()
    {
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
            let span = finish.value - start.value;
            let fraction = if span.is_finite() {
                (value - start.value) / span
            } else {
                // Finite opposite-sign endpoints can overflow a f32 difference.
                ((f64::from(value) - f64::from(start.value))
                    / (f64::from(finish.value) - f64::from(start.value))) as f32
            };
            start.position + (finish.position - start.position) * fraction
        };
        if position <= before {
            return position;
        }
    }
    points
        .first()
        .filter(|point| point.value == value && point.position <= before)
        .map_or(f32::NAN, |point| point.position)
}

#[inline]
pub fn sample_gradient(gradient: &Gradient, position: f32) -> Color {
    sample_gradient_stops(&gradient.stops, position)
}

/// Equal-position stops form a step: the last stop wins at the exact position.
#[inline]
pub fn sample_gradient_stops(stops: &[GradientStop], position: f32) -> Color {
    if position.is_nan() {
        return Color::BLACK;
    }
    let Some(first) = stops.first() else {
        return Color::BLACK;
    };
    if stops.len() == 1 {
        return first.color;
    }
    if position < first.position {
        return first.color;
    }
    let last = &stops[stops.len() - 1];
    if position >= last.position {
        return last.color;
    }
    let index = 1 + stops[1..stops.len() - 1].partition_point(|stop| stop.position <= position);
    let previous = &stops[index - 1];
    let stop = &stops[index];
    let span = (stop.position - previous.position).max(1e-9);
    mix_colors(
        previous.color,
        stop.color,
        unit_span_fraction(position - previous.position, span).clamp(0.0, 1.0),
    )
}

#[inline(always)]
fn unit_span_fraction(numerator: f32, span: f32) -> f32 {
    if span == 1.0 {
        numerator
    } else if span == -1.0 {
        -numerator
    } else {
        numerator / span
    }
}

#[inline(always)]
pub fn mix_colors(left: Color, right: Color, t: f32) -> Color {
    if t.is_nan() {
        return Color::BLACK;
    }
    let channel =
        |left: u8, right: u8| byte_channel(left as f32 + (right as f32 - left as f32) * t);
    Color {
        red: channel(left.red, right.red),
        green: channel(left.green, right.green),
        blue: channel(left.blue, right.blue),
    }
}

#[inline(always)]
pub fn scale_color(color: Color, scale: f32) -> Color {
    if scale.is_nan() {
        return Color::BLACK;
    }
    let channel = |value: u8| byte_channel(value as f32 * scale);
    Color {
        red: channel(color.red),
        green: channel(color.green),
        blue: channel(color.blue),
    }
}

#[inline(always)]
pub fn add_colors(left: Color, right: Color) -> Color {
    Color {
        red: left.red.saturating_add(right.red),
        green: left.green.saturating_add(right.green),
        blue: left.blue.saturating_add(right.blue),
    }
}

#[inline(always)]
pub fn multiply_colors(left: Color, right: Color) -> Color {
    let channel = |a: u8, b: u8| ((u16::from(a) * u16::from(b) + 127) / 255) as u8;
    Color {
        red: channel(left.red, right.red),
        green: channel(left.green, right.green),
        blue: channel(left.blue, right.blue),
    }
}

#[inline(always)]
pub fn max_colors(left: Color, right: Color) -> Color {
    Color {
        red: left.red.max(right.red),
        green: left.green.max(right.green),
        blue: left.blue.max(right.blue),
    }
}

#[inline(always)]
pub fn invert_color(color: Color) -> Color {
    Color {
        red: 255 - color.red,
        green: 255 - color.green,
        blue: 255 - color.blue,
    }
}

#[inline(always)]
pub fn color_intensity(color: Color) -> f32 {
    f32::from(color.red.max(color.green).max(color.blue)) * (1.0 / 255.0)
}

// RGB components bound both HSV divisors to 1..=255. Store these constants in
// read-only program data, rather than performing software division per pixel.
// This has no initialization, heap allocation, or retained frame state.
const CHANNEL_RECIPROCALS: [f32; 256] = {
    let mut values = [0.0; 256];
    let mut channel = 1;
    while channel < values.len() {
        values[channel] = 1.0 / channel as f32;
        channel += 1;
    }
    values
};

/// HSV hue in turns, in [0, 1). Achromatic colors have hue zero.
#[inline]
pub fn color_hue(color: Color) -> f32 {
    let max = color.red.max(color.green).max(color.blue);
    let min = color.red.min(color.green).min(color.blue);
    hue_between(color, max, min)
}

fn hue_between(color: Color, max: u8, min: u8) -> f32 {
    let chroma = max - min;
    if chroma == 0 {
        return 0.0;
    }
    let inverse = CHANNEL_RECIPROCALS[usize::from(chroma)];
    let difference = |left: u8, right: u8| f32::from(i16::from(left) - i16::from(right));
    let sector = if max == color.red {
        difference(color.green, color.blue) * inverse
    } else if max == color.green {
        difference(color.blue, color.red) * inverse + 2.0
    } else {
        difference(color.red, color.green) * inverse + 4.0
    };
    let hue = sector * (1.0 / 6.0);
    if hue < 0.0 { hue + 1.0 } else { hue }
}

/// HSV saturation in [0, 1]. Black and grayscale have saturation zero.
#[inline]
pub fn color_saturation(color: Color) -> f32 {
    let max = color.red.max(color.green).max(color.blue);
    let min = color.red.min(color.green).min(color.blue);
    saturation_between(max, min)
}

fn saturation_between(max: u8, min: u8) -> f32 {
    if max == 0 {
        0.0
    } else {
        f32::from(max - min) * CHANNEL_RECIPROCALS[usize::from(max)]
    }
}

/// `color` with its hue replaced by `hue`, or shifted by it when `shift`:
/// `hsv(hue, saturation(color), intensity(color))`, or the same with
/// `hue(color) + hue`, computing the color's components once.
pub fn recolor(color: Color, hue: f32, shift: bool) -> Color {
    let max = color.red.max(color.green).max(color.blue);
    let min = color.red.min(color.green).min(color.blue);
    let hue = if shift {
        hue_between(color, max, min) + hue
    } else {
        hue
    };
    hsv(
        hue,
        saturation_between(max, min),
        f32::from(max) * (1.0 / 255.0),
    )
}

#[inline]
pub fn hsv(h: f32, s: f32, v: f32) -> Color {
    if h.is_nan() || s.is_nan() || v.is_nan() {
        return Color::BLACK;
    }
    let h = h - libm::floorf(h);
    let sector = h * 6.0;
    let c = v * s;
    let x = c * (1.0 - (sector - libm::floorf(sector / 2.0) * 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if sector < 1.0 {
        (c, x, 0.0)
    } else if sector < 2.0 {
        (x, c, 0.0)
    } else if sector < 3.0 {
        (0.0, c, x)
    } else if sector < 4.0 {
        (0.0, x, c)
    } else if sector < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let channel = |value: f32| byte_channel(value * 255.0);
    Color {
        red: channel(r + m),
        green: channel(g + m),
        blue: channel(b + m),
    }
}

#[inline(always)]
pub fn deterministic_random(values: impl Iterator<Item = f32>) -> f32 {
    let seed = values.fold(0.0, |seed, value| seed * 31.0 + value);
    deterministic_random_seed(seed)
}

#[inline(always)]
pub fn deterministic_random_seed(seed: f32) -> f32 {
    if seed.is_nan() {
        return f32::NAN;
    }
    // MurmurHash3's 32-bit avalanche finalizer. Hash the seed representation,
    // not its sine: this is stateless, allocation-free and uses no doubles.
    // Normalize signed zero so numerically equal zero seeds agree.
    let mut value = if seed == 0.0 { 0 } else { seed.to_bits() };
    value ^= value >> 16;
    value = value.wrapping_mul(0x85eb_ca6b);
    value ^= value >> 13;
    value = value.wrapping_mul(0xc2b2_ae35);
    value ^= value >> 16;
    // The upper 24 bits convert exactly to f32 and cannot round up to 1.
    (value >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Unary math propagates NaN before its implementation sees it; negation,
/// smoothstep and random seeds take NaN as it is.
#[inline(always)]
pub fn float_unary(op: crate::dsl::bytecode::FloatUnary, value: f32) -> f32 {
    use crate::dsl::bytecode::FloatUnary;
    let math = |function: fn(f32) -> f32| {
        if value.is_nan() {
            f32::NAN
        } else {
            function(value)
        }
    };
    match op {
        FloatUnary::Negate => -value,
        FloatUnary::Smoothstep => smoothstep(value),
        FloatUnary::Rand => deterministic_random_seed(value),
        FloatUnary::Sin => math(micromath::F32Ext::sin),
        FloatUnary::Cos => math(micromath::F32Ext::cos),
        FloatUnary::Abs => math(f32::abs),
        FloatUnary::Floor => math(libm::floorf),
        FloatUnary::Ceil => math(libm::ceilf),
        FloatUnary::Trunc => math(libm::truncf),
        FloatUnary::RoundEven => math(libm::roundevenf),
        FloatUnary::Sqrt => math(libm::sqrtf),
    }
}

pub fn float_binary(op: crate::dsl::bytecode::FloatBinary, left: f32, right: f32) -> f32 {
    use crate::dsl::bytecode::FloatBinary;
    match op {
        FloatBinary::Add => left + right,
        FloatBinary::Subtract => left - right,
        FloatBinary::Multiply => left * right,
        FloatBinary::Divide => left / right,
        FloatBinary::Remainder => float_remainder(left, right),
        FloatBinary::ValueOr => {
            if left.is_nan() {
                right
            } else {
                left
            }
        }
        FloatBinary::Min | FloatBinary::Max if left.is_nan() || right.is_nan() => f32::NAN,
        FloatBinary::Min => left.min(right),
        FloatBinary::Max => left.max(right),
        FloatBinary::Atan2 => libm::atan2f(left, right),
    }
}

/// Clamped cubic interpolation of an already normalized position.
#[inline(always)]
pub fn smoothstep(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A source query inside the effect duration, in seconds; NaN outside it.
pub fn query_seconds(seconds: f32, duration: crate::values::SampleDuration) -> f32 {
    crate::values::sample_time_from_seconds_f32(seconds)
        .ok()
        .filter(|time| time.as_ticks() < duration.as_ticks())
        .map_or(f32::NAN, crate::values::sample_time_seconds_f32)
}

pub fn query_progress(seconds: f32, duration: crate::values::SampleDuration) -> f32 {
    let duration = duration.as_ticks();
    crate::values::sample_time_from_seconds_f32(seconds)
        .ok()
        .filter(|time| time.as_ticks() < duration)
        .map_or(f32::NAN, |time| {
            (time.as_ticks() as f32 / duration as f32).clamp(0.0, 1.0)
        })
}

/// Position within a section of `width` pixels, given the width's reciprocal.
#[inline(always)]
pub fn section_position(pixel_index: i32, width: f32, inverse: f32) -> f32 {
    let index = pixel_index as f32;
    (index - libm::floorf(index * inverse) * width) * inverse
}

#[inline(always)]
pub fn gradient_color_scaled(gradient: &Gradient, position: f32, scale: f32) -> Color {
    let scale = scale.clamp(0.0, 1.0);
    if scale <= 0.0 {
        Color::BLACK
    } else {
        scale_color(sample_gradient(gradient, position), scale)
    }
}

/// Time of the mark at `index`, or NaN for a missing index.
pub fn mark_at(marks: &crate::values::Marks, index: i32) -> f32 {
    usize::try_from(index)
        .ok()
        .and_then(|index| marks.seconds().get(index))
        .copied()
        .unwrap_or(f32::NAN)
}

/// Marks are chronological and seconds conversion is monotonic, so the marks
/// at or before `seconds` form a prefix. A NaN query matches no mark.
/// Returns the mark's index and its time in seconds.
pub fn previous_mark(marks: &crate::values::Marks, seconds: f32) -> Option<(usize, f32)> {
    let times = marks.seconds();
    let index = times
        .partition_point(|&mark| mark <= seconds)
        .checked_sub(1)?;
    Some((index, times[index]))
}

pub fn previous_mark_index(marks: &crate::values::Marks, seconds: f32) -> i32 {
    previous_mark(marks, seconds)
        .map(|(index, _)| length_int(index))
        .unwrap_or(-1)
}

#[inline(always)]
pub fn length_int(length: usize) -> i32 {
    i32::try_from(length).unwrap_or(i32::MAX)
}

/// Array indices clamp to the first or last element of a nonempty array.
#[inline(always)]
pub fn clamp_array_index(index: i32, nonempty_length: usize) -> usize {
    (index.max(0) as usize).min(nonempty_length - 1)
}

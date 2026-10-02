use crate::values::{Color, Curve, CurvePoint, Gradient, GradientStop};

#[cfg(test)]
mod tests;

#[inline(always)]
pub fn sample_curve(curve: &Curve, position: f32) -> f32 {
    sample_curve_points(&curve.points, position)
}

#[inline(always)]
pub(crate) fn sample_curve_points(points: &[CurvePoint], position: f32) -> f32 {
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
pub(crate) fn curve_crossing(curve: &Curve, value: f32, fallback: f32) -> f32 {
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
pub(crate) fn curve_last_crossing(curve: &Curve, value: f32, before: f32) -> f32 {
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
pub(crate) fn sample_gradient(gradient: &Gradient, position: f32) -> Color {
    sample_gradient_stops(&gradient.stops, position)
}

/// Equal-position stops form a step: the last stop wins at the exact position.
#[inline]
pub(crate) fn sample_gradient_stops(stops: &[GradientStop], position: f32) -> Color {
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
pub(crate) fn mix_colors(left: Color, right: Color, t: f32) -> Color {
    if t.is_nan() {
        return Color::BLACK;
    }
    let channel = |left: u8, right: u8| {
        ((left as f32 + (right as f32 - left as f32) * t).clamp(0.0, 255.0) + 0.5) as u8
    };
    Color {
        red: channel(left.red, right.red),
        green: channel(left.green, right.green),
        blue: channel(left.blue, right.blue),
    }
}

#[inline(always)]
pub(crate) fn scale_color(color: Color, scale: f32) -> Color {
    if scale.is_nan() {
        return Color::BLACK;
    }
    let channel = |value: u8| ((value as f32 * scale).clamp(0.0, 255.0) + 0.5) as u8;
    Color {
        red: channel(color.red),
        green: channel(color.green),
        blue: channel(color.blue),
    }
}

#[inline(always)]
pub(crate) fn add_colors(left: Color, right: Color) -> Color {
    Color {
        red: left.red.saturating_add(right.red),
        green: left.green.saturating_add(right.green),
        blue: left.blue.saturating_add(right.blue),
    }
}

#[inline(always)]
pub(crate) fn multiply_colors(left: Color, right: Color) -> Color {
    let channel = |a: u8, b: u8| ((u16::from(a) * u16::from(b) + 127) / 255) as u8;
    Color {
        red: channel(left.red, right.red),
        green: channel(left.green, right.green),
        blue: channel(left.blue, right.blue),
    }
}

#[inline(always)]
pub(crate) fn max_colors(left: Color, right: Color) -> Color {
    Color {
        red: left.red.max(right.red),
        green: left.green.max(right.green),
        blue: left.blue.max(right.blue),
    }
}

#[inline(always)]
pub(crate) fn invert_color(color: Color) -> Color {
    Color {
        red: 255 - color.red,
        green: 255 - color.green,
        blue: 255 - color.blue,
    }
}

#[inline(always)]
pub(crate) fn color_intensity(color: Color) -> f32 {
    f32::from(color.red.max(color.green).max(color.blue)) / 255.0
}

/// HSV hue in turns, in [0, 1). Achromatic colors have hue zero.
#[inline]
pub(crate) fn color_hue(color: Color) -> f32 {
    let r = f32::from(color.red);
    let g = f32::from(color.green);
    let b = f32::from(color.blue);
    let max = r.max(g).max(b);
    let chroma = max - r.min(g).min(b);
    if chroma == 0.0 {
        return 0.0;
    }
    let sector = if max == r {
        (g - b) / chroma
    } else if max == g {
        (b - r) / chroma + 2.0
    } else {
        (r - g) / chroma + 4.0
    };
    let hue = sector / 6.0;
    if hue < 0.0 { hue + 1.0 } else { hue }
}

/// HSV saturation in [0, 1]. Black and grayscale have saturation zero.
#[inline]
pub(crate) fn color_saturation(color: Color) -> f32 {
    let max = color.red.max(color.green).max(color.blue);
    let min = color.red.min(color.green).min(color.blue);
    if max == 0 {
        0.0
    } else {
        f32::from(max - min) / f32::from(max)
    }
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
    let channel = |value: f32| ((value * 255.0).clamp(0.0, 255.0) + 0.5) as u8;
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
pub(crate) fn deterministic_random_seed(seed: f32) -> f32 {
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

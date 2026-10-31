//! Lights zigzagging between a star's inner and outer points. Vixen:
//! `PreviewStar`.

use super::ellipse::{at, inscribed, turn_degrees};
use super::single::own_pixels;
use super::{Laid, ShapePixel, ShapeSkip, unrotated};
use crate::vixen::contract::{self, FieldError};
use crate::xml::Element;
use glam::DVec2;

/// Whether the lights run counter-clockwise. Vixen: `StringDirections`.
fn counter_clockwise(shape: &Element) -> Result<bool, ShapeSkip> {
    const FIELD: &str = "_stringDirection";
    match contract::text(shape, FIELD).map(str::trim) {
        None | Some("Clockwise") => Ok(false),
        Some("CounterClockwise") => Ok(true),
        Some(other) => Err(ShapeSkip::Field(FieldError {
            field: FIELD.to_string(),
            value: other.to_string(),
        })),
    }
}

/// Vixen: `PreviewStar.Layout`, `RecalcPoints`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    let mut nodes = own_pixels(shape)?;
    let (left, top) = contract::point(shape, "_topLeftPoint")?;
    let (right, bottom) = contract::point(shape, "_bottomRightPoint")?;
    let point_count = contract::int(shape, "_pointCount", 0)?;
    let pixel_count = contract::int(shape, "_pixelCount", 0)?;
    let inside_size = contract::int(shape, "_insideSize", 0)?;
    // The serializer leaves a field it does not find at zero, not at its
    // initializer's 18.
    let xy_rotation = contract::int(shape, "_XYRotation", 0)?;
    let counter_clockwise = counter_clockwise(shape)?;

    let outer_width = right.wrapping_sub(left);
    let outer_height = bottom.wrapping_sub(top);
    if outer_width.wrapping_abs() <= 10 || outer_height.wrapping_abs() <= 10 {
        return match nodes.len() {
            0 => Ok(Laid::default()),
            _ => Err(ShapeSkip::Unplaceable(
                "a star no more than 10 units wide or high",
            )),
        };
    }
    if point_count < 1 {
        return Err(ShapeSkip::Unplaceable("a star of no points"));
    }
    if pixel_count < 0 {
        // Vixen would add lights up to its two per point from there.
        return Err(ShapeSkip::Unplaceable("a star of a negative light count"));
    }

    // RecalcPoints: an even number of lights per point, at least two,
    // removing lights from the end or adding unlinked ones to match.
    let mut per_point = pixel_count / point_count;
    while per_point % 2 != 0 {
        per_point -= 1;
    }
    per_point = per_point.max(2);
    let Some(new_count) = per_point.checked_mul(point_count) else {
        return Err(ShapeSkip::Unplaceable(
            "a star of more lights than Vixen counts",
        ));
    };
    if pixel_count > new_count {
        let removed = (i64::from(pixel_count) - i64::from(new_count)) as usize;
        if removed > nodes.len() {
            // Vixen removes past its first light.
            return Err(ShapeSkip::Unplaceable(
                "a star with fewer lights than its light count",
            ));
        }
        nodes.truncate(nodes.len() - removed);
    } else {
        let added = (i64::from(new_count) - i64::from(pixel_count)) as usize;
        nodes.resize(nodes.len() + added, None);
    }
    let new_count = new_count as usize;
    if nodes.len() > new_count {
        return Err(ShapeSkip::Unplaceable(
            "a star with more lights than its light count",
        ));
    }

    let points = point_count as usize;
    // Vixen sizes and places the inner points' box in whole units; that
    // is kept, as it shapes the star rather than snapping a light.
    let inner_size = |outer: i32| {
        let inner = (f64::from(outer.wrapping_mul(inside_size)) * 0.01) as i32;
        inner.max(4)
    };
    let inner_width = inner_size(outer_width);
    let inner_height = inner_size(outer_height);
    let inner_left_top = DVec2::new(
        f64::from(left.wrapping_add(outer_width.wrapping_sub(inner_width) / 2)),
        f64::from(top.wrapping_add(outer_height.wrapping_sub(inner_height) / 2)),
    );
    let outer_ellipse = inscribed(
        DVec2::new(f64::from(left), f64::from(top)),
        DVec2::new(f64::from(outer_width), f64::from(outer_height)),
    );
    let inner_ellipse = inscribed(
        inner_left_top,
        DVec2::new(f64::from(inner_width), f64::from(inner_height)),
    );
    let outer_offset = f64::from(360 / point_count / 2) + f64::from(xy_rotation);
    let inner_offset = f64::from(xy_rotation);
    let outer_at = |index| at(&outer_ellipse, turn_degrees(points, outer_offset, index));
    let inner_at = |index| at(&inner_ellipse, turn_degrees(points, inner_offset, index));

    // Each point's lights run from an inner point out to the outer point
    // and back to the next inner point, half on each line, none on the
    // line's end. Vixen rounds each light to whole units; this does not.
    let half = (per_point / 2) as usize;
    let mut positions = Vec::with_capacity(new_count);
    for point in 0..points {
        let (first, tip, next) = if counter_clockwise {
            let index = points - point;
            (inner_at(index), outer_at(index - 1), inner_at(index - 1))
        } else {
            let next = if point < points - 1 { point + 1 } else { 0 };
            (inner_at(point), outer_at(point), inner_at(next))
        };
        for (from, to) in [(first, tip), (tip, next)] {
            for step in 0..half {
                positions.push(from + (to - from) * (step as f64 / half as f64));
            }
        }
    }

    Ok(Laid {
        pixels: nodes
            .into_iter()
            .zip(positions)
            .map(|(node, position)| ShapePixel {
                node,
                position,
                arc: None,
            })
            .collect(),
        ellipses: Vec::new(),
    })
}

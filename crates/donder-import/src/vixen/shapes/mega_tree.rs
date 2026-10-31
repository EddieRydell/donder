//! A mega tree: straight strings from points around a base ellipse up to
//! points around a smaller top ellipse. Vixen: `PreviewMegaTree`.

use super::{Laid, ShapePixel, ShapeSkip, pixel_nodes, strings, unrotated};
use crate::vixen::contract;
use crate::xml::Element;
use glam::DVec2;
use std::f64::consts::PI;

/// The points Vixen steps around the ellipse in the box `width` by
/// `height` at `left`, `top`, from `degree_offset` through `degrees` more,
/// stopping once it has `wanted` of them; without truncating each to whole
/// units. Vixen: `PreviewTools.GetEllipsePoints`.
#[allow(clippy::too_many_arguments)]
fn ellipse_points(
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    total_points: f64,
    degrees: f64,
    degree_offset: f64,
    wanted: usize,
) -> Result<Vec<DVec2>, ShapeSkip> {
    let total_radians = (degrees * PI) / 180.0;
    let point_count = total_points / 2.0;
    let center_x = width / 2.0;
    let center_y = height / 2.0;
    let start = (degree_offset * PI) / 180.0;
    let end = total_radians + start;
    let increment = if degrees <= 180.0 {
        PI / (point_count - 1.0)
    } else {
        (PI * 2.0) / total_points
    };
    let mut points = Vec::new();
    // The same accumulated angle as Vixen's loop, whose floating-point adds
    // decide how many points there are.
    let mut t = start;
    while points.len() < wanted && t < end + increment / 10.0 {
        let next = t + increment;
        if next <= t {
            // Vixen's loop never ends.
            return Err(ShapeSkip::Unplaceable(
                "a mega tree whose string angles never advance",
            ));
        }
        points.push(DVec2::new(
            (center_x + (width / 2.0) * t.cos()) + left,
            (center_y + (height / 2.0) * t.sin()) + top,
        ));
        t = next;
    }
    Ok(points)
}

/// Each string from its point on the base ellipse to its point on the top
/// ellipse, the last string at the first points, as the tree's
/// `OnDeserialized` lays them out after each string's own; each string's
/// lights evenly spaced along it from the base. The ellipse points and the
/// string ends are not truncated or rounded to whole units; the top
/// ellipse's offset keeps Vixen's whole-unit halving of the widths, which
/// decides where the top sits rather than snapping a light. Vixen:
/// `PreviewMegaTree.OnDeserialized`, `Layout`, `Pixels`;
/// `PreviewTools.TransformPreviewPoint`; `PreviewLine.Layout`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    let (left, top) = contract::point(shape, "_topLeft")?;
    let (right, bottom) = contract::point(shape, "_bottomRight")?;
    let string_count = contract::int(shape, "_stringCount", 0)?;
    let top_height = contract::int(shape, "_topHeight", 0)?;
    let top_width = contract::int(shape, "_topWidth", 0)?;
    let base_height = contract::int(shape, "_baseHeight", 0)?;
    let degrees = contract::int(shape, "_degrees", 0)?;
    let degree_offset = contract::int(shape, "_degreeOffset", 0)?;
    let lines = strings(shape);
    // Vixen lays out the first `_stringCount` strings by index, and its
    // `Pixels` lists every string's lights.
    let count = match usize::try_from(string_count) {
        Ok(count) if count == lines.len() => count,
        Ok(count) if count > lines.len() => {
            return Err(ShapeSkip::Unplaceable(
                "a mega tree with fewer strings than its string count",
            ));
        }
        _ => return Err(ShapeSkip::Setting("strings beyond its string count")),
    };
    if count == 0 {
        return Ok(Laid::default());
    }

    // Vixen's int arithmetic, which wraps.
    let width = right.wrapping_sub(left);
    let top_left_offset = left.wrapping_add(width / 2).wrapping_sub(top_width / 2);
    let bottom_top_offset = bottom.wrapping_sub(base_height);
    let total_points = ((360.0 / f64::from(degrees)) * f64::from(string_count)).ceil();
    let top_points = ellipse_points(
        f64::from(top_left_offset),
        f64::from(top),
        f64::from(top_width),
        f64::from(top_height),
        total_points,
        f64::from(degrees),
        f64::from(degree_offset),
        count,
    )?;
    let base_points = ellipse_points(
        f64::from(left),
        f64::from(bottom_top_offset),
        f64::from(width),
        f64::from(base_height),
        total_points,
        f64::from(degrees),
        f64::from(degree_offset),
        count,
    )?;
    if top_points.len() < count || base_points.len() < count {
        // Vixen indexes past the ellipse's points, or leaves every string
        // where it was saved.
        return Err(ShapeSkip::Unplaceable(
            "a mega tree whose degrees give fewer string angles than strings",
        ));
    }

    let mut pixels = Vec::new();
    // String `k` takes the ellipses' points at `count - 1 - k`.
    for ((line, top_end), base_end) in lines
        .into_iter()
        .zip(top_points.into_iter().rev())
        .zip(base_points.into_iter().rev())
    {
        if contract::type_name(line) != Some("PreviewLine") {
            // Vixen leaves a string that is not a line where it was saved.
            return Err(ShapeSkip::Setting("strings that are not lines"));
        }
        // The string rotates its lights about its own centre.
        unrotated(line)?;
        // Vixen sets the line's two saved points, which must be there.
        super::line::ends(line)?;
        let nodes = pixel_nodes(line)?;
        let positions = super::line::spaced(base_end, top_end, nodes.len(), false, false)?;
        pixels.extend(
            nodes
                .into_iter()
                .zip(positions)
                .map(|(node, position)| ShapePixel {
                    node,
                    position,
                    arc: None,
                }),
        );
    }
    Ok(Laid {
        pixels,
        ellipses: Vec::new(),
    })
}

//! A half ellipse of lights standing on a box's bottom edge. Vixen:
//! `PreviewArch`.

use super::{Ellipse2, Laid, ShapePixel, ShapeSkip, pixel_nodes, point, strings, unrotated};
use crate::xml::Element;
use glam::DVec2;

/// The ellipse of an arch `width` wide and `height` high whose box's top
/// left is `top_left`, and `count` lights evenly spaced over its upper
/// half from left to right, each with its angle on the ellipse in degrees.
/// Vixen: `PreviewTools.GetArcPoints`, plus the box's top left as
/// `PreviewArch.Layout` and `PreviewCane.Layout` add it, without the
/// truncation of each point to whole units.
pub(super) fn arc(
    top_left: DVec2,
    width: f64,
    height: f64,
    count: usize,
) -> (Ellipse2, Vec<(DVec2, f64)>) {
    let ellipse = Ellipse2 {
        center: top_left + DVec2::new(width / 2.0, height),
        u: DVec2::new(width / 2.0, 0.0),
        v: DVec2::new(0.0, height),
    };
    let points = (0..count)
        .map(|index| {
            // Vixen steps by 180° / (count - 1) from 180°; a single light
            // takes only the first step, at 180°.
            let degrees = match index {
                0 => 180.0,
                _ => 180.0 + 180.0 * index as f64 / (count - 1) as f64,
            };
            let (sin, cos) = degrees.to_radians().sin_cos();
            (ellipse.center + ellipse.u * cos + ellipse.v * sin, degrees)
        })
        .collect();
    (ellipse, points)
}

/// Vixen: `PreviewArch.OnDeserialized`, `PreviewArch.Layout`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    if !strings(shape).is_empty() {
        // Vixen's `Pixels` would then be the strings' pixels, which the
        // arch's layout does not place.
        return Err(ShapeSkip::Setting("strings"));
    }
    let top_left = point(shape, "_topLeft")?;
    let bottom_right = point(shape, "_bottomRight")?;
    let nodes = pixel_nodes(shape)?;
    let size = bottom_right - top_left;
    let (ellipse, points) = arc(top_left, size.x, size.y, nodes.len());
    Ok(Laid {
        pixels: nodes
            .into_iter()
            .zip(points)
            .map(|(node, (position, degrees))| ShapePixel {
                node,
                position,
                arc: Some((0, degrees)),
            })
            .collect(),
        ellipses: vec![ellipse],
    })
}

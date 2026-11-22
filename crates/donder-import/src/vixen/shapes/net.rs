//! A net of lights filling a four-cornered area on a staggered grid, all
//! lit by one element. Vixen: `PreviewNet`.

use super::{Laid, ShapePixel, ShapeSkip, pixel_nodes, point};
use crate::xml::Element;
use glam::DVec2;

/// Vixen: `PreviewNet._pixelSpacing`, which loads as 8 when unwritten or 0.
fn spacing(shape: &Element) -> Result<i64, ShapeSkip> {
    match super::contract::int(shape, "_pixelSpacing", 0)? {
        0 => Ok(8),
        spacing if spacing > 0 => Ok(i64::from(spacing)),
        _ => Err(ShapeSkip::Unplaceable("a net with negative spacing")),
    }
}

/// Whether `point` is inside the polygon `corners`, by the even-odd rule.
fn inside(point: DVec2, corners: &[DVec2; 4]) -> bool {
    let mut inside = false;
    for index in 0..corners.len() {
        let a = corners[index];
        let b = corners[(index + 1) % corners.len()];
        if (a.y > point.y) != (b.y > point.y)
            && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
        {
            inside = !inside;
        }
    }
    inside
}

/// Vixen: `PreviewNet.Layout`. Every light takes the node of the net's first
/// saved pixel. Rows run down the bounds of the corners every spacing; each
/// row's lights run across every spacing, alternate rows offset by half a
/// spacing, where the filled polygon of the corners covers them. Vixen
/// fills that polygon with GDI, whose edge pixels this decides by point
/// in polygon.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    let corners = [
        point(shape, "_topLeft")?,
        point(shape, "_topRight")?,
        point(shape, "_bottomRight")?,
        point(shape, "_bottomLeft")?,
    ];
    let spacing = spacing(shape)?;
    let node = pixel_nodes(shape)?.into_iter().next().flatten();
    let min = corners
        .iter()
        .fold(DVec2::splat(f64::INFINITY), |min, corner| min.min(*corner));
    let max = corners
        .iter()
        .fold(DVec2::splat(f64::NEG_INFINITY), |max, corner| {
            max.max(*corner)
        });
    let (width, height) = ((max.x - min.x) as i64, (max.y - min.y) as i64);
    let mut laid = Laid::default();
    if width <= 0 || height <= 0 {
        return Ok(laid);
    }
    let local = corners.map(|corner| corner - min);
    // Vixen counts rows from 1: odd rows start half a spacing in.
    for (row, y) in (1..).zip((0..height).step_by(spacing as usize)) {
        let offset = if row % 2 == 0 { spacing } else { spacing / 2 };
        for x in (0..width).filter(|x| (x + offset) % spacing == 0) {
            let at = DVec2::new(x as f64, y as f64);
            if inside(at + DVec2::splat(0.5), &local) {
                laid.pixels.push(ShapePixel {
                    node: node.clone(),
                    position: at + min,
                    arc: None,
                });
            }
        }
    }
    Ok(laid)
}

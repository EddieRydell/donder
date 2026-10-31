//! Lights evenly around an ellipse. Vixen: `PreviewEllipse`.

use super::single::own_pixels;
use super::{Ellipse2, Laid, ShapePixel, ShapeSkip, unrotated};
use crate::vixen::contract;
use crate::xml::Element;
use glam::DVec2;

/// The ellipse inscribed in the box `size` across from `left_top`, which
/// may be negative. Vixen: `PreviewTools.GetEllipsePoints`.
pub(super) fn inscribed(left_top: DVec2, size: DVec2) -> Ellipse2 {
    let half = size / 2.0;
    Ellipse2 {
        center: left_top + half,
        u: DVec2::new(half.x, 0.0),
        v: DVec2::new(0.0, half.y),
    }
}

/// How far around, in degrees, the `index`th of `count` points of a full
/// turn starting `offset` degrees around lies. Vixen:
/// `PreviewTools.GetEllipsePoints` with `degrees` 360.
pub(super) fn turn_degrees(count: usize, offset: f64, index: usize) -> f64 {
    offset + 360.0 * index as f64 / count as f64
}

/// The point `degrees` around the ellipse, as Vixen places it before
/// truncating it to whole units.
pub(super) fn at(ellipse: &Ellipse2, degrees: f64) -> DVec2 {
    let (sin, cos) = degrees.to_radians().sin_cos();
    ellipse.center + ellipse.u * cos + ellipse.v * sin
}

/// A box corner. Vixen: `PreviewPoint`.
fn corner(shape: &Element, name: &str) -> Result<DVec2, ShapeSkip> {
    let (x, y) = contract::point(shape, name)?;
    Ok(DVec2::new(f64::from(x), f64::from(y)))
}

/// Vixen: `PreviewEllipse.Layout`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    let nodes = own_pixels(shape)?;
    let top_left = corner(shape, "_topLeft")?;
    let bottom_right = corner(shape, "_bottomRight")?;
    let ellipse = inscribed(top_left, bottom_right - top_left);
    let count = nodes.len();
    Ok(Laid {
        pixels: nodes
            .into_iter()
            .enumerate()
            .map(|(index, node)| {
                let degrees = turn_degrees(count, 0.0, index);
                ShapePixel {
                    node,
                    position: at(&ellipse, degrees),
                    arc: Some((0, degrees)),
                }
            })
            .collect(),
        ellipses: vec![ellipse],
    })
}

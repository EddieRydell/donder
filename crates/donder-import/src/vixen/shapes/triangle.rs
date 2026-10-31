//! Three lines of lights around a triangle's corners. Vixen:
//! `PreviewTriangle`.

use super::line::{ends, spaced};
use super::single::{not_custom, own_pixels};
use super::{Laid, ShapePixel, ShapeSkip, strings, unrotated};
use crate::vixen::contract;
use crate::xml::Element;
use glam::DVec2;

/// Vixen: `PreviewTriangle.Layout`, then each side's `PreviewLine.Layout`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    not_custom(shape)?;
    let corner = |name| -> Result<DVec2, ShapeSkip> {
        let (x, y) = contract::point(shape, name)?;
        Ok(DVec2::new(f64::from(x), f64::from(y)))
    };
    let corners = [corner("_point1")?, corner("_point2")?, corner("_point3")?];
    let sides = strings(shape);
    match sides.len() {
        // Its own pixels, which loading clears.
        0 => return Ok(Laid::default()),
        3 => {}
        // Vixen leaves each side where the side's own points put it.
        _ => return Err(ShapeSkip::Setting("sides other than three")),
    }
    let mut pixels = Vec::new();
    for (index, side) in sides.into_iter().enumerate() {
        if contract::type_name(side) != Some("PreviewLine") {
            return Err(ShapeSkip::Unplaceable("a triangle side that is not a line"));
        }
        unrotated(side)?;
        ends(side)?;
        let nodes = own_pixels(side)?;
        let positions = spaced(
            corners[index],
            corners[(index + 1) % 3],
            nodes.len(),
            false,
            false,
        )?;
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

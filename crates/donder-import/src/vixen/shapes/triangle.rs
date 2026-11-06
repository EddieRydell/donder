//! Three lines of lights around a triangle's corners. Vixen:
//! `PreviewTriangle`.

use super::multi_string::{add_line, child_line};
use super::single::not_custom;
use super::{Laid, ShapeSkip, point, strings, unrotated};
use crate::vixen::contract;
use crate::xml::Element;

/// Vixen: `PreviewTriangle.Layout`, then each side's `PreviewLine.Layout`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    not_custom(shape)?;
    let corners = [
        point(shape, "_point1")?,
        point(shape, "_point2")?,
        point(shape, "_point3")?,
    ];
    let sides = strings(shape);
    match sides.len() {
        // Its own pixels, which loading clears.
        0 => return Ok(Laid::default()),
        3 => {}
        // Vixen leaves each side where the side's own points put it.
        _ => return Err(ShapeSkip::Setting("sides other than three")),
    }
    let mut laid = Laid::default();
    for (index, side) in sides.into_iter().enumerate() {
        if contract::type_name(side) != Some("PreviewLine") {
            return Err(ShapeSkip::Unplaceable("a triangle side that is not a line"));
        }
        not_custom(side)?;
        let nodes = child_line(side)?;
        add_line(
            &mut laid,
            nodes,
            corners[index],
            corners[(index + 1) % 3],
            false,
            false,
        )?;
    }
    Ok(laid)
}

//! Lines of lights from an inner circle out to an ellipse. Vixen:
//! `PreviewStarBurst`.

use super::ellipse::{at, inscribed, turn_degrees};
use super::line::{ends, spaced};
use super::single::{not_custom, own_pixels};
use super::{Laid, ShapePixel, ShapeSkip, strings, unrotated};
use crate::vixen::contract;
use crate::xml::Element;
use glam::DVec2;

/// Vixen: `PreviewStarBurst.Layout`, then each spoke's `PreviewLine.Layout`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    not_custom(shape)?;
    let (left, top) = contract::point(shape, "_topLeft")?;
    let (right, bottom) = contract::point(shape, "_bottomRight")?;
    // Layout sets these two corners from the others; it fails without them.
    contract::required(shape, "_topRight")?;
    contract::required(shape, "_bottomLeft")?;
    let inner_size = contract::int(shape, "_innerCircleSize", 0)?;
    let xy_rotation = f64::from(contract::int(shape, "_XYRotation", 0)?);
    if contract::field(shape, "_strings").is_none() {
        // Vixen's `Strings` is then the star burst itself, not a line.
        return Err(ShapeSkip::Unplaceable("a star burst without spokes"));
    }
    let spokes = strings(shape);
    let mut lights = Vec::with_capacity(spokes.len());
    for spoke in &spokes {
        if contract::type_name(spoke) != Some("PreviewLine") {
            return Err(ShapeSkip::Unplaceable(
                "a star burst spoke that is not a line",
            ));
        }
        unrotated(spoke)?;
        ends(spoke)?;
        lights.push(own_pixels(spoke)?);
    }

    let width = right.wrapping_sub(left).wrapping_abs();
    let height = bottom.wrapping_sub(top).wrapping_abs();
    let outer = inscribed(
        DVec2::new(f64::from(left), f64::from(top)),
        DVec2::new(f64::from(width), f64::from(height)),
    );
    // Vixen centres the inner circle in whole units; that is kept, as it
    // shapes the star burst rather than snapping a light.
    let center_x = right.wrapping_sub(width / 2);
    let center_y = bottom.wrapping_sub(height / 2);
    let inner = inscribed(
        DVec2::new(
            f64::from(center_x.wrapping_sub(inner_size / 2)),
            f64::from(center_y.wrapping_sub(inner_size / 2)),
        ),
        DVec2::splat(f64::from(inner_size)),
    );

    // Vixen truncates each spoke's ends, and rounds its lights, to whole
    // units; this does neither.
    let count = spokes.len();
    let mut pixels = Vec::new();
    for (index, nodes) in lights.into_iter().enumerate() {
        let degrees = turn_degrees(count, xy_rotation, index);
        let positions = spaced(
            at(&inner, degrees),
            at(&outer, degrees),
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

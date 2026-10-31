//! A straight string of lights between two points. Vixen: `PreviewLine`.

use super::{Laid, ShapePixel, ShapeSkip, pixel_nodes, unrotated};
use crate::vixen::contract;
use crate::xml::Element;
use glam::DVec2;

/// `count` lights evenly spaced from `start` to `end`, with a light's gap
/// added before the first when `start_padding` and after the last when
/// `end_padding`, as a shape of strings sets them on its lines. Vixen:
/// `PreviewLine.Layout`, `AddStartPadding`, `AddEndPadding`.
pub(super) fn spaced(
    start: DVec2,
    end: DVec2,
    count: usize,
    start_padding: bool,
    end_padding: bool,
) -> Result<Vec<DVec2>, ShapeSkip> {
    let gaps = count as i64 - i64::from(!start_padding) + i64::from(end_padding);
    if gaps <= 0 {
        // One light and no padding: Vixen places it at the start before it
        // would step by a gap of no number.
        return Ok(vec![start; count]);
    }
    let step = (start - end) / gaps as f64;
    let first = if start_padding { start - step } else { start };
    Ok((0..count)
        .map(|index| first - step * index as f64)
        .collect())
}

/// The line's two points. Vixen: `PreviewLine._points`.
pub(super) fn ends(shape: &Element) -> Result<(DVec2, DVec2), ShapeSkip> {
    let points = contract::points(shape, "_points")?;
    match points[..] {
        [(x0, y0), (x1, y1), ..] => Ok((
            DVec2::new(f64::from(x0), f64::from(y0)),
            DVec2::new(f64::from(x1), f64::from(y1)),
        )),
        _ => Err(ShapeSkip::Unplaceable("a line without two points")),
    }
}

pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    let (start, end) = ends(shape)?;
    let nodes = pixel_nodes(shape)?;
    let positions = spaced(start, end, nodes.len(), false, false)?;
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

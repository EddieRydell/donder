//! A candy cane: a vertical string of lights up its left edge, then an arch
//! over its top. Vixen: `PreviewCane`.

use super::arch::{arc, lights, point};
use super::{Laid, ShapePixel, ShapeSkip, strings, unrotated};
use crate::vixen::contract;
use crate::xml::Element;
use glam::DVec2;

/// A light count field, which Vixen indexes its pixels by.
fn count(shape: &Element, name: &str) -> Result<usize, ShapeSkip> {
    // Vixen indexes the hook's pixels after a negative number of line
    // pixels, or before the first.
    usize::try_from(contract::int(shape, name, 0)?)
        .map_err(|_| ShapeSkip::Unplaceable("a cane with a negative light count"))
}

/// Vixen: `PreviewCane.OnDeserialized`, `PreviewCane.Layout`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    if !strings(shape).is_empty() {
        // Vixen's `Pixels` would then be the strings' pixels, which the
        // cane's layout does not place.
        return Err(ShapeSkip::Setting("strings"));
    }
    let top_left = point(shape, "_topLeftPoint")?;
    let bottom_right = point(shape, "_bottomRightPoint")?;
    let arch_left = point(shape, "_archLeftPoint")?;
    let line_count = count(shape, "_linePixelCount")?;
    let arch_count = count(shape, "_archPixelCount")?;
    let nodes = lights(shape)?;
    if nodes.len() < line_count + arch_count {
        return Err(ShapeSkip::Unplaceable(
            "a cane with fewer lights than its counts",
        ));
    }
    if nodes.len() > line_count + arch_count {
        // Vixen leaves the lights past its counts where they were saved.
        return Err(ShapeSkip::Setting("lights beyond its counts"));
    }
    // The line runs up from the bottom right's height at the top left's x,
    // its lights spaced over the height to the arch's start, without
    // Vixen's truncation of each light's offset up the line to whole units.
    let spacing = (bottom_right.y - arch_left.y) / line_count as f64;
    let line = (0..line_count).map(|index| {
        (
            DVec2::new(top_left.x, bottom_right.y - index as f64 * spacing),
            None,
        )
    });
    let (ellipse, points) = arc(
        top_left,
        bottom_right.x - top_left.x,
        arch_left.y - top_left.y,
        arch_count,
    );
    let hook = points
        .into_iter()
        .map(|(position, degrees)| (position, Some((0, degrees))));
    Ok(Laid {
        pixels: nodes
            .into_iter()
            .zip(line.chain(hook))
            .map(|(node, (position, arc))| ShapePixel {
                node,
                position,
                arc,
            })
            .collect(),
        ellipses: vec![ellipse],
    })
}

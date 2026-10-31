//! A string of lights along a path of points. Vixen: `PreviewPolyLine`.

use super::single::own_pixels;
use super::{Laid, ShapePixel, ShapeSkip, unrotated};
use crate::vixen::contract;
use crate::xml::Element;
use glam::DVec2;

/// Vixen: `PreviewPolyLine.Layout`, `LineLength`, `SegmentLength`,
/// `PixelSpacing`.
pub(super) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    unrotated(shape)?;
    let nodes = own_pixels(shape)?;
    let points: Vec<DVec2> = contract::points(shape, "_points")?
        .into_iter()
        .map(|(x, y)| DVec2::new(f64::from(x), f64::from(y)))
        .collect();
    let Some(&last) = points.last() else {
        return Err(ShapeSkip::Unplaceable("a poly line without points"));
    };
    let lengths: Vec<f64> = points
        .windows(2)
        .map(|ends| ends[0].distance(ends[1]))
        .collect();
    let line_length: f64 = lengths.iter().sum();
    let count = nodes.len();
    if line_length <= 0.0 {
        return match count {
            0 => Ok(Laid::default()),
            _ => Err(ShapeSkip::Unplaceable("a poly line of no length")),
        };
    }
    match count {
        // Vixen sets the last of no lights.
        0 => return Err(ShapeSkip::Unplaceable("a poly line without lights")),
        // Vixen spaces one light infinitely, then fails converting the
        // distance to its end.
        1 => return Err(ShapeSkip::Unplaceable("a poly line of one light")),
        _ => {}
    }
    let spacing = line_length / (count - 1) as f64;
    let mut positions = vec![None; count];
    positions[count - 1] = Some(last);
    let mut empty = 0.0;
    let mut pixel = 0;
    for (segment, &length) in lengths.iter().enumerate() {
        if !(empty + length > spacing || segment == 0) {
            empty += length;
            continue;
        }
        if length == 0.0 {
            // Vixen's direction along it is no number.
            return Err(ShapeSkip::Unplaceable("a poly line segment of no length"));
        }
        let start_length = if segment == 0 { 0.0 } else { spacing - empty };
        let spaces = ((length - start_length) / spacing).trunc();
        empty = length - start_length - spaces * spacing;
        let start = points[segment];
        let direction = (points[segment + 1] - start) / length;
        // Vixen rounds the distances to these lights, and the lights, to
        // whole units; this places them where its spacing puts them.
        let mut index = 0.0;
        while index < spaces + 1.0 {
            if pixel < count - 1 {
                positions[pixel] = Some(start + direction * (start_length + index * spacing));
            }
            pixel += 1;
            index += 1.0;
        }
    }
    let positions: Option<Vec<DVec2>> = positions.into_iter().collect();
    let Some(positions) = positions else {
        return Err(ShapeSkip::Unplaceable(
            "a poly line that ends before its lights",
        ));
    };
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

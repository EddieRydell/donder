//! Window frames: one string around a rectangle.

use super::{Local, Node, Spec};
use crate::placement;
use crate::report::SkipReason;
use glam::DVec3;

/// The sides of a frame `width` wide and `height` tall, clockwise from the
/// bottom left corner: up the left, across the top, down the right and back
/// along the bottom. The sides reach the corners; the top and bottom stop
/// short of them.
fn sides(top: u32, side: u32, bottom: u32) -> [Vec<DVec3>; 4] {
    let width = f64::from(top.max(bottom) + 2);
    let height = f64::from(side.max(1) - 1);
    let (left, right) = (-width / 2.0, width / 2.0);
    let (low, high) = (-height / 2.0, height / 2.0);
    let column = |count: u32, x: f64, rising: bool| {
        (0..count)
            .map(|index| {
                let y = low + f64::from(index);
                DVec3::new(x, if rising { y } else { low + high - y }, 0.0)
            })
            .collect::<Vec<_>>()
    };
    let row = |count: u32, y: f64, rightward: bool| {
        let step = width / f64::from(count + 1);
        (1..=count)
            .map(|index| {
                let x = left + step * f64::from(index);
                DVec3::new(if rightward { x } else { left + right - x }, y, 0.0)
            })
            .collect::<Vec<_>>()
    };
    [
        column(side, left, true),
        row(top, high, true),
        column(side, right, false),
        row(bottom, low, false),
    ]
}

/// The string starts at the corner the direction and start side name and
/// runs clockwise or counter-clockwise.
pub(super) fn window_frame(spec: &Spec) -> Result<Local, SkipReason> {
    let top = spec.count("TopNodes", "parm1", 0)?;
    let side = spec.count("SideNodes", "parm2", 0)?;
    let bottom = spec.count("BottomNodes", "parm3", 0)?;
    let clockwise = matches!(spec.text("Rotation").unwrap_or("CW"), "CW" | "Clockwise");
    let sides = sides(top, side, bottom);
    // The side leaving each corner clockwise: bottom left, top left, top
    // right, bottom right.
    let corner = match (spec.left_to_right(), spec.bottom_to_top(true)) {
        (true, true) => 0,
        (true, false) => 1,
        (false, false) => 2,
        (false, true) => 3,
    };
    let order: Vec<DVec3> = if clockwise {
        (0..4)
            .flat_map(|step| sides[(corner + step) % 4].iter().copied())
            .collect()
    } else {
        (1..=4)
            .flat_map(|step| sides[(corner + 4 - step) % 4].iter().rev().copied())
            .collect()
    };
    let nodes = order
        .into_iter()
        .enumerate()
        .map(|(node, position)| Node {
            string: 0,
            offset: node as u32,
            position,
            arc: None,
        })
        .collect::<Vec<_>>();
    let total = nodes.len() as u32;
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        strings: 1,
        string_nodes: total,
        placement: placement::boxed(spec)?,
    })
}

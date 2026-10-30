//! Icicles: drops hanging from the placement's line.

use super::{Local, Node, Spec};
use crate::placement;
use crate::report::SkipReason;
use glam::DVec3;

/// Each drop is a column one unit right of the last, its lights one unit
/// apart down from the line. Drop lengths repeat `DropPattern`, restarting
/// on each string; every light follows the last in the model's channels.
pub(super) fn icicles(spec: &Spec) -> Result<Local, SkipReason> {
    let strings = spec.count("NumStrings", "parm1", 1)?;
    let string_lights = spec.count("NodesPerString", "parm2", 1)?;
    let mut pattern = spec.csv("DropPattern")?;
    if !spec.has("DropPattern") {
        pattern = vec![3, 4, 5, 4];
    }
    let pattern = pattern
        .into_iter()
        .map(|drop| {
            u32::try_from(drop)
                .ok()
                .filter(|drop| *drop > 0)
                .ok_or_else(|| SkipReason::Attribute {
                    name: "DropPattern".to_string(),
                    value: drop.to_string(),
                })
        })
        .collect::<Result<Vec<u32>, _>>()?;
    let alternate = spec.is_true("AlternateNodes", false);
    let mut column: i64 = -1;
    let mut lights = Vec::new();
    for _ in 0..strings {
        column += 1;
        let mut drop = 0;
        let mut down = 0;
        for _ in 0..string_lights {
            while down >= pattern[drop] {
                column += 1;
                down = 0;
                drop = (drop + 1) % pattern.len();
            }
            let size = pattern[drop];
            let place = if !alternate {
                down
            } else if down < size.div_ceil(2) {
                down * 2
            } else {
                (size - (down + 1)) * 2 + 1
            };
            lights.push((column, place));
            down += 1;
        }
    }
    let last = column.max(0);
    let single = last == 0;
    let nodes = lights
        .into_iter()
        .enumerate()
        .map(|(node, (column, place))| {
            let column = if spec.left_to_right() {
                column
            } else {
                last - column
            };
            Node {
                string: 0,
                offset: node as u32,
                position: DVec3::new(
                    if single { 0.5 } else { column as f64 },
                    f64::from(place),
                    0.0,
                ),
                arc: None,
            }
        })
        .collect();
    let width = if single { 1.0 } else { last as f64 };
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        strings,
        string_nodes: string_lights,
        placement: placement::three_point(spec, width, true)?,
    })
}

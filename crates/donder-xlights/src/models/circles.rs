//! Circle and Wreath models: rings of evenly spaced lights.

use super::{ArcPoint, Ellipse, Local, Node, Spec};
use crate::placement;
use crate::report::SkipReason;
use glam::DVec3;
use std::f64::consts::{PI, TAU};

fn ring(radius: f64) -> Ellipse {
    Ellipse {
        center: DVec3::ZERO,
        u: DVec3::new(radius, 0.0, 0.0),
        v: DVec3::new(0.0, radius, 0.0),
    }
}

/// A node at `angle` clockwise from the top of a ring.
fn on_ring(radius: f64, angle: f64) -> DVec3 {
    DVec3::new(angle.sin() * radius, angle.cos() * radius, 0.0)
}

/// Concentric rings, the outer one first unless inside out, their sizes
/// from `LayerSizes`. Each ring starts at the top, or the bottom, and runs
/// clockwise, or counter-clockwise right to left.
pub(super) fn circle(spec: &Spec) -> Result<Local, SkipReason> {
    let strings = spec.count("NumStrings", "parm1", 1)?;
    let string_nodes = spec.count("NodesPerString", "parm2", 1)?;
    let center_percent = spec.count("centerPercent", "parm3", 0)?;
    let inside_out = spec.text("InsideOut") == Some("1");
    let total = super::nodes(&[strings, string_nodes])?;
    // Older files list the rings innermost first.
    let mut sizes: Vec<i64> = if spec.has("circleSizes") {
        spec.csv("circleSizes")?.into_iter().rev().collect()
    } else {
        spec.csv("LayerSizes")?
    };
    if sizes.is_empty() {
        sizes.push(i64::from(total));
    }
    let mut placed = 0_i64;
    let sizes: Vec<u32> = sizes
        .into_iter()
        .map(|size| {
            let size = size.clamp(0, (i64::from(total) - placed).max(0));
            placed += size;
            size as u32
        })
        .collect();
    let largest = sizes.iter().copied().max().unwrap_or(0);
    let outer = f64::from(largest) / 2.0;
    let inner = f64::from(center_percent) / 100.0 * outer;
    let layers = sizes.len();
    let bottom = spec.bottom_to_top(false);
    let left_to_right = spec.left_to_right();

    let mut nodes = Vec::new();
    let mut ellipses = Vec::new();
    for (layer, &count) in sizes.iter().enumerate() {
        let radius = if layers == 1 {
            outer
        } else {
            let step = if inside_out {
                layers - layer - 1
            } else {
                layer
            };
            inner + (outer - inner) * (1.0 - step as f64 / (layers - 1) as f64)
        };
        ellipses.push(ring(radius));
        for index in 0..count {
            let mut angle =
                if bottom { -PI } else { 0.0 } + TAU * f64::from(index) / f64::from(count);
            if !left_to_right {
                angle = -angle;
            }
            let node = nodes.len() as u32;
            nodes.push(Node {
                string: node / string_nodes.max(1),
                offset: node % string_nodes.max(1),
                position: on_ring(radius, angle),
                arc: Some(ArcPoint {
                    arc: ellipses.len() - 1,
                    degrees: 90.0 - angle.to_degrees(),
                }),
            });
        }
    }
    Ok(Local {
        nodes,
        ellipses,
        strings,
        string_nodes,
        placement: placement::boxed(spec)?,
    })
}

/// One ring of all the lights, starting at the top, or the bottom, and
/// running clockwise when numbered left to right from the top or right to
/// left from the bottom, and counter-clockwise otherwise, as in xLights.
pub(super) fn wreath(spec: &Spec) -> Result<Local, SkipReason> {
    let strings = spec.count("NumStrings", "parm1", 1)?;
    let string_nodes = spec.count("NodesPerString", "parm2", 50)?;
    let total = super::nodes(&[strings, string_nodes])?;
    let bottom = spec.bottom_to_top(true);
    let radius = f64::from(total / 2);
    let mut step = 1.0 / f64::from(total.max(1));
    if spec.left_to_right() != bottom {
        step = -step;
    }
    let mut turn: f64 = if bottom { 0.5 } else { 0.0 };
    let nodes = (0..total)
        .map(|node| {
            let angle = turn * TAU;
            turn = (turn + step).rem_euclid(1.0);
            Node {
                string: node / string_nodes.max(1),
                offset: node % string_nodes.max(1),
                position: on_ring(radius, angle),
                arc: Some(ArcPoint {
                    arc: 0,
                    degrees: 90.0 - angle.to_degrees(),
                }),
            }
        })
        .collect();
    Ok(Local {
        nodes,
        ellipses: vec![ring(radius)],
        strings,
        string_nodes,
        placement: placement::boxed(spec)?,
    })
}

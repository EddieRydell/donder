//! Arches: half ellipses side by side along the placement's line.

use super::{ArcPoint, Ellipse, Local, Node, Spec};
use crate::placement;
use crate::report::SkipReason;
use glam::DVec3;
use std::f64::consts::{FRAC_PI_2, PI};

/// Each arch is one string. Its lights run evenly by angle over `Arc`
/// degrees of an ellipse as wide as twice its light count and as tall as its
/// light count times the placement's height. Arches numbered right to left
/// start with the last arch's last node.
pub(super) fn arches(spec: &Spec) -> Result<Local, SkipReason> {
    if spec.has("LayerSizes") {
        return Err(SkipReason::Setting("layer sizes"));
    }
    let arches = spec.count("NumArches", "parm1", 1)?;
    let arch_nodes = spec.count("NodesPerArch", "parm2", 1)?;
    let lights = spec.count("LightsPerNode", "parm3", 1)?.max(1);
    let arc = spec.float("Arc", spec.float("arc", 180.0)?)?.to_radians();
    let gap = spec.float("Gap", 0.0)?;
    let height = spec.float("Height", 1.0)?;
    let skew = spec
        .float("ArchesSkew", spec.float("Angle", 0.0)?)?
        .to_radians();
    let left_to_right = spec.left_to_right();

    let arch_lights = f64::from(arch_nodes * lights);
    let middle = (arch_lights - 1.0) / 2.0;
    let start = (PI - arc) / 2.0;
    let first_x = middle * (-FRAC_PI_2 + start).sin() * 2.0 + arch_lights;
    let width = arch_lights * 2.0 - first_x;
    let angle = |light: u32| -FRAC_PI_2 + start + arc * f64::from(light) / middle / 2.0;

    let mut lowest = f64::INFINITY;
    let mut nodes = Vec::new();
    let mut ellipses = Vec::new();
    for arch in 0..arches {
        let center = f64::from(arch) * (width + gap) + arch_lights;
        ellipses.push(Ellipse {
            center: DVec3::new(center, 0.0, 0.0),
            u: DVec3::new(middle * 2.0, 0.0, 0.0),
            v: DVec3::new(0.0, arch_lights * height, 0.0),
        });
        for node in 0..arch_nodes {
            let points: Vec<DVec3> = (0..lights)
                .map(|light| {
                    let angle = angle(node * lights + light);
                    let y = arch_lights * angle.cos();
                    lowest = lowest.min(y);
                    // The skew turns each light about the foot of its own
                    // column.
                    let (sin, cos) = skew.sin_cos();
                    DVec3::new(
                        center + middle * angle.sin() * 2.0 - y * height * sin,
                        y * height * cos,
                        0.0,
                    )
                })
                .collect();
            let (string, offset) = if left_to_right {
                (arch, node)
            } else {
                (arches - 1 - arch, arch_nodes - 1 - node)
            };
            nodes.push(Node {
                string,
                offset,
                position: super::mean(&points),
                arc: (lights == 1 && skew == 0.0).then(|| ArcPoint {
                    arc: ellipses.len() - 1,
                    degrees: 90.0 - angle(node).to_degrees(),
                }),
            });
        }
    }
    // Arches that do not reach their feet are drawn from the bottom.
    if lowest > 1.0 {
        for node in &mut nodes {
            node.position.y -= lowest;
        }
        for ellipse in &mut ellipses {
            ellipse.center.y -= lowest;
        }
    }
    let total_width = width * f64::from(arches) + f64::from(arches.saturating_sub(1)) * gap;
    Ok(Local {
        nodes,
        ellipses,
        strings: arches,
        string_nodes: arch_nodes,
        placement: placement::three_point(spec, total_width.max(1.0), false)?,
    })
}

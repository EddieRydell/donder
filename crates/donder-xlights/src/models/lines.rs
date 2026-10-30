//! Single Line and Poly Line models.

use super::{Local, Node, Spec, mean};
use crate::placement;
use crate::report::SkipReason;
use glam::DVec3;

/// `strings × nodes` nodes evenly along the line, the first light at its
/// start and the last at its end. A string numbered right to left starts at
/// its far end.
pub(super) fn single_line(spec: &Spec) -> Result<Local, SkipReason> {
    let strings = spec.count("NumStrings", "parm1", 1)?;
    let string_nodes = spec.count("NodesPerString", "parm2", 50)?;
    let lights = spec.count("LightsPerNode", "parm3", 1)?.max(1);
    let total = super::nodes(&[strings, string_nodes])?;
    let left_to_right = spec.left_to_right();
    let light_count = super::nodes(&[total, lights])?;
    let step = if light_count > 1 {
        f64::from(total) / f64::from(light_count - 1)
    } else {
        0.0
    };
    let nodes = (0..total)
        .map(|node| {
            let points: Vec<DVec3> = (0..lights)
                .map(|light| {
                    let x = if light_count > 1 {
                        f64::from(node * lights + light) * step
                    } else {
                        0.5
                    };
                    DVec3::new(x, 0.0, 0.0)
                })
                .collect();
            let along = node % string_nodes;
            Node {
                string: node / string_nodes,
                offset: if left_to_right {
                    along
                } else {
                    string_nodes - 1 - along
                },
                position: mean(&points),
                arc: None,
            }
        })
        .collect();
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        strings,
        string_nodes,
        placement: placement::two_point(spec, f64::from(total.max(1)))?,
    })
}

fn triples(spec: &Spec, name: &str) -> Result<Vec<f64>, SkipReason> {
    spec.element
        .attribute(name)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            item.parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| SkipReason::Attribute {
                    name: name.to_string(),
                    value: item.to_string(),
                })
        })
        .collect()
}

/// Enough points along a curved segment to measure and place lights on it.
const CURVE_STEPS: usize = 200;

/// The path from point to point, with curved segments broken into short
/// straight pieces.
fn path(spec: &Spec) -> Result<Vec<DVec3>, SkipReason> {
    let values = triples(spec, "PointData")?;
    let points: Vec<DVec3> = values
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[x, y, z]| DVec3::new(x, y, z))
        .collect();
    let curves = triples(spec, "cPointData")?;
    let mut path = Vec::new();
    for (segment, pair) in points.windows(2).enumerate() {
        let curve = curves
            .as_chunks::<7>()
            .0
            .iter()
            .find(|curve| curve[0] as usize == segment);
        path.push(pair[0]);
        if let Some(curve) = curve {
            let control0 = DVec3::new(curve[1], curve[2], curve[3]);
            let control1 = DVec3::new(curve[4], curve[5], curve[6]);
            for step in 1..CURVE_STEPS {
                let t = step as f64 / CURVE_STEPS as f64;
                let u = 1.0 - t;
                path.push(
                    pair[0] * (u * u * u)
                        + control0 * (3.0 * u * u * t)
                        + control1 * (3.0 * u * t * t)
                        + pair[1] * (t * t * t),
                );
            }
        }
    }
    if let Some(last) = points.last() {
        path.push(*last);
    }
    Ok(path)
}

/// The point `distance` along `path`.
fn along(path: &[DVec3], distance: f64) -> DVec3 {
    let mut covered = 0.0;
    for pair in path.windows(2) {
        let length = pair[0].distance(pair[1]);
        if length > 0.0 && covered + length >= distance {
            return pair[0].lerp(pair[1], (distance - covered) / length);
        }
        covered += length;
    }
    path.last().copied().unwrap_or_default()
}

/// One string of lights spread evenly along the path, half a gap from each
/// end. Per-segment light counts, drops and extra strings are not imported.
pub(super) fn poly_line(spec: &Spec) -> Result<Local, SkipReason> {
    if spec.has("PolyStrings") && spec.int("PolyStrings", 1)? > 1 {
        return Err(SkipReason::Setting("number of strings"));
    }
    if spec.element.attribute("Seg1").is_some() {
        return Err(SkipReason::Setting("lights per segment"));
    }
    if spec.csv("DropPattern")?.iter().any(|&drop| drop != 1) {
        return Err(SkipReason::Setting("drop pattern"));
    }
    let string_nodes = spec.count("NodesPerString", "parm2", 0)?;
    let lights = spec.count("LightsPerNode", "parm3", 1)?.max(1);
    let path = path(spec)?;
    let length: f64 = path.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
    if path.len() < 2 || length == 0.0 {
        return Err(SkipReason::NoPixels);
    }
    let light_count = f64::from(super::nodes(&[string_nodes, lights])?);
    let left_to_right = spec.left_to_right();
    let nodes = (0..string_nodes)
        .map(|node| {
            let points: Vec<DVec3> = (0..lights)
                .map(|light| {
                    let index = f64::from(node * lights + light);
                    along(&path, (index + 0.5) * length / light_count)
                })
                .collect();
            Node {
                string: 0,
                offset: if left_to_right {
                    node
                } else {
                    string_nodes - 1 - node
                },
                position: mean(&points),
                arc: None,
            }
        })
        .collect();
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        strings: 1,
        string_nodes,
        placement: placement::points(spec)?,
    })
}

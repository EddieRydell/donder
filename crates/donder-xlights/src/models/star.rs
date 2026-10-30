//! Star models: lights evenly along the zig-zag between an outer and an
//! inner circle, one star outline per layer.

use super::{Local, Node, Spec};
use crate::placement;
use crate::report::SkipReason;
use glam::DVec3;
use std::f64::consts::{PI, TAU};

fn on_circle(radius: f64, angle: f64) -> DVec3 {
    DVec3::new(radius * angle.sin(), radius * angle.cos(), 0.0)
}

/// Where the star starts, as xLights names it; files that leave it out
/// give it by direction and start side.
fn start_location(spec: &Spec) -> String {
    match spec.element.attribute("StarStartLocation") {
        Some(location) if !location.is_empty() => location.to_string(),
        _ => match (spec.left_to_right(), spec.bottom_to_top(true)) {
            (true, true) => "Bottom Ctr-CW",
            (true, false) => "Top Ctr-CCW",
            (false, true) => "Bottom Ctr-CCW",
            (false, false) => "Top Ctr-CW",
        }
        .to_string(),
    }
}

pub(super) fn star(spec: &Spec) -> Result<Local, SkipReason> {
    let strings = spec.count("NumStrings", "parm1", 1)?;
    let string_nodes = spec.count("NodesPerString", "parm2", 1)?;
    let points = spec.count("StarPoints", "parm3", 5)?.max(2);
    let total = strings * string_nodes;
    let mut sizes = if spec.has("starSizes") {
        spec.csv("starSizes")?
    } else {
        spec.csv("LayerSizes")?
    };
    if sizes.len() <= 1 {
        sizes = vec![i64::from(total)];
    }
    let sizes = sizes
        .into_iter()
        .map(|size| {
            u32::try_from(size).map_err(|_| SkipReason::Attribute {
                name: "LayerSizes".to_string(),
                value: size.to_string(),
            })
        })
        .collect::<Result<Vec<u32>, _>>()?;
    let layers = sizes.len() as u32;
    let width = sizes
        .iter()
        .enumerate()
        .map(|(layer, &size)| {
            let outside = layers - layer as u32 - 1;
            1 + (size as f32 * (1.0 + outside as f32 / layers as f32)) as u32
        })
        .max()
        .unwrap_or(1);
    let ratio = spec.float("starRatio", 2.618034)?.max(1.0);
    let mut outer = f64::from(width) / 2.0;
    let mut inner = outer / ratio;
    let mut delta = 0.0;
    if layers > 1 {
        let mut center_percent = spec.float("starCenterPercent", -1.0)?;
        if center_percent == -1.0 {
            center_percent = 100.0 / f64::from(layers);
        }
        delta = outer * (100.0 - center_percent) / (100.0 * f64::from(layers - 1));
    }
    let location = start_location(spec);
    let gap = TAU / f64::from(points);
    let direction = if location.contains("-CCW") { -1.0 } else { 1.0 };
    let start_angle = if location.contains("Top") {
        0.0
    } else if location.contains("Bottom Ctr") {
        PI
    } else if location.contains("Left") {
        PI + if points % 2 == 1 { gap / 2.0 } else { 0.0 }
    } else {
        PI - if points % 2 == 1 { gap / 2.0 } else { 0.0 }
    };
    let order: Vec<usize> = if location.contains("Inside") {
        outer -= delta * f64::from(layers - 1);
        inner = outer / ratio;
        delta = -delta;
        (0..sizes.len()).collect()
    } else {
        (0..sizes.len()).rev().collect()
    };
    let segments = 2 * points;
    let mut nodes = Vec::new();
    for layer in order {
        let layer_nodes = sizes[layer];
        if nodes.len() as u32 >= total || layer_nodes == 0 {
            outer -= delta;
            inner = outer / ratio;
            continue;
        }
        let end_node = (nodes.len() as u32 + layer_nodes).min(total);
        let mut start_outer = !location.contains("Bottom Ctr");
        let first = on_circle(if start_outer { outer } else { inner }, start_angle);
        let second = on_circle(
            if start_outer { inner } else { outer },
            start_angle + gap / 2.0,
        );
        let segment_length = first.distance(second);
        let light_gap = f64::from(segments) * segment_length / f64::from(layer_nodes);
        let mut position = 0.0;
        let mut angle = start_angle;
        let mut segment_end = 0.0;
        for segment in 0..segments {
            let from = on_circle(if start_outer { outer } else { inner }, angle);
            let to = on_circle(
                if start_outer { inner } else { outer },
                angle + gap * direction / 2.0,
            );
            let segment_start = segment_end;
            segment_end = segment_start + segment_length;
            if segment == segments - 1 {
                segment_end += 0.001;
            }
            while position < segment_end && (nodes.len() as u32) < end_node {
                let point = from.lerp(to, (position - segment_start) / segment_length);
                position += light_gap;
                let node = nodes.len() as u32;
                nodes.push(Node {
                    string: node / string_nodes.max(1),
                    offset: node % string_nodes.max(1),
                    position: point,
                    arc: None,
                });
            }
            angle += gap * direction / 2.0;
            start_outer = !start_outer;
        }
        outer -= delta;
        inner = outer / ratio;
    }
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        strings,
        string_nodes,
        placement: placement::boxed(spec)?,
    })
}

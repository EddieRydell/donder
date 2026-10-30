//! Star models: layers of lines between two circles.

use super::{Common, Local, Node, Span, Strings, account};
use crate::placement::{self, RenderSize};
use crate::read::{self, Name, Reader, Table, Use};
use crate::report::SkipReason;
use glam::DVec3;
use std::f64::consts::PI;

const STAR: Table = &[
    (Name::Exact("NumStrings"), Use::Modeled),
    (Name::Exact("NodesPerString"), Use::Modeled),
    (Name::Exact("StarPoints"), Use::Modeled),
    (Name::Exact("parm1"), Use::Modeled),
    (Name::Exact("parm2"), Use::Modeled),
    (Name::Exact("parm3"), Use::Modeled),
    (Name::Exact("starSizes"), Use::Modeled),
    (Name::Exact("LayerSizes"), Use::Modeled),
    (Name::Exact("StarStartLocation"), Use::Modeled),
    (Name::Exact("starRatio"), Use::Modeled),
    (Name::Exact("starCenterPercent"), Use::Modeled),
];

struct Settings {
    strings: i32,
    nodes_per_string: i32,
    points: i32,
    /// Lights on each layer, innermost first.
    layers: Vec<i32>,
    location: String,
    ratio: f32,
    center_percent: i32,
}

/// xLights: `XmlDeserializingModelFactory::DeserializeStar` and
/// `StarModel::ConvertFromDirStartSide`.
fn settings(reader: &Reader) -> Result<Settings, SkipReason> {
    let layers = if reader.has("starSizes") {
        read::layer_sizes(reader.as_string("starSizes", ""))
    } else {
        read::layer_sizes(reader.as_string("LayerSizes", ""))
    };
    let mut location = reader.as_string("StarStartLocation", "").to_string();
    if location.is_empty() {
        let (dir, side) = (
            reader.as_string("Dir", "L"),
            reader.as_string("StartSide", "B"),
        );
        location = match (dir == "L", side == "B") {
            (true, true) => "Bottom Ctr-CW",
            (true, false) => "Top Ctr-CCW",
            (false, true) => "Bottom Ctr-CCW",
            (false, false) => "Top Ctr-CW",
        }
        .to_string();
    }
    Ok(Settings {
        strings: reader.parm("NumStrings", "parm1", "1"),
        nodes_per_string: reader.parm("NodesPerString", "parm2", "1"),
        points: reader.parm("StarPoints", "parm3", "5"),
        layers,
        location,
        ratio: reader.as_float("starRatio", 2.618034)?,
        center_percent: reader.as_int("starCenterPercent", -1),
    })
}

pub(super) fn star(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let settings = match settings(reader) {
        Ok(settings) => settings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let strings = match Strings::uniform(settings.strings, settings.nodes_per_string, common.ts) {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let span = Ok(strings.span(&common.string_type));
    (span, layout(reader, common, settings, &strings))
}

fn on_circle(radius: f64, angle: f64) -> (f64, f64) {
    (radius * angle.sin(), radius * angle.cos())
}

/// xLights: `StarModel::InitModel`: each layer, outermost first unless the
/// start is inside, as lines from the outer circle to the inner and back,
/// its lights evenly along them; nodes the layers leave over stay at the
/// middle.
fn layout(
    reader: &Reader,
    common: &Common,
    settings: Settings,
    strings: &Strings,
) -> Result<Local, SkipReason> {
    account(reader, super::BOXED_PLACEMENT, &[STAR], super::NO_CHILDREN)?;
    let lights = strings.nodes as i32;
    if lights == 0 {
        return Err(SkipReason::NoPixels);
    }
    let points = settings.points.max(2);
    let mut layers = settings.layers;
    if layers.len() <= 1 {
        layers = vec![lights];
    }
    let count = layers.len() as i32;
    let mut max_on_layer = 0;
    for (layer, &size) in layers.iter().enumerate() {
        let outside = count - layer as i32 - 1;
        // xLights: a float times a double, truncated.
        let inflated = f64::from(size as f32) * (1.0 + f64::from(outside as f32 / count as f32));
        max_on_layer = max_on_layer.max(1 + inflated as i32);
    }
    let mut outer = f64::from(max_on_layer) / 2.0;
    let ratio = f64::from(settings.ratio.max(1.0));
    let mut inner = outer / ratio;
    let mut delta = 0.0;
    if count > 1 {
        let mut percent = settings.center_percent;
        if percent == -1 {
            percent = (100.0f32 / count as f32) as i32;
        }
        delta = (outer * f64::from(100.0f32 - percent as f32))
            / f64::from(100.0f32 * (count as f32 - 1.0));
    }
    let gap = (PI * 2.0) / f64::from(points);
    let location = settings.location.as_str();
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
        outer -= delta * f64::from(count - 1);
        inner = outer / ratio;
        delta = -delta;
        (0..layers.len()).collect()
    } else {
        (0..layers.len()).rev().collect()
    };
    let segments = 2 * points;
    let string_type = &common.string_type;
    let per_string = settings.nodes_per_string.max(1) as u32;
    let channel = |node: u32| strings.channel(string_type, node / per_string, node % per_string);
    let mut nodes: Vec<Node> = Vec::with_capacity(lights as usize);
    for layer in order {
        if nodes.len() as i32 >= lights {
            break;
        }
        let layer_nodes = layers[layer];
        let end_node = nodes.len() as i32 + layer_nodes;
        let mut start_outer = !location.contains("Bottom Ctr");
        let (sx, sy) = on_circle(if start_outer { outer } else { inner }, start_angle);
        let (ex, ey) = on_circle(
            if start_outer { inner } else { outer },
            start_angle + gap / 2.0,
        );
        let segment_length = ((ex - sx) * (ex - sx) + (ey - sy) * (ey - sy)).sqrt();
        let total = f64::from(segments) * segment_length;
        let light_gap = total / f64::from(layer_nodes);
        let mut position = 0.0;
        let mut angle = start_angle;
        let mut segment_end = 0.0;
        for segment in 0..segments {
            if nodes.len() as i32 >= lights {
                break;
            }
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
            while position < segment_end && (nodes.len() as i32) < end_node {
                let length =
                    ((to.0 - from.0) * (to.0 - from.0) + (to.1 - from.1) * (to.1 - from.1)).sqrt();
                let (x, y) = if length == 0.0 {
                    from
                } else {
                    let t = (position - segment_start) / length;
                    ((1.0 - t) * from.0 + t * to.0, (1.0 - t) * from.1 + t * to.1)
                };
                position += light_gap;
                let node = nodes.len() as u32;
                nodes.push(Node {
                    channel: channel(node),
                    lights: vec![DVec3::new(f64::from(x as f32), f64::from(y as f32), 0.0)],
                    arc: None,
                });
                if nodes.len() as i32 >= lights {
                    break;
                }
            }
            angle += gap * direction / 2.0;
            start_outer = !start_outer;
        }
        outer -= delta;
        inner = outer / ratio;
    }
    // xLights keeps the nodes the layers leave over, at the middle.
    for node in nodes.len() as u32..lights as u32 {
        nodes.push(Node {
            channel: channel(node),
            lights: vec![DVec3::ZERO],
            arc: None,
        });
    }
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        size: RenderSize {
            width: max_on_layer as f32,
        },
        placement: placement::boxed(reader)?,
    })
}

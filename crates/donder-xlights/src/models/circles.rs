//! Circles and wreaths: rings of lights.

use super::{ArcPoint, Common, Ellipse, Local, Node, Span, Strings, account};
use crate::placement::{self, RenderSize};
use crate::read::{self, Name, Reader, Table, Use};
use crate::report::SkipReason;
use glam::DVec3;
use std::f64::consts::PI;

const CIRCLE: Table = &[
    (Name::Exact("NumStrings"), Use::Modeled),
    (Name::Exact("NodesPerString"), Use::Modeled),
    (Name::Exact("centerPercent"), Use::Modeled),
    (Name::Exact("parm1"), Use::Modeled),
    (Name::Exact("parm2"), Use::Modeled),
    (Name::Exact("parm3"), Use::Modeled),
    (Name::Exact("circleSizes"), Use::Modeled),
    (Name::Exact("LayerSizes"), Use::Modeled),
    (Name::Exact("InsideOut"), Use::Modeled),
];

const WREATH: Table = &[
    (Name::Exact("NumStrings"), Use::Modeled),
    (Name::Exact("NodesPerString"), Use::Modeled),
    (Name::Exact("parm1"), Use::Modeled),
    (Name::Exact("parm2"), Use::Modeled),
];

struct CircleSettings {
    strings: i32,
    nodes_per_string: i32,
    /// Lights on each ring, innermost first.
    layers: Vec<i32>,
    center_percent: i32,
    inside_out: bool,
    /// xLights: a circle without `StartSide` starts at the top.
    bottom_to_top: bool,
}

/// xLights: `XmlDeserializingModelFactory::DeserializeCircle`, which reverses
/// the older `circleSizes` into layer sizes.
fn circle_settings(reader: &Reader, common: &Common) -> CircleSettings {
    let circle_sizes = reader.as_string("circleSizes", "");
    let layers = if circle_sizes.is_empty() {
        read::layer_sizes(reader.as_string("LayerSizes", ""))
    } else {
        let mut sizes: Vec<&str> = circle_sizes.split(',').collect();
        sizes.reverse();
        read::layer_sizes(&sizes.join(","))
    };
    CircleSettings {
        strings: reader.parm("NumStrings", "parm1", "1"),
        nodes_per_string: reader.parm("NodesPerString", "parm2", "1"),
        layers,
        center_percent: reader.parm("centerPercent", "parm3", "0"),
        inside_out: reader.equals("InsideOut", "0", "1"),
        bottom_to_top: reader.has("StartSide") && common.bottom_to_top,
    }
}

pub(super) fn circle(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let settings = circle_settings(reader, common);
    let strings = match Strings::uniform(settings.strings, settings.nodes_per_string, common.ts) {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let span = Ok(strings.span(&common.string_type));
    (span, circle_layout(reader, common, settings, &strings))
}

/// xLights: `CircleModel::InitModel`, `InitCircle` and `SetCircleCoord`:
/// rings wired outermost first unless inside out, each ring's lights evenly
/// round it from the top, or the bottom.
fn circle_layout(
    reader: &Reader,
    common: &Common,
    settings: CircleSettings,
    strings: &Strings,
) -> Result<Local, SkipReason> {
    account(
        reader,
        super::BOXED_PLACEMENT,
        &[CIRCLE],
        super::NO_CHILDREN,
    )?;
    let lights = strings.nodes as i64;
    let mut layers = settings.layers;
    if layers.len() <= 1 {
        layers = vec![lights as i32];
    }
    // xLights: InitCircle trims the layers to the model's lights, through
    // Model::SetLayerSize, which leaves a layer as it is when trimmed to 0.
    let mut placed = 0i64;
    for layer in &mut layers {
        if placed + i64::from(*layer) > lights {
            let trimmed = if placed > lights {
                0
            } else {
                (lights - placed) as i32
            };
            if trimmed != 0 {
                *layer = trimmed;
            }
        }
        placed += i64::from(*layer);
    }
    if placed < lights {
        return Err(SkipReason::Setting(
            "layer sizes that place fewer lights than the model has",
        ));
    }
    let count = layers.len();
    let strand = |circle: usize| {
        layers[if settings.inside_out {
            circle
        } else {
            count - circle - 1
        }]
    };
    let max_lights = layers.iter().copied().max().unwrap_or(0);
    let max_radius = f64::from(max_lights) / 2.0;
    let min_radius = f64::from(settings.center_percent) / 100.0 * max_radius;
    let string_type = &common.string_type;
    let per_string = settings.nodes_per_string.max(1) as u32;
    let mut nodes = Vec::with_capacity(lights as usize);
    let mut ellipses = Vec::new();
    let mut node = 0u32;
    let mut left = lights;
    for circle in 0..count {
        let loop_count = left.min(i64::from(strand(circle))) as i32;
        let radius = if count == 1 {
            max_radius
        } else if settings.inside_out {
            min_radius
                + (max_radius - min_radius)
                    * (1.0 - (count - circle - 1) as f64 / (count - 1) as f64)
        } else {
            min_radius + (max_radius - min_radius) * (1.0 - circle as f64 / (count - 1) as f64)
        };
        ellipses.push(Ellipse {
            center: DVec3::ZERO,
            u: DVec3::new(0.0, radius, 0.0),
            v: DVec3::new(radius, 0.0, 0.0),
        });
        for n in 0..loop_count {
            let along = if loop_count == 1 {
                0.0
            } else {
                f64::from(n) / f64::from(loop_count)
            };
            let mut angle = if settings.bottom_to_top { -PI } else { 0.0 } + PI * along * 2.0;
            if !common.left_to_right {
                angle = -angle;
            }
            nodes.push(Node {
                channel: strings.channel(string_type, node / per_string, node % per_string),
                lights: vec![DVec3::new(
                    f64::from((angle.sin() * radius) as f32),
                    f64::from((angle.cos() * radius) as f32),
                    0.0,
                )],
                arc: Some(ArcPoint {
                    arc: ellipses.len() - 1,
                    degrees: angle.to_degrees(),
                }),
            });
            node += 1;
        }
        left -= i64::from(loop_count);
    }
    Ok(Local {
        nodes,
        ellipses,
        size: RenderSize {
            width: max_lights as f32,
        },
        placement: placement::boxed(reader)?,
    })
}

pub(super) fn wreath(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    // xLights: DeserializeWreath.
    let count = reader.parm("NumStrings", "parm1", "1");
    let per_string = reader.parm("NodesPerString", "parm2", "50");
    let strings = match Strings::uniform(count, per_string, common.ts) {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let span = Ok(strings.span(&common.string_type));
    (span, wreath_layout(reader, common, &strings, per_string))
}

/// xLights: `WreathModel::InitWreath` and `CopyBufCoord2ScreenCoord`: one
/// ring of all the lights, each at the whole buffer cell nearest its place,
/// from the top, or the bottom, clockwise when numbered left to right from
/// the top or right to left from the bottom.
fn wreath_layout(
    reader: &Reader,
    common: &Common,
    strings: &Strings,
    per_string: i32,
) -> Result<Local, SkipReason> {
    account(
        reader,
        super::BOXED_PLACEMENT,
        &[WREATH],
        super::NO_CHILDREN,
    )?;
    let lights = strings.nodes as i32;
    if lights == 0 {
        return Err(SkipReason::NoPixels);
    }
    let offset = lights / 2;
    let radius = f64::from(offset);
    let btt = common.bottom_to_top;
    let mut pct = if btt { 0.5 } else { 0.0 };
    let mut step = 1.0 / f64::from(lights);
    if common.left_to_right != btt {
        step = -step;
    }
    let size = lights + 1;
    let string_type = &common.string_type;
    let per_string = per_string.max(1) as u32;
    let mut nodes = Vec::with_capacity(lights as usize);
    for node in 0..lights as u32 {
        let x = (radius * (pct * 2.0 * PI).sin() + f64::from(offset) + 0.5) as i32;
        let y = (radius * (pct * 2.0 * PI).cos() + f64::from(offset) + 0.5) as i32;
        pct += step;
        if pct >= 1.0 {
            pct -= 1.0;
        }
        if pct < 0.0 {
            pct += 1.0;
        }
        nodes.push(Node {
            channel: strings.channel(string_type, node / per_string, node % per_string),
            lights: vec![DVec3::new(
                f64::from(x - size / 2),
                f64::from(y - size / 2),
                0.0,
            )],
            arc: None,
        });
    }
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        size: RenderSize { width: size as f32 },
        placement: placement::boxed(reader)?,
    })
}

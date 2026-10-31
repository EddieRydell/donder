//! Candy canes: an upright and a hook of lights, side by side.

use super::{Common, Local, Node, Span, Strings, account};
use crate::report::SkipReason;
use crate::xlights::placement::{self, RenderSize};
use crate::xlights::read::{Name, Reader, Table, Use};
use glam::DVec3;
use std::f64::consts::PI;

const CANES: Table = &[
    (Name::Exact("NumCanes"), Use::Modeled),
    (Name::Exact("NodesPerCane"), Use::Modeled),
    (Name::Exact("LightsPerNode"), Use::Modeled),
    (Name::Exact("parm1"), Use::Modeled),
    (Name::Exact("parm2"), Use::Modeled),
    (Name::Exact("parm3"), Use::Modeled),
    (Name::Exact("CandyCaneReverse"), Use::Modeled),
    (Name::Exact("CandyCaneSticks"), Use::Modeled),
    (Name::Exact("AlternateNodes"), Use::Modeled),
    (Name::Exact("CandyCaneSkew"), Use::Modeled),
    (Name::Exact("CandyCaneHeight"), Use::Modeled),
    (
        Name::Exact("Shear"),
        Use::Irrelevant("xLights shears only icicles"),
    ),
];

struct Settings {
    canes: i32,
    per_cane: i32,
    lights_per_node: i32,
    reverse: bool,
    sticks: bool,
    alternate: bool,
    skew: i32,
    cane_height: f32,
    height: f32,
}

/// xLights: `XmlDeserializingModelFactory::DeserializeCandyCane` and
/// `DeserializeThreePointScreenLocationAttributes`.
fn settings(reader: &Reader) -> Result<Settings, SkipReason> {
    let mut skew = reader.as_int("Angle", 0);
    if reader.has("CandyCaneSkew") {
        skew = reader.as_int("CandyCaneSkew", 0);
    }
    Ok(Settings {
        canes: reader.parm("NumCanes", "parm1", "1"),
        per_cane: reader.parm("NodesPerCane", "parm2", "1"),
        lights_per_node: reader.parm("LightsPerNode", "parm3", "1"),
        reverse: reader.equals("CandyCaneReverse", "false", "true"),
        sticks: reader.equals("CandyCaneSticks", "false", "true"),
        alternate: reader.equals("AlternateNodes", "false", "true"),
        skew,
        cane_height: reader.as_float("CandyCaneHeight", 1.0)?,
        height: reader.as_float("Height", 1.0)?,
    })
}

pub(super) fn candy_canes(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let settings = match settings(reader) {
        Ok(settings) => settings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    // xLights: CandyCaneModel::CalcChannelsPerString, which takes no ts.
    let strings = match Strings::uniform(settings.canes, settings.per_cane, 1) {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let span = Ok(strings.span(&common.string_type));
    (span, layout(reader, common, &settings, &strings))
}

/// xLights: `rotate_point` in `CandyCaneModel.cpp`, about `(cx, 0)`.
fn rotate_point(cx: f32, angle: f32, x: &mut f32, y: &mut f32) {
    let (s, c) = (angle.sin(), angle.cos());
    let (px, py) = (*x - cx, *y);
    *x = px * c - py * s + cx;
    *y = px * s + py * c;
}

/// xLights: `CandyCaneModel::InitModel` and `SetCaneCoord`: each cane's
/// lights up its upright, two thirds of them, then round its hook; or, as
/// sticks, all straight up. Numbered right to left, the canes' channels run
/// from the last cane.
fn layout(
    reader: &Reader,
    common: &Common,
    settings: &Settings,
    strings: &Strings,
) -> Result<Local, SkipReason> {
    account(
        reader,
        super::THREE_POINT_PLACEMENT,
        &[CANES],
        super::NO_CHILDREN,
    )?;
    let (canes, per_cane, lights) = (settings.canes, settings.per_cane, settings.lights_per_node);
    if canes < 1 || per_cane < 1 || lights < 1 {
        return Err(SkipReason::NoPixels);
    }
    // Each node's buffer row. xLights: InitModel.
    let row = |x: i32| -> i32 {
        if settings.alternate {
            if x < (per_cane + 1) / 2 {
                2 * x
            } else {
                (per_cane - (x + 1)) * 2 + 1
            }
        } else {
            x
        }
    };
    // The node at a cane's buffer row. xLights: Model::FindNodeAtXY.
    let node_at = |cane: i32, at: i32| {
        (0..per_cane)
            .find(|x| row(*x) == at)
            .map(|x| (cane * per_cane + x) as usize)
    };
    super::lights(u64::from(strings.nodes), lights as u64)?;
    let string_type = &common.string_type;
    let mut nodes: Vec<Node> = (0..canes)
        .flat_map(|cane| {
            let string = if common.left_to_right {
                cane
            } else {
                canes - 1 - cane
            } as u32;
            (0..per_cane).map(move |x| (string, x))
        })
        .map(|(string, x)| Node {
            channel: strings.channel(string_type, string, x as u32),
            lights: vec![DVec3::ZERO; lights as usize],
            arc: None,
        })
        .collect();
    let light_count = per_cane * lights;
    let angle = (2.0 * PI * f64::from(settings.skew as f32) / 360.0) as f32;
    let height = f64::from(settings.height);
    let upright = ((f64::from(per_cane) * 6.0 / 9.0) as i32) * lights;
    let width_per_cane = f64::from(light_count) * 3.0 / 9.0;
    let gap = 2.0;
    let width = f64::from(canes) * width_per_cane + f64::from(canes - 1) * gap;
    let set = |nodes: &mut [Node], node: Option<usize>, c: usize, x: f32, y: f32, about: f32| {
        if let Some(node) = node {
            let (mut sx, mut sy) = (x, y);
            rotate_point(about, angle, &mut sx, &mut sy);
            nodes[node].lights[c] = DVec3::new(f64::from(sx), f64::from(sy), 0.0);
        }
    };
    if settings.sticks {
        for cane in 0..canes {
            let mut y = 0i32;
            let x = f64::from(cane) * (width_per_cane + gap) + width_per_cane / 2.0;
            for _ in 0..per_cane {
                let node = node_at(cane, y / lights);
                for c in 0..lights as usize {
                    let sy = settings.cane_height * y as f32 * settings.height;
                    set(&mut nodes, node, c, x as f32, sy, x as f32);
                    y += 1;
                }
            }
        }
    } else {
        let arc_lights = light_count - upright;
        for cane in 0..canes {
            let mut x = f64::from(cane) * (width_per_cane + gap);
            if settings.reverse {
                x += width_per_cane;
            }
            let mut y = 0f64;
            let mut light = 0;
            let cx = if settings.reverse {
                x - width_per_cane / 2.0 * height
            } else {
                x + width_per_cane / 2.0 * height
            };
            let ox = x;
            while light < upright {
                let node = node_at(cane, (y / f64::from(lights)) as i32);
                for c in 0..lights as usize {
                    let sy = settings.cane_height * y as f32 * settings.height;
                    set(&mut nodes, node, c, x as f32, sy, x as f32);
                    y += 1.0;
                    light += 1;
                }
            }
            y -= 1.0;
            x = cx;
            while light < light_count {
                let node = node_at(cane, light / lights);
                for c in 0..lights as usize {
                    let hook = PI - PI * f64::from(light - upright + 1) / f64::from(arc_lights);
                    let y2 = hook.sin() * width_per_cane / 2.0 * height;
                    let x2 = hook.cos() * width_per_cane / 2.0 * height;
                    let sx = if settings.reverse { x - x2 } else { x + x2 };
                    let sy = settings.cane_height * (y * height + y2) as f32;
                    set(&mut nodes, node, c, sx as f32, sy, ox as f32);
                    light += 1;
                }
            }
        }
    }
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        size: RenderSize {
            width: width as f32,
        },
        placement: placement::three_point(reader, true, false)?,
    })
}

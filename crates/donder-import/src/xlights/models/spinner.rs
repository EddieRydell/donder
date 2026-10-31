//! Spinners: arms of lights out from a centre.

use super::{Common, Local, Node, Span, Strings, account};
use crate::report::SkipReason;
use crate::xlights::placement::{self, RenderSize};
use crate::xlights::read::{Name, Reader, Table, Use};
use glam::DVec3;
use std::f64::consts::PI;

const SPINNER: Table = &[
    (Name::Exact("NumStrings"), Use::Modeled),
    (Name::Exact("NodesPerArm"), Use::Modeled),
    (Name::Exact("ArmsPerString"), Use::Modeled),
    (Name::Exact("parm1"), Use::Modeled),
    (Name::Exact("parm2"), Use::Modeled),
    (Name::Exact("parm3"), Use::Modeled),
    (Name::Exact("Hollow"), Use::Modeled),
    (Name::Exact("StartAngle"), Use::Modeled),
    (Name::Exact("Arc"), Use::Modeled),
    (Name::Exact("ZigZag"), Use::Modeled),
    (Name::Exact("Alternate"), Use::Modeled),
];

struct Settings {
    strings: i32,
    nodes_per_arm: i32,
    arms_per_string: i32,
    hollow: i32,
    start_angle: i32,
    arc: i32,
    zig_zag: bool,
    alternate: bool,
}

/// xLights: `XmlDeserializingModelFactory::DeserializeSpinner`.
fn settings(reader: &Reader) -> Settings {
    Settings {
        strings: reader.parm("NumStrings", "parm1", "1"),
        nodes_per_arm: reader.parm("NodesPerArm", "parm2", "1"),
        arms_per_string: reader.parm("ArmsPerString", "parm3", "1"),
        hollow: reader.as_int("Hollow", 20),
        start_angle: reader.as_int("StartAngle", 0),
        arc: reader.as_int("Arc", 360),
        zig_zag: reader.equals("ZigZag", "false", "true"),
        alternate: reader.equals("Alternate", "false", "true"),
    }
}

pub(super) fn spinner(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let settings = settings(reader);
    let per_string = i64::from(settings.nodes_per_arm) * i64::from(settings.arms_per_string);
    // xLights: SpinnerModel::CalcChannelsPerString, which takes no ts.
    let strings = match i32::try_from(per_string)
        .map_err(|_| SkipReason::TooManyPixels)
        .and_then(|per_string| Strings::uniform(settings.strings, per_string, 1))
    {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let span = Ok(strings.span(&common.string_type));
    (span, layout(reader, common, &settings, &strings))
}

/// xLights: `SpinnerModel::InitModel` and `SetSpinnerCoord`: arms spread
/// over the arc from the start angle, counter-clockwise unless numbered
/// right to left, each arm's lights out from the hollow middle, or in from
/// the end.
fn layout(
    reader: &Reader,
    common: &Common,
    settings: &Settings,
    strings: &Strings,
) -> Result<Local, SkipReason> {
    account(
        reader,
        super::BOXED_PLACEMENT,
        &[SPINNER],
        super::NO_CHILDREN,
    )?;
    let (per_arm, arms_per_string) = (settings.nodes_per_arm, settings.arms_per_string);
    if per_arm < 1 || arms_per_string < 1 || settings.strings < 1 {
        return Err(SkipReason::NoPixels);
    }
    let arms = arms_per_string * settings.strings;
    let mut angle = (std::f32::consts::PI * 2.0 * (270.0 + settings.start_angle as f32)) / 360.0;
    let mut increment = ((PI * 2.0 * f64::from(settings.arc as f32))
        / f64::from(settings.strings as f32 * arms_per_string as f32 * 360.0))
        as f32;
    if settings.arc < 360 && arms > 1 {
        increment = ((PI * 2.0 * f64::from(settings.arc as f32))
            / f64::from((settings.strings as f32 * arms_per_string as f32 - 1.0) * 360.0))
            as f32;
    }
    let clockwise = !common.left_to_right;
    let from_centre = !common.bottom_to_top;
    let channels = common.string_type.channels;
    let hollow = f64::from(settings.hollow as f32) * 2.0 * f64::from(per_arm as f32) / 100.0;
    let mut nodes = Vec::with_capacity(strings.nodes as usize);
    for arm in 0..arms {
        let string = (arm / arms_per_string) as u32;
        let segment = (arm % arms_per_string) as u32;
        for n in 0..per_arm {
            let along = if settings.alternate {
                if n < (per_arm + 1) / 2 {
                    2 * n
                } else {
                    (per_arm - (n + 1)) * 2 + 1
                }
            } else {
                let mut along = if from_centre { n } else { per_arm - n - 1 };
                if settings.zig_zag && arm % 2 > 0 {
                    along = if from_centre { per_arm - n - 1 } else { n };
                }
                along
            };
            let radius = f64::from(0.5f32 + along as f32) + hollow;
            nodes.push(Node {
                channel: strings.channel(&common.string_type, string, 0)
                    + (segment * per_arm as u32 + n as u32) * channels,
                lights: vec![DVec3::new(
                    f64::from((radius * f64::from(angle.cos())) as f32),
                    f64::from((radius * f64::from(angle.sin())) as f32),
                    0.0,
                )],
                arc: None,
            });
        }
        if clockwise {
            angle -= increment;
        } else {
            angle += increment;
        }
    }
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        size: RenderSize { width: 1.0 },
        placement: placement::boxed(reader)?,
    })
}

//! Window frames: one string around a rectangle.

use super::{Common, Local, Node, Span, Strings, account};
use crate::placement::{self, RenderSize};
use crate::read::{Name, Reader, Table, Use};
use crate::report::SkipReason;
use glam::DVec3;

const WINDOW: Table = &[
    (Name::Exact("TopNodes"), Use::Modeled),
    (Name::Exact("SideNodes"), Use::Modeled),
    (Name::Exact("BottomNodes"), Use::Modeled),
    (Name::Exact("parm1"), Use::Modeled),
    (Name::Exact("parm2"), Use::Modeled),
    (Name::Exact("parm3"), Use::Modeled),
    (Name::Exact("Rotation"), Use::Modeled),
];

struct Settings {
    top: i32,
    side: i32,
    bottom: i32,
    clockwise: bool,
}

/// xLights: `XmlDeserializingModelFactory::DeserializeWindow`.
fn settings(reader: &Reader) -> Settings {
    let rotation = reader.as_string("Rotation", "CW");
    Settings {
        top: reader.parm("TopNodes", "parm1", "0"),
        side: reader.parm("SideNodes", "parm2", "0"),
        bottom: reader.parm("BottomNodes", "parm3", "0"),
        clockwise: rotation == "Clockwise" || rotation == "CW",
    }
}

pub(super) fn window_frame(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let settings = settings(reader);
    let total = i64::from(settings.top) + 2 * i64::from(settings.side) + i64::from(settings.bottom);
    let strings = match i32::try_from(total)
        .map_err(|_| SkipReason::TooManyPixels)
        .and_then(|total| Strings::uniform(1, total, 1))
    {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let span = Ok(strings.span(&common.string_type));
    (span, layout(reader, common, &settings, &strings))
}

/// xLights: `WindowFrameModel::InitFrame`: one string up the sides and
/// along the top and bottom from the corner the start side and direction
/// name. For the corners and rotations where xLights adjusts the lengths
/// (`wadj`, `hadj`), the corner lights belong to the top and bottom.
fn layout(
    reader: &Reader,
    common: &Common,
    settings: &Settings,
    strings: &Strings,
) -> Result<Local, SkipReason> {
    account(
        reader,
        super::BOXED_PLACEMENT,
        &[WINDOW],
        super::NO_CHILDREN,
    )?;
    if settings.top < 0 || settings.bottom < 0 {
        return Err(SkipReason::Setting(
            "negative number of top or bottom lights",
        ));
    }
    if settings.side < 2 {
        return Err(SkipReason::Setting("side with fewer than two lights"));
    }
    let (left, top, bottom) = (settings.side, settings.top, settings.bottom);
    let width = top.max(bottom) + 2;
    let height = settings.side;
    let (ltr, btt) = (common.left_to_right, common.bottom_to_top);
    let dir: f32 = if settings.clockwise { 1.0 } else { -1.0 };
    let corners_on_rows = if dir == -1.0 { btt == ltr } else { btt != ltr };
    let (wadj, hadj) = if corners_on_rows { (2, -2) } else { (0, 0) };
    let w = width as f32;
    let top_screen = if top + wadj - 1 != 0 {
        w / (top + 1) as f32
    } else {
        1.0
    };
    let bottom_screen = if bottom + wadj - 1 != 0 {
        -w / (bottom + 1) as f32
    } else {
        1.0
    };
    let lengths = [left + hadj, top + wadj, left + hadj, bottom + wadj];
    let x_screen_incr = [0.0, top_screen, 0.0, bottom_screen];
    let y_screen_incr = [1.0, 0.0, -1.0, 0.0];
    let half_h = (height - 1) as f32 / 2.0;
    // Where each side's first light is drawn.
    let (x_screen, y_screen): ([f32; 4], [f32; 4]) = if dir == -1.0 {
        if btt == ltr {
            let (a, b) = (-w / 2.0, w / 2.0);
            ([a, b, b, a], [half_h - 1.0, half_h, -half_h + 1.0, -half_h])
        } else {
            let a = -w / 2.0;
            (
                [a, w / 2.0 - top_screen, w / 2.0, a - bottom_screen],
                [half_h, half_h, -half_h, -half_h],
            )
        }
    } else if btt != ltr {
        let (a, b) = (-w / 2.0, w / 2.0);
        ([a, a, b, b], [-half_h + 1.0, half_h, half_h - 1.0, -half_h])
    } else {
        (
            [
                -w / 2.0,
                -w / 2.0 + top_screen,
                w / 2.0,
                w / 2.0 + bottom_screen,
            ],
            [-half_h, half_h, half_h, -half_h],
        )
    };
    let indexes = match (ltr, btt, dir == 1.0) {
        (true, true, true) => [0, 1, 2, 3],
        (true, true, false) => [3, 2, 1, 0],
        (true, false, true) => [1, 2, 3, 0],
        (true, false, false) => [0, 3, 2, 1],
        (false, true, true) => [3, 0, 1, 2],
        (false, true, false) => [2, 1, 0, 3],
        (false, false, true) => [2, 3, 0, 1],
        (false, false, false) => [1, 0, 3, 2],
    };
    if lengths.iter().all(|length| *length <= 0) {
        return Err(SkipReason::NoPixels);
    }
    let mut side = 0;
    while lengths[indexes[side]] == 0 {
        side += 1;
    }
    let mut screen_x = x_screen[indexes[side]];
    let mut screen_y = y_screen[indexes[side]];
    let mut length = lengths[indexes[side]];
    let channels = common.string_type.channels;
    let mut nodes = Vec::with_capacity(strings.nodes as usize);
    for index in 0..strings.nodes {
        if length <= 0 {
            return Err(SkipReason::Unplaceable("a side with no lights"));
        }
        nodes.push(Node {
            channel: index * channels,
            lights: vec![DVec3::new(f64::from(screen_x), f64::from(screen_y), 0.0)],
            arc: None,
        });
        screen_x += x_screen_incr[indexes[side]] * dir;
        screen_y += y_screen_incr[indexes[side]] * dir;
        length -= 1;
        if length <= 0 {
            side = (side + 1) % 4;
            while lengths[indexes[side]] == 0 {
                side = (side + 1) % 4;
            }
            screen_x = x_screen[indexes[side]];
            screen_y = y_screen[indexes[side]];
            length = lengths[indexes[side]];
        }
    }
    Ok(Local {
        nodes,
        ellipses: Vec::new(),
        size: RenderSize { width: w },
        placement: placement::boxed(reader)?,
    })
}

//! Matrices, trees and spheres: strands of lights in a grid, as xLights'
//! `MatrixModel` numbers them, placed flat, on a cone or on a sphere.

use super::{ArcPoint, Common, Ellipse, Local, Node, Span, Strings, account};
use crate::placement::{self, Placement, RenderSize};
use crate::read::{Name, Reader, Table, Use, strtol};
use crate::report::SkipReason;
use glam::{DVec3, Vec3};
use std::f64::consts::PI;

const GRID: Table = &[
    (Name::Exact("NumStrings"), Use::Modeled),
    (Name::Exact("NodesPerString"), Use::Modeled),
    (Name::Exact("StrandsPerString"), Use::Modeled),
    (Name::Exact("parm1"), Use::Modeled),
    (Name::Exact("parm2"), Use::Modeled),
    (Name::Exact("parm3"), Use::Modeled),
    (Name::Exact("AlternateNodes"), Use::Modeled),
    (Name::Exact("NoZig"), Use::Modeled),
];

const MATRIX: Table = &[
    (Name::Exact("Vertical"), Use::Modeled),
    (
        Name::Exact("exportFirstStrand"),
        Use::Irrelevant("xLights reads it only for trees"),
    ),
];

const TREE: Table = &[
    (Name::Exact("StrandDir"), Use::Modeled),
    (Name::Exact("exportFirstStrand"), Use::RefusedUnlessDefault),
    (Name::Exact("TreeRotation"), Use::Modeled),
    (Name::Exact("TreeSpiralRotations"), Use::Modeled),
    (Name::Exact("TreeBottomTopRatio"), Use::Modeled),
    (Name::Exact("TreeType"), Use::Modeled),
    (Name::Exact("TreeDegrees"), Use::Modeled),
    (
        Name::Exact("TreePerspective"),
        Use::Irrelevant("the lean of a tree in xLights' 2D preview"),
    ),
];

const SPHERE: Table = &[
    (Name::Exact("StartLatitude"), Use::Modeled),
    (Name::Exact("EndLatitude"), Use::Modeled),
    (Name::Exact("Degrees"), Use::Modeled),
    (
        Name::Exact("exportFirstStrand"),
        Use::Irrelevant("xLights reads it only for trees"),
    ),
];

/// A matrix's counts. xLights: `MatrixModel::InitVMatrix` and `InitHMatrix`,
/// which clamp the strands per string and round each string down to whole
/// strands.
struct Counts {
    strings: i32,
    nodes_per_string: i32,
    strands: i32,
    per_strand: i32,
}

fn counts(reader: &Reader) -> Result<Counts, SkipReason> {
    let strings = reader.parm("NumStrings", "parm1", "1");
    let nodes_per_string = reader.parm("NodesPerString", "parm2", "1");
    if nodes_per_string < 1 {
        return Err(SkipReason::NoPixels);
    }
    let strands = reader
        .parm("StrandsPerString", "parm3", "1")
        .clamp(1, nodes_per_string);
    Ok(Counts {
        strings,
        nodes_per_string,
        strands,
        per_strand: nodes_per_string / strands,
    })
}

/// The channels of a matrix: strings spaced by the nodes per string as
/// written, with `ts`, each holding whole strands. xLights:
/// `MatrixModel::NodesPerString` and `SetNodeCount`.
fn strings(counts: &Counts, common: &Common) -> Result<Strings, SkipReason> {
    let mut strings = Strings::uniform(
        counts.strings,
        counts.per_strand * counts.strands,
        common.ts,
    )?;
    let ts = if common.string_type.single_node {
        1
    } else {
        common.ts
    };
    strings.nodes_per_string = if ts <= 1 {
        counts.nodes_per_string as u32
    } else {
        (counts.nodes_per_string as u32).saturating_mul(ts as u32)
    };
    Ok(strings)
}

/// A node of the grid: its channel and its buffer cell.
struct Cell {
    channel: u32,
    x: i32,
    y: i32,
}

/// The grid, in xLights' node order. xLights: `InitVMatrix` and
/// `InitHMatrix` for individual pixels, without the first exported strand.
fn grid(
    counts: &Counts,
    strings: &Strings,
    common: &Common,
    vertical: bool,
    alternate: bool,
    no_zig: bool,
) -> (Vec<Cell>, i32, i32) {
    let strands = counts.strings * counts.strands;
    let per_strand = counts.per_strand;
    let per_string = per_strand * counts.strands;
    let channels = common.string_type.channels as i64;
    let mut cells: Vec<Option<Cell>> = (0..strands * per_strand).map(|_| None).collect();
    let (ltr, btt) = (common.left_to_right, common.bottom_to_top);
    for strand in 0..strands {
        let string = strand / counts.strands;
        let segment = strand % counts.strands;
        for index in 0..per_strand {
            let node = (string * per_string + segment * per_strand + index) as usize;
            let channel = strings.channel(&common.string_type, string as u32, 0) as i64
                + i64::from(segment * per_strand) * channels
                + i64::from(index) * channels;
            let along = |forward: bool| -> i32 {
                if alternate {
                    let half = index < (per_strand + 1) / 2;
                    let row = if half {
                        index * 2
                    } else {
                        (per_strand - (index + 1)) * 2 + 1
                    };
                    if forward { row } else { per_strand - 1 - row }
                } else if no_zig {
                    if forward {
                        index
                    } else {
                        per_strand - index - 1
                    }
                } else if forward == (segment % 2 == 0) {
                    index
                } else {
                    per_strand - index - 1
                }
            };
            let cell = if vertical {
                Cell {
                    channel: channel as u32,
                    x: if ltr { strand } else { strands - strand - 1 },
                    y: along(btt),
                }
            } else {
                Cell {
                    channel: channel as u32,
                    x: along(ltr),
                    y: if btt { strand } else { strands - strand - 1 },
                }
            };
            cells[node] = Some(cell);
        }
    }
    let (width, height) = if vertical {
        (strands, per_strand)
    } else {
        (per_strand, strands)
    };
    (cells.into_iter().flatten().collect(), width, height)
}

fn boxed_local(
    reader: &Reader,
    nodes: Vec<Node>,
    ellipses: Vec<Ellipse>,
) -> Result<Local, SkipReason> {
    Ok(Local {
        nodes,
        ellipses,
        size: RenderSize { width: 1.0 },
        placement: placement::boxed(reader)?,
    })
}

fn flags(reader: &Reader) -> (bool, bool) {
    (
        reader.equals("AlternateNodes", "false", "true"),
        reader.equals("NoZig", "false", "true"),
    )
}

/// xLights: `Model::CopyBufCoord2ScreenCoord`.
fn centered(cell: &Cell, width: i32, height: i32) -> DVec3 {
    DVec3::new(
        f64::from(cell.x - width / 2),
        f64::from(cell.y - height / 2),
        0.0,
    )
}

pub(super) fn matrix(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let counts = match counts(reader) {
        Ok(counts) => counts,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let strings = match strings(&counts, common) {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let span = Ok(strings.span(&common.string_type));
    // xLights: DeserializeMatrix.
    let vertical = reader.as_string("DisplayAs", "Matrix") == "Vert Matrix"
        || reader.equals("Vertical", "false", "true");
    let local = account(
        reader,
        super::BOXED_PLACEMENT,
        &[GRID, MATRIX],
        super::NO_CHILDREN,
    )
    .and_then(|()| {
        let (alternate, no_zig) = flags(reader);
        let (cells, width, height) = grid(&counts, &strings, common, vertical, alternate, no_zig);
        let nodes = cells
            .iter()
            .map(|cell| Node {
                channel: cell.channel,
                lights: vec![centered(cell, width, height)],
                arc: None,
            })
            .collect();
        boxed_local(reader, nodes, Vec::new())
    });
    (span, local)
}

/// xLights: `toRadians` in `UtilFunctions.h`.
fn to_radians(degrees: f32) -> f64 {
    2.0 * PI * f64::from(degrees) / 360.0
}

pub(super) fn tree(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let counts = match counts(reader) {
        Ok(counts) => counts,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let strings = match strings(&counts, common) {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let span = Ok(strings.span(&common.string_type));
    (span, tree_layout(reader, common, &counts, &strings))
}

/// xLights: `DeserializeTree`, `TreeModel::InitModel` and `SetTreeCoord`.
fn tree_layout(
    reader: &Reader,
    common: &Common,
    counts: &Counts,
    strings: &Strings,
) -> Result<Local, SkipReason> {
    account(
        reader,
        super::BOXED_PLACEMENT,
        &[GRID, TREE],
        super::NO_CHILDREN,
    )?;
    let vertical = reader.equals("StrandDir", "Vertical", "Vertical");
    // xLights: a first exported strand above 1 on a vertical tree drops the
    // model's start channel (MatrixModel::InitVMatrix).
    if vertical && reader.as_int("exportFirstStrand", 0) - 1 > 0 {
        return Err(SkipReason::Setting("first exported strand"));
    }
    let display = reader.as_string("DisplayAs", "Tree");
    let mut tree_type = 0;
    let mut degrees: i64 = 360;
    if display != "Tree" {
        if let Some((_, token)) = display.split_once(' ') {
            match token {
                "Flat" => {
                    tree_type = 1;
                    degrees = 0;
                }
                "Ribbon" => {
                    tree_type = 2;
                    degrees = -1;
                }
                other => degrees = strtol(other),
            }
        }
    } else {
        tree_type = reader.as_int("TreeType", 0);
        degrees = i64::from(reader.as_int("TreeDegrees", 360));
    }
    let degrees = match tree_type {
        1 => 0,
        2 => -1,
        _ => degrees,
    };
    let rotation = reader.as_float("TreeRotation", 3.0)?;
    let spiral = reader.as_float("TreeSpiralRotations", 0.0)?;
    let ratio = reader.as_float("TreeBottomTopRatio", 6.0)?;
    let (alternate, no_zig) = flags(reader);
    let (cells, width, height) = grid(counts, strings, common, vertical, alternate, no_zig);
    if width < 1 || height < 1 {
        return Err(SkipReason::NoPixels);
    }
    let (wi, ht) = (f64::from(width), f64::from(height));
    let mut nodes = Vec::with_capacity(cells.len());
    let mut ellipses = Vec::new();
    if degrees > 0 {
        let render_ht = ht * 3.0;
        let render_wi = render_ht / 1.8;
        let radians = to_radians(degrees as f32);
        let mut radius = render_wi / 2.0;
        let mut top = radius;
        if ratio != 0.0 {
            top = radius / f64::from(ratio.abs());
        }
        if ratio < 0.0 {
            std::mem::swap(&mut top, &mut radius);
        }
        let mut start = -radians / 2.0;
        let mut increment = radians / wi;
        if degrees < 350 && width > 1 {
            increment = radians / (wi - 1.0);
        }
        start += to_radians(rotation);
        let rows = height as usize;
        let mut y_pos: Vec<f32> = (0..rows).map(|row| row as f32).collect();
        let mut x_inc = vec![0f32; rows];
        if spiral != 0.0 {
            let gap = ((radius - top) / 10.0) as f32;
            let mut lengths = [0f32; 10];
            let mut total = 0f32;
            for (index, length) in lengths.iter_mut().enumerate() {
                *length = (2.0 * PI * (radius - f64::from(gap) * index as f64)
                    - f64::from(gap) / 2.0) as f32;
                *length = (f64::from(*length) * (f64::from(spiral) / 10.0)) as f32;
                let rise = f64::from(height as f32) / 10.0 * f64::from(height as f32) / 10.0;
                *length = (f64::from(*length) * f64::from(*length) + rise).sqrt() as f32;
                total += *length;
            }
            for length in &mut lengths {
                *length /= total;
            }
            let mut segment = 0usize;
            let mut in_segment = (lengths[0] * height as f32).round();
            let mut light = 0f32;
            for row in 1..rows {
                if light >= in_segment {
                    segment += 1;
                    light = 0.0;
                    in_segment = if segment == 9 {
                        (rows - row) as f32
                    } else {
                        (lengths[segment] * height as f32).round()
                    };
                }
                let mut angle = (f64::from(spiral) * 2.0 * PI / 10.0) as f32;
                angle /= in_segment;
                y_pos[row] =
                    (f64::from(y_pos[row - 1]) + (ht / 10.0 / f64::from(in_segment))) as f32;
                x_inc[row] = x_inc[row - 1] + angle;
                light += 1.0;
            }
        }
        for cell in &cells {
            let angle = start + f64::from(cell.x) * increment + f64::from(x_inc[cell.y as usize]);
            let (xb, xt) = (radius * angle.sin(), top * angle.sin());
            let (zb, zt) = (radius * angle.cos(), top * angle.cos());
            let along = if height > 1 {
                f64::from(y_pos[cell.y as usize]) / (ht - 1.0)
            } else {
                0.5
            };
            nodes.push(Node {
                channel: cell.channel,
                lights: vec![DVec3::new(
                    f64::from((xb + (xt - xb) * along) as f32),
                    f64::from((render_ht * along - render_ht / 2.0) as f32),
                    f64::from((zb + (zt - zb) * along) as f32),
                )],
                arc: None,
            });
        }
        // Each row of a tree without a spiral is a ring, or part of one.
        if spiral == 0.0 && ratio == 1.0 {
            for row in 0..height {
                let along = if height > 1 {
                    f64::from(row) / (ht - 1.0)
                } else {
                    0.5
                };
                ellipses.push(Ellipse {
                    center: DVec3::new(0.0, render_ht * along - render_ht / 2.0, 0.0),
                    u: DVec3::new(0.0, 0.0, radius),
                    v: DVec3::new(radius, 0.0, 0.0),
                });
            }
            for (node, cell) in nodes.iter_mut().zip(&cells) {
                let angle = start + f64::from(cell.x) * increment;
                node.arc = Some(ArcPoint {
                    arc: cell.y as usize,
                    degrees: angle.to_degrees(),
                });
            }
        }
    } else {
        let scale = if degrees == -1 { 5.0 } else { 4.0 };
        let render_ht = ht * 2.0;
        for cell in &cells {
            let (bx, by) = (f64::from(cell.x), f64::from(cell.y));
            let xt = (bx + 0.5 - wi / 2.0) * 0.9;
            let xb = (bx + 0.5 - wi / 2.0) * scale;
            let mut lights = Vec::new();
            if degrees == -1 {
                // xLights: a ribbon tree's node is three lights.
                let h = (render_ht * render_ht + (xt - xb) * (xt - xb)).sqrt();
                for (offset, fallback) in [(0.0, 0.5), (-0.33, 0.0), (0.33, 1.0)] {
                    let along = if height > 1 {
                        (by + offset) / (ht - 1.0)
                    } else {
                        fallback
                    };
                    let rise = render_ht * along;
                    lights.push(DVec3::new(
                        f64::from((xb + (xt - xb) * along) as f32),
                        f64::from((render_ht * rise / h - render_ht / 2.0) as f32),
                        0.0,
                    ));
                }
            } else {
                let along = if height > 1 { by / (ht - 1.0) } else { 0.5 };
                lights.push(DVec3::new(
                    f64::from((xb + (xt - xb) * along) as f32),
                    f64::from((render_ht * along - render_ht / 2.0) as f32),
                    0.0,
                ));
            }
            nodes.push(Node {
                channel: cell.channel,
                lights,
                arc: None,
            });
        }
    }
    boxed_local(reader, nodes, ellipses)
}

pub(super) fn sphere(
    reader: &Reader,
    common: &Common,
) -> (Result<Span, SkipReason>, Result<Local, SkipReason>) {
    let counts = match counts(reader) {
        Ok(counts) => counts,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let strings = match strings(&counts, common) {
        Ok(strings) => strings,
        Err(reason) => return (Err(reason.clone()), Err(reason)),
    };
    let span = Ok(strings.span(&common.string_type));
    (span, sphere_layout(reader, common, &counts, &strings))
}

/// xLights: `DeserializeSphere`, `SphereModel::InitModel` and
/// `SetSphereCoord`.
fn sphere_layout(
    reader: &Reader,
    common: &Common,
    counts: &Counts,
    strings: &Strings,
) -> Result<Local, SkipReason> {
    account(
        reader,
        super::BOXED_PLACEMENT,
        &[GRID, SPHERE],
        super::NO_CHILDREN,
    )?;
    let start_latitude = reader.as_int("StartLatitude", -86);
    let end_latitude = reader.as_int("EndLatitude", 86);
    let degrees = reader.as_int("Degrees", 360);
    let (alternate, no_zig) = flags(reader);
    let (cells, width, height) = grid(counts, strings, common, true, alternate, no_zig);
    if width < 1 || height < 1 {
        return Err(SkipReason::NoPixels);
    }
    let (wi, ht) = (f64::from(width), f64::from(height));
    let radius = f64::from(width.max(height)) / 1.8 / 2.0;
    let radians = to_radians(360.0);
    let remove = to_radians((360.0 - f64::from(degrees)) as f32);
    let fudge = to_radians(((360.0 - f64::from(degrees)) / wi) as f32);
    let h_start = radians / 4.0 + 0.003 - remove / 2.0;
    let h_step = (-radians + remove - fudge) / wi;
    let v_start = to_radians((start_latitude - 90) as f32);
    let v_step =
        (to_radians((-start_latitude) as f32) + to_radians(end_latitude as f32)) / (ht - 1.0);
    let mut nodes = Vec::with_capacity(cells.len());
    let mut ellipses = Vec::new();
    for cell in &cells {
        let h = h_start + f64::from(cell.x) * h_step;
        let v = v_start + f64::from(cell.y) * v_step;
        let sv = v.sin();
        nodes.push(Node {
            channel: cell.channel,
            lights: vec![DVec3::new(
                f64::from((radius * h.cos() * sv) as f32),
                f64::from((radius * v.cos()) as f32),
                f64::from((radius * h.sin() * sv) as f32),
            )],
            arc: None,
        });
    }
    // Each strand is a line of longitude.
    if height > 2 {
        for strand in 0..width {
            let h = h_start + f64::from(strand) * h_step;
            ellipses.push(Ellipse {
                center: DVec3::ZERO,
                u: DVec3::new(0.0, radius, 0.0),
                v: DVec3::new(radius * h.cos(), 0.0, radius * h.sin()),
            });
        }
        for (node, cell) in nodes.iter_mut().zip(&cells) {
            node.arc = Some(ArcPoint {
                arc: cell.x as usize,
                degrees: (v_start + f64::from(cell.y) * v_step).to_degrees(),
            });
        }
    }
    let mut local = boxed_local(reader, nodes, ellipses)?;
    // xLights: DeserializeSphere rescales spheres saved before version 8.
    let version = reader.as_string("versionNumber", "");
    if (version.is_empty() || strtol(version) < 8)
        && let Placement::Boxed { scale, .. } = &mut local.placement
    {
        let biggest = height.max(counts.strings * counts.strands);
        if biggest > 0 {
            let ratio = height as f32 / biggest as f32;
            // A negative scale is rescaled first, then made 1 by
            // BoxedScreenLocation::Init.
            let factor = Vec3::new(ratio / 1.8, ratio, ratio / 1.8);
            let written = Vec3::new(
                reader.as_float("ScaleX", 1.0)?,
                reader.as_float("ScaleY", 1.0)?,
                reader.as_float("ScaleZ", 1.0)?,
            );
            let rescaled = written * factor;
            *scale = Vec3::select(rescaled.cmplt(Vec3::ZERO), Vec3::ONE, rescaled);
        }
    }
    Ok(local)
}

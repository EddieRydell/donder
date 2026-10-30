//! Matrices, and the trees and spheres xLights builds from them.

use super::{ArcPoint, Ellipse, Local, Node, Spec};
use crate::placement::{self, Placement};
use crate::report::SkipReason;
use glam::DVec3;
use std::f64::consts::TAU;

/// A node of a matrix: its channels and its cell, counted from the bottom
/// left.
struct Cell {
    string: u32,
    offset: u32,
    x: u32,
    y: u32,
}

/// Cells across and up.
struct Grid {
    cells: Vec<Cell>,
    width: u32,
    height: u32,
    strings: u32,
    /// Each string's nodes: whole strands, as xLights counts them.
    string_nodes: u32,
}

/// Where a strand's node `index` of `count` sits along it, with the strand's
/// direction: alternating nodes fill every other place out and back.
fn along(index: u32, count: u32, forward: bool, alternate: bool) -> u32 {
    if alternate {
        let place = if index < count.div_ceil(2) {
            index * 2
        } else {
            (count - (index + 1)) * 2 + 1
        };
        if forward { place } else { count - 1 - place }
    } else if forward {
        index
    } else {
        count - 1 - index
    }
}

/// Strings of `strands` strands each. Vertical strands are columns, the
/// first at the left unless numbered right to left, running up from the
/// bottom unless the start side is the top, and back the other way on
/// every second strand of a string unless the matrix has no zig-zag.
/// Horizontal strands are rows, the same with the axes swapped.
fn grid(spec: &Spec, vertical: bool) -> Result<Grid, SkipReason> {
    let strings = spec.count("NumStrings", "parm1", 1)?;
    let written_nodes = spec.count("NodesPerString", "parm2", 1)?;
    let strands_per_string = spec
        .count("StrandsPerString", "parm3", 1)?
        .clamp(1, written_nodes.max(1));
    let strands = super::nodes(&[strings, strands_per_string])?;
    let strand_nodes = written_nodes / strands_per_string;
    // Like xLights, a string's nodes past its last whole strand are not
    // part of the model.
    let string_nodes = strands_per_string * strand_nodes;
    super::nodes(&[strands, strand_nodes])?;
    let alternate = spec.is_true("AlternateNodes", false);
    let zig_zag = !spec.is_true("NoZig", false);
    let left_to_right = spec.left_to_right();
    let bottom_to_top = spec.bottom_to_top(true);
    let mut cells = Vec::with_capacity((strands * strand_nodes) as usize);
    for strand in 0..strands {
        let string = strand / strands_per_string;
        let segment = strand % strands_per_string;
        for index in 0..strand_nodes {
            let offset = segment * strand_nodes + index;
            let cell = if vertical {
                let forward = if alternate || !zig_zag {
                    bottom_to_top
                } else {
                    bottom_to_top == (segment % 2 == 0)
                };
                Cell {
                    string,
                    offset,
                    x: if left_to_right {
                        strand
                    } else {
                        strands - strand - 1
                    },
                    y: along(index, strand_nodes, forward, alternate),
                }
            } else {
                let forward = if alternate || !zig_zag {
                    left_to_right
                } else {
                    left_to_right == (segment % 2 == 0)
                };
                Cell {
                    string,
                    offset,
                    x: along(index, strand_nodes, forward, alternate),
                    y: if bottom_to_top {
                        strand
                    } else {
                        strands - strand - 1
                    },
                }
            };
            cells.push(cell);
        }
    }
    let (width, height) = if vertical {
        (strands, strand_nodes)
    } else {
        (strand_nodes, strands)
    };
    // The strand xLights exports first carries the model's first channels.
    let first = spec.int("exportFirstStrand", 0)?.saturating_sub(1);
    if let Ok(first) = u32::try_from(first)
        && first > 0
        && first < strands
    {
        let total = strands * strand_nodes;
        let shift = first * strand_nodes;
        for cell in &mut cells {
            let node = cell.string * string_nodes + cell.offset;
            let node = (node + total - shift) % total;
            cell.string = node / string_nodes;
            cell.offset = node % string_nodes;
        }
    }
    Ok(Grid {
        cells,
        width,
        height,
        strings,
        string_nodes,
    })
}

fn local(grid: &Grid, nodes: Vec<Node>, ellipses: Vec<Ellipse>, placement: Placement) -> Local {
    Local {
        nodes,
        ellipses,
        strings: grid.strings,
        string_nodes: grid.string_nodes,
        placement,
    }
}

/// Cells one unit apart, centred on the model.
pub(super) fn matrix(spec: &Spec) -> Result<Local, SkipReason> {
    let vertical = spec.display == "Vert Matrix"
        || (spec.display == "Matrix" && spec.is_true("Vertical", false));
    let grid = grid(spec, vertical)?;
    let nodes = grid
        .cells
        .iter()
        .map(|cell| Node {
            string: cell.string,
            offset: cell.offset,
            position: DVec3::new(
                f64::from(cell.x) - f64::from(grid.width / 2),
                f64::from(cell.y) - f64::from(grid.height / 2),
                0.0,
            ),
            arc: None,
        })
        .collect();
    Ok(local(&grid, nodes, Vec::new(), placement::boxed(spec)?))
}

enum TreeShape {
    Round(f64),
    Flat,
    Ribbon,
}

fn tree_shape(spec: &Spec) -> Result<TreeShape, SkipReason> {
    match spec.display.strip_prefix("Tree ") {
        Some("Flat") => Ok(TreeShape::Flat),
        Some("Ribbon") => Ok(TreeShape::Ribbon),
        Some(degrees) => degrees
            .trim()
            .parse::<f64>()
            .map(TreeShape::Round)
            .map_err(|_| SkipReason::Type(spec.display.to_string())),
        None => match spec.int("TreeType", 0)? {
            1 => Ok(TreeShape::Flat),
            2 => Ok(TreeShape::Ribbon),
            _ => Ok(TreeShape::Round(spec.float("TreeDegrees", 360.0)?)),
        },
    }
}

/// How far up its strand each row is, and how far a spiral has turned it:
/// a spiralling strand is ten pieces, each with lights for its length.
fn spiral(height: u32, radius: f64, top: f64, rotations: f64) -> (Vec<f64>, Vec<f64>) {
    let mut up: Vec<f64> = (0..height).map(f64::from).collect();
    let mut turned = vec![0.0; height as usize];
    if rotations == 0.0 || height == 0 {
        return (up, turned);
    }
    let gap = (radius - top) / 10.0;
    let rise = f64::from(height) / 10.0;
    let lengths: Vec<f64> = (0..10)
        .map(|piece| {
            let around = (TAU * (radius - gap * f64::from(piece)) - gap / 2.0) * rotations / 10.0;
            (around * around + rise * rise).sqrt()
        })
        .collect();
    let total: f64 = lengths.iter().sum();
    let lights_in = |piece: usize, row: u32| {
        if piece == 9 {
            f64::from(height - row)
        } else {
            (lengths[piece] / total * f64::from(height)).round()
        }
    };
    // A piece too short for a light of its own has no rows; the last piece
    // takes every row left.
    let mut piece = 0;
    let mut in_piece = lights_in(0, 0);
    while in_piece == 0.0 {
        piece += 1;
        in_piece = lights_in(piece, 0);
    }
    let mut placed = 0.0;
    for row in 1..height {
        if placed >= in_piece {
            placed = 0.0;
            loop {
                piece += 1;
                in_piece = lights_in(piece, row);
                if in_piece > 0.0 {
                    break;
                }
            }
        }
        let row = row as usize;
        up[row] = up[row - 1] + rise / in_piece;
        turned[row] = turned[row - 1] + rotations * TAU / 10.0 / in_piece;
        placed += 1.0;
    }
    (up, turned)
}

/// A round tree spreads its strands around a cone `degrees` wide, three
/// times as tall as a strand's light count; flat and ribbon trees fan them
/// out in a plane.
pub(super) fn tree(spec: &Spec) -> Result<Local, SkipReason> {
    let vertical = spec
        .text("StrandDir")
        .is_none_or(|direction| direction == "Vertical");
    let grid = grid(spec, vertical)?;
    let (width, height) = (f64::from(grid.width), f64::from(grid.height));
    let fraction = |up: f64| {
        if grid.height > 1 {
            up / (height - 1.0)
        } else {
            0.5
        }
    };
    let position = |cell: &Cell,
                    shape: &TreeShape,
                    up: &[f64],
                    turned: &[f64],
                    geometry: &(f64, f64, f64, f64)| match shape {
        TreeShape::Round(_) => {
            let (radius, top, start, step) = *geometry;
            let angle = start + f64::from(cell.x) * step + turned[cell.y as usize];
            let t = fraction(up[cell.y as usize]);
            let tall = height * 3.0;
            DVec3::new(
                radius * angle.sin() + (top - radius) * angle.sin() * t,
                t * tall - tall / 2.0,
                radius * angle.cos() + (top - radius) * angle.cos() * t,
            )
        }
        TreeShape::Flat | TreeShape::Ribbon => {
            let spread = if matches!(shape, TreeShape::Ribbon) {
                5.0
            } else {
                4.0
            };
            let tall = height * 2.0;
            let column = f64::from(cell.x) + 0.5 - width / 2.0;
            let (top, bottom) = (column * 0.9, column * spread);
            let t = fraction(f64::from(cell.y));
            let y = if matches!(shape, TreeShape::Ribbon) {
                let slant = (tall * tall + (top - bottom) * (top - bottom)).sqrt();
                tall * tall * t / slant
            } else {
                tall * t
            };
            DVec3::new(bottom + (top - bottom) * t, y - tall / 2.0, 0.0)
        }
    };
    let shape = tree_shape(spec)?;
    let mut geometry = (0.0, 0.0, 0.0, 0.0);
    let (mut up, mut turned) = (
        (0..grid.height).map(f64::from).collect(),
        vec![0.0; grid.height as usize],
    );
    if let TreeShape::Round(degrees) = shape {
        let radius = height * 3.0 / 1.8 / 2.0;
        let ratio = spec.float("TreeBottomTopRatio", 6.0)?;
        let mut bottom = radius;
        let mut top = if ratio != 0.0 {
            radius / ratio.abs()
        } else {
            radius
        };
        if ratio < 0.0 {
            std::mem::swap(&mut top, &mut bottom);
        }
        let radians = degrees.to_radians();
        let step = if degrees < 350.0 && grid.width > 1 {
            radians / (width - 1.0)
        } else {
            radians / width
        };
        let start = -radians / 2.0 + spec.float("TreeRotation", 3.0)?.to_radians();
        (up, turned) = spiral(
            grid.height,
            bottom,
            top,
            spec.float("TreeSpiralRotations", 0.0)?,
        );
        geometry = (bottom, top, start, step);
    }
    let nodes = grid
        .cells
        .iter()
        .map(|cell| Node {
            string: cell.string,
            offset: cell.offset,
            position: position(cell, &shape, &up, &turned, &geometry),
            arc: None,
        })
        .collect();
    Ok(local(&grid, nodes, Vec::new(), placement::boxed(spec)?))
}

/// Vertical strands as lines of longitude, from `StartLatitude` to
/// `EndLatitude`, spread over `Degrees` of longitude.
pub(super) fn sphere(spec: &Spec) -> Result<Local, SkipReason> {
    let grid = grid(spec, true)?;
    let (width, height) = (f64::from(grid.width), f64::from(grid.height));
    let radius = width.max(height) / 1.8 / 2.0;
    let degrees = spec.float("Degrees", 360.0)?;
    let removed = (360.0 - degrees).to_radians();
    let fudge = ((360.0 - degrees) / width).to_radians();
    let start = TAU / 4.0 + 0.003 - removed / 2.0;
    let step = (-TAU + removed - fudge) / width;
    let start_latitude = spec.float("StartLatitude", -86.0)?;
    let end_latitude = spec.float("EndLatitude", 86.0)?;
    let first_row = (start_latitude - 90.0).to_radians();
    let row_step = (end_latitude - start_latitude).to_radians() / (height - 1.0).max(1.0);
    let ellipses: Vec<Ellipse> = (0..grid.width)
        .map(|column| {
            let longitude = start + f64::from(column) * step;
            Ellipse {
                center: DVec3::ZERO,
                u: DVec3::new(0.0, radius, 0.0),
                v: DVec3::new(radius * longitude.cos(), 0.0, radius * longitude.sin()),
            }
        })
        .collect();
    let nodes = grid
        .cells
        .iter()
        .map(|cell| {
            let longitude = start + f64::from(cell.x) * step;
            let latitude = first_row + f64::from(cell.y) * row_step;
            Node {
                string: cell.string,
                offset: cell.offset,
                position: DVec3::new(
                    radius * longitude.cos() * latitude.sin(),
                    radius * latitude.cos(),
                    radius * longitude.sin() * latitude.sin(),
                ),
                arc: Some(ArcPoint {
                    arc: cell.x as usize,
                    degrees: latitude.to_degrees(),
                }),
            }
        })
        .collect();
    // Before layout version 8 xLights drew spheres flattened; their scale
    // made up for it.
    let mut placement = placement::boxed(spec)?;
    let version = spec.int("versionNumber", 0)?;
    if version < 8
        && let Placement::Boxed { scale, .. } = &mut placement
    {
        let squash = height / width.max(height);
        *scale *= DVec3::new(squash / 1.8, squash, squash / 1.8);
    }
    Ok(local(&grid, nodes, ellipses, placement))
}

//! Admission proof for color blocks: scalar control/data is identical in every
//! lane, and colors may vary. No proof state survives program admission.
use super::bytecode::{BytecodeProgram, ContextRead as ContextField, Instruction, ValueSlot};
use alloc::vec;

/// Primitive register banks can execute in bounded lanes. Reference-valued
/// temporaries retain scalar ownership. Signal queries must address the current
/// pixel at a time proved uniform by the query-prefix analysis.
pub(super) fn numeric_blocks(program: &BytecodeProgram) -> bool {
    let layout = program.layout;
    layout.arrays == 0
        && layout.enums == 0
        && layout.marks == 0
        && layout.curves == 0
        && layout.gradients == 0
        && program.instructions.iter().all(|op| match op {
            Instruction::SignalSample {
                pixel, frame_cache, ..
            } => matches!(pixel, super::bytecode::SignalPixel::Current) && *frame_cache != u32::MAX,
            _ => true,
        })
}

/// One source instruction whose time is initialized in the query prefix. Every
/// execution therefore samples the same time/input for a given pixel, even if
/// pixel-dependent branches or loops surround it. A block may materialize that
/// source once, then execute arbitrary scalar continuations for its lanes.
pub(super) fn single_query(program: &BytecodeProgram) -> bool {
    let mut queries = program.instructions.iter().filter_map(|op| match op {
        Instruction::SignalSample {
            pixel, frame_cache, ..
        } => Some((pixel, frame_cache)),
        _ => None,
    });
    matches!(queries.next(), Some((super::bytecode::SignalPixel::Current, slot)) if *slot != u32::MAX)
        && queries.next().is_none()
}

pub(super) fn color_blocks(program: &BytecodeProgram) -> bool {
    use Instruction::*;
    // Array snapshots can carry lane-dependent colors into scalar control.
    // Their ordinary interpreter semantics remain available to these programs.
    if program.layout.arrays != 0
        || program.instructions.iter().any(|op| {
            matches!(
                op,
                ContextRead {
                    read: ContextField::PixelIndex
                        | ContextField::PixelCount
                        | ContextField::PixelFraction
                        | ContextField::PixelX
                        | ContextField::PixelY
                        | ContextField::TargetMinX
                        | ContextField::TargetMinY
                        | ContextField::TargetMaxX
                        | ContextField::TargetMaxY,
                    ..
                } | Select { .. }
                    | SectionPosition { .. }
                    | SectionQuery { .. }
            )
        })
    {
        return false;
    }
    let mut varying = vec![false; program.layout.colors as usize];
    loop {
        let mut changed = false;
        for op in &program.instructions {
            let depends = |slot: super::bytecode::ColorSlot| varying[slot.0 as usize];
            let output = match *op {
                SignalSample { dst, .. } => Some((dst, true)),
                Move {
                    dst: ValueSlot::Color(dst),
                    src,
                } => Some((dst, varying[src as usize])),
                ColorBinary {
                    dst, left, right, ..
                }
                | MixColor {
                    dst, left, right, ..
                } => Some((dst, depends(left) || depends(right))),
                ColorScale { dst, color, .. } | ColorInvert { dst, color } => {
                    Some((dst, depends(color)))
                }
                _ => None,
            };
            if let Some((dst, true)) = output {
                changed |= !varying[dst.0 as usize];
                varying[dst.0 as usize] = true;
            }
        }
        if !changed {
            break;
        }
    }
    program.instructions.iter().all(|op| match op {
        ColorComponent { color, .. } => !varying[color.0 as usize],
        ValueEqual {
            left: ValueSlot::Color(left),
            right: ValueSlot::Color(right),
            ..
        } => !varying[left.0 as usize] && !varying[right.0 as usize],
        _ => true,
    })
}

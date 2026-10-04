//! Batched execution plans. Every admitted program runs in batches; a plan
//! records which initialization outputs every lane shares and which reference
//! registers need a value per lane.
use super::bytecode::{BytecodeProgram, Instruction, ValueSlot};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

const SHARED: u32 = u32::MAX;

#[derive(Clone, Debug, PartialEq)]
pub struct BatchPlan {
    /// Primitive initialization outputs, copied into every lane. Target-stage
    /// outputs follow query-stage outputs.
    outputs: Box<[ValueSlot]>,
    target_outputs: usize,
    /// Per reference bank (arrays, enums, marks, curves, gradients): the lane
    /// row of each register, or `SHARED`.
    rows: [Box<[u32]>; 5],
    row_count: usize,
    /// Primitive registers and loop counters of each instruction.
    ops: Box<[LaneOp]>,
    /// Registers that initialization makes uniform, as a bit set.
    initial: Box<[u64]>,
    /// Whether the program reads the pixel count or target bounds. Runs of a
    /// program that does not may span pixels with different counts and bounds.
    reads_target: bool,
}

/// A register numbering over the primitive banks and loop counters, in that
/// order: ints, floats, bools, colors, loops.
pub const NO_REGISTER: u16 = u16::MAX;

/// An instruction's primitive inputs and output. When `uniform` holds and
/// every input has the same value in every live lane, so does the output, and
/// one lane can compute it for all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneOp {
    pub inputs: [u16; 3],
    pub output: u16,
    pub uniform: bool,
}

impl BatchPlan {
    /// Outputs written by initialization from the query or target entry.
    pub fn outputs(&self, from_target: bool) -> &[ValueSlot] {
        &self.outputs[if from_target { self.target_outputs } else { 0 }..]
    }

    /// The lane row of a reference register whose value may differ between
    /// lanes. A register written by exactly one load holds the same value in
    /// every lane that has executed it, so it has no row.
    pub fn row(&self, slot: ValueSlot) -> Option<usize> {
        let row = self.rows[bank(slot)?][slot.index() as usize];
        (row != SHARED).then_some(row as usize)
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn ops(&self) -> &[LaneOp] {
        &self.ops
    }

    pub fn initial(&self) -> &[u64] {
        &self.initial
    }

    pub fn reads_target(&self) -> bool {
        self.reads_target
    }
}

/// Number of registers in the [`LaneOp`] numbering.
pub fn lane_registers<C, S, A>(program: &BytecodeProgram<C, S, A>) -> usize {
    let layout = program.layout;
    (layout.ints + layout.floats + layout.bools + layout.colors + program.loop_count) as usize
}

fn register(program: &BytecodeProgram, slot: ValueSlot) -> Option<u16> {
    let layout = program.layout;
    let index = match slot {
        ValueSlot::Int(slot) => slot.0,
        ValueSlot::Float(slot) => layout.ints + slot.0,
        ValueSlot::Bool(slot) => layout.ints + layout.floats + slot.0,
        ValueSlot::Color(slot) => layout.ints + layout.floats + layout.bools + slot.0,
        _ => return None,
    };
    u16::try_from(index).ok()
}

fn loop_register(program: &BytecodeProgram, id: u32) -> Option<u16> {
    let layout = program.layout;
    u16::try_from(layout.ints + layout.floats + layout.bools + layout.colors + id).ok()
}

/// Instructions whose result is a function of their inputs alone, with no
/// pixel context and no per-lane reference.
fn uniform_capable(op: &Instruction, shared: impl Fn(ValueSlot) -> bool) -> bool {
    use super::bytecode::ContextRead as Read;
    use Instruction::*;
    match op {
        ContextRead { read, .. } => !matches!(
            read,
            Read::PixelIndex | Read::PixelFraction | Read::PixelX | Read::PixelY
        ),
        Move { dst, .. } => bank(*dst).is_none(),
        CurveSample { curve, .. }
        | CurveFloatClamped { curve, .. }
        | CurveCrossing { curve, .. } => shared(ValueSlot::Curve(*curve)),
        GradientSample { gradient, .. } | GradientColorScaled { gradient, .. } => {
            shared(ValueSlot::Gradient(*gradient))
        }
        Mark { marks, .. } | LoopMarksStart { marks, .. } => shared(ValueSlot::Marks(*marks)),
        Len { value, .. } => shared(ValueSlot::Array(*value)),
        ValueEqual { left, right, .. } => bank(*left).is_none() && bank(*right).is_none(),
        SectionQuery { .. }
        | SectionPosition { .. }
        | SignalSample { .. }
        | Index { .. }
        | Select { .. }
        | MakeArray { .. }
        | ReturnColor(_) => false,
        _ => true,
    }
}

fn lane_op(
    program: &BytecodeProgram,
    op: &Instruction,
    shared: impl Fn(ValueSlot) -> bool,
) -> LaneOp {
    let mut operands = program.value_operands.to_vec();
    let mut copy = op.clone();
    let mut inputs = Vec::new();
    let mut output = NO_REGISTER;
    let mut complete = true;
    super::bytecode::slots(&mut copy, &mut operands, |slot, write| {
        match (register(program, slot), write) {
            (Some(index), true) => output = index,
            (Some(index), false) => inputs.push(index),
            (None, _) if bank(slot).is_some() && !shared(slot) => complete = false,
            _ => {}
        }
        slot
    });
    match op {
        Instruction::LoopRangeStart { id, .. } | Instruction::LoopMarksStart { id, .. } => {
            output = loop_register(program, *id).unwrap_or(NO_REGISTER);
        }
        Instruction::LoopEnd { id, .. } => {
            let counter = loop_register(program, *id).unwrap_or(NO_REGISTER);
            inputs.push(counter);
            output = counter;
        }
        _ => {}
    }
    let uniform = complete && inputs.len() <= 3 && uniform_capable(op, &shared);
    let mut fixed = [NO_REGISTER; 3];
    for (slot, input) in fixed.iter_mut().zip(&inputs) {
        *slot = *input;
    }
    LaneOp {
        inputs: fixed,
        output,
        uniform,
    }
}

fn bank(slot: ValueSlot) -> Option<usize> {
    match slot {
        ValueSlot::Array(_) => Some(0),
        ValueSlot::Enum(_) => Some(1),
        ValueSlot::Marks(_) => Some(2),
        ValueSlot::Curve(_) => Some(3),
        ValueSlot::Gradient(_) => Some(4),
        _ => None,
    }
}

fn load(op: &Instruction) -> bool {
    use Instruction::*;
    matches!(
        op,
        LoadMarksConst { .. }
            | LoadMarksParam { .. }
            | LoadArrayConst { .. }
            | LoadArrayParam { .. }
            | LoadCurveConst { .. }
            | LoadCurveParam { .. }
            | LoadGradientConst { .. }
            | LoadGradientParam { .. }
            | LoadEnumConst { .. }
            | LoadEnumParam { .. }
    )
}

pub(super) fn batch_plan(program: &BytecodeProgram) -> BatchPlan {
    let layout = program.layout;
    let sizes = [
        layout.arrays,
        layout.enums,
        layout.marks,
        layout.curves,
        layout.gradients,
    ];
    // Writers per register, and whether the only one is a load.
    let mut writers: [Vec<(u32, bool)>; 5] = sizes.map(|size| vec![(0, false); size as usize]);
    for op in &program.instructions {
        let Some(slot) = op.written_slot() else {
            continue;
        };
        let Some(bank) = bank(slot) else {
            continue;
        };
        let entry = &mut writers[bank][slot.index() as usize];
        entry.0 += 1;
        entry.1 = load(op);
    }
    let mut row_count = 0;
    let rows: [Box<[u32]>; 5] = writers.map(|bank| {
        bank.into_iter()
            .map(|(count, loaded)| {
                if count == 1 && loaded {
                    SHARED
                } else {
                    row_count += 1;
                    row_count as u32 - 1
                }
            })
            .collect()
    });
    let entry = program.pixel_entry as usize;
    let target_entry = program.target_entry();
    let mut outputs = Vec::new();
    let mut target_outputs = 0;
    for (ip, op) in program.instructions[..entry].iter().enumerate() {
        if ip == target_entry {
            target_outputs = outputs.len();
        }
        if let Some(slot) = op.written_slot()
            && bank(slot).is_none()
            && slot != ValueSlot::Void
        {
            outputs.push(slot);
        }
    }
    if target_entry >= entry {
        target_outputs = outputs.len();
    }
    let shared = |slot: ValueSlot| {
        bank(slot).is_some_and(|bank| rows[bank][slot.index() as usize] == SHARED)
    };
    let mut ops: Vec<LaneOp> = program
        .instructions
        .iter()
        .map(|op| lane_op(program, op, shared))
        .collect();
    // Registers that can differ between lanes: written by an instruction that
    // is not uniform-capable, or reading such a register. They never need
    // runtime tracking.
    let mut varying = vec![false; lane_registers(program)];
    loop {
        let mut changed = false;
        for op in &ops {
            let reads_varying = op
                .inputs
                .iter()
                .any(|&input| input != NO_REGISTER && varying[usize::from(input)]);
            if op.output != NO_REGISTER
                && (!op.uniform || reads_varying)
                && !varying[usize::from(op.output)]
            {
                varying[usize::from(op.output)] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for op in &mut ops {
        if op
            .inputs
            .iter()
            .any(|&input| input != NO_REGISTER && varying[usize::from(input)])
        {
            op.uniform = false;
        }
        // A one-lane result must be copied to every lane; a varying
        // register is never copied, so its writers run in every lane.
        if op.output != NO_REGISTER && varying[usize::from(op.output)] {
            op.output = NO_REGISTER;
            op.uniform = false;
        }
    }
    let ops = ops.into_boxed_slice();
    let mut initial = vec![0u64; lane_registers(program).div_ceil(64)];
    for slot in &outputs {
        if let Some(index) = register(program, *slot) {
            initial[usize::from(index) / 64] |= 1 << (index % 64);
        }
    }
    BatchPlan {
        outputs: outputs.into_boxed_slice(),
        target_outputs,
        rows,
        row_count,
        ops,
        initial: initial.into_boxed_slice(),
        reads_target: program.instructions.iter().any(|op| {
            matches!(
                op,
                Instruction::ContextRead {
                    read: super::bytecode::ContextRead::PixelCount
                        | super::bytecode::ContextRead::TargetMinX
                        | super::bytecode::ContextRead::TargetMinY
                        | super::bytecode::ContextRead::TargetMaxX
                        | super::bytecode::ContextRead::TargetMaxY,
                    ..
                }
            )
        }),
    }
}

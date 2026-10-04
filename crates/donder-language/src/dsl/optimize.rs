use std::collections::{HashMap, HashSet};

use super::bytecode::{
    BoolSlot, ColorSlot, FloatSlot, Instruction, IntSlot, SignalPixel, SlotLayout, ValueSlot, slots,
};
use super::types::{Type, Value};

mod dataflow;
mod loops;
mod registers;
pub(super) use dataflow::prepare_bindings;
pub(super) use registers::reuse;

/// Find bindings that can specialize control flow. Ordinary uniform data stays
/// in shared parameterized programs instead of producing a variant per clip.
pub(super) fn control_inputs(
    code: &[Instruction],
    operands: &mut [ValueSlot],
    dynamic: impl Fn(usize) -> bool,
    constants: super::ProgramConstants,
) -> (HashSet<usize>, HashSet<super::bytecode::ContextRead>) {
    use super::bytecode::ContextRead;
    #[derive(Clone, Default, PartialEq, Eq)]
    struct Inputs {
        params: HashSet<usize>,
        context: HashSet<ContextRead>,
        runtime: bool,
    }
    impl Inputs {
        fn merge(&mut self, other: &Self) {
            self.params.extend(&other.params);
            self.context.extend(&other.context);
            self.runtime |= other.runtime;
        }
    }
    let metadata: Vec<_> = code
        .iter()
        .map(|op| {
            let mut inputs = Inputs::default();
            match op {
                Instruction::LoadIntParam { param, .. }
                | Instruction::LoadFloatParam { param, .. }
                | Instruction::LoadBoolParam { param, .. }
                | Instruction::LoadColorParam { param, .. }
                | Instruction::EnumParamEqualConst { param, .. } => {
                    inputs.params.insert(*param);
                    inputs.runtime = dynamic(*param);
                }
                Instruction::ContextRead { read, .. } => {
                    inputs.context.insert(*read);
                    inputs.runtime = match read {
                        ContextRead::PixelCount => constants.pixel_count.is_none(),
                        ContextRead::Duration => constants.duration_seconds.is_none(),
                        _ => true,
                    };
                }
                _ => inputs.runtime = !loops::pure(op) && !matches!(op, Instruction::Rand { .. }),
            }
            let mut reads = Vec::new();
            slots(&mut op.clone(), operands, |slot, write| {
                if !write {
                    reads.push(slot);
                }
                slot
            });
            (op.written_slot(), reads, inputs)
        })
        .collect();
    let mut values: HashMap<_, _> = metadata
        .iter()
        .filter_map(|(dst, ..)| dst.map(|slot| (slot, Inputs::default())))
        .collect();
    loop {
        let mut changed = false;
        for (dst, reads, own) in &metadata {
            let Some(dst) = dst else { continue };
            let mut inputs = own.clone();
            for read in reads {
                if let Some(value) = values.get(read) {
                    inputs.merge(value);
                } else {
                    inputs.runtime = true;
                }
            }
            let value = values
                .get_mut(dst)
                .unwrap_or_else(|| unreachable!("indexed output"));
            let previous = value.clone();
            value.merge(&inputs);
            changed |= previous != *value;
        }
        if !changed {
            break;
        }
    }
    let mut result = Inputs::default();
    for (op, (_, reads, _)) in code.iter().zip(&metadata) {
        // A choice's condition decides control as much as a branch does.
        let condition;
        let reads = match op {
            Instruction::Choose {
                condition: slot, ..
            } => {
                condition = [ValueSlot::Bool(*slot)];
                &condition[..]
            }
            _ if op.conditional_target().is_some()
                || matches!(op, Instruction::LoopRangeStart { .. }) =>
            {
                &reads[..]
            }
            _ => continue,
        };
        let mut inputs = Inputs::default();
        for read in reads {
            if let Some(value) = values.get(read) {
                inputs.merge(value);
            } else {
                inputs.runtime = true;
            }
        }
        if !inputs.runtime {
            result.merge(&inputs);
        }
    }
    (result.params, result.context)
}

pub(super) fn prepare_pixels(
    code: &mut Vec<Instruction>,
    operands: &mut [ValueSlot],
    layout: &mut SlotLayout,
    array_types: &mut Vec<Type>,
) -> u32 {
    while if_convert(code, operands, layout) {}
    let entry = hoist_uniform(code, operands, layout, array_types);
    split_uniform_samples(code, layout, entry as usize);
    // Pixel execution is an implicit loop too. Invert proven bounded divisors
    // in query/target initialization even when there is only one textual use.
    loops::reciprocals(code, operands, layout, entry as usize);
    hoist_uniform(code, operands, layout, array_types)
}

/// Rewrite one `x = a; ...; if (!c) { x = b; }` into single assignments and a
/// `Choose`, so staging can lift the result when its inputs are uniform. The
/// skipped body is one total, pure instruction, `a` is the only other write,
/// and control flow from `a` through the join is straight-line. Returns whether
/// a rewrite happened.
fn if_convert(
    code: &mut Vec<Instruction>,
    operands: &mut [ValueSlot],
    layout: &mut SlotLayout,
) -> bool {
    let mut targets = vec![0usize; code.len() + 1];
    for op in code.iter() {
        if let Some(target) = op.jump_target() {
            targets[target.min(code.len())] += 1;
        }
    }
    let mut writes = HashMap::<ValueSlot, Vec<usize>>::new();
    for (ip, op) in code.iter_mut().enumerate() {
        slots(op, operands, |slot, write| {
            if write {
                writes.entry(slot).or_default().push(ip);
            }
            slot
        });
    }
    let control =
        |op: &Instruction| op.jump_target().is_some() || matches!(op, Instruction::ReturnColor(_));
    for branch in 0..code.len().saturating_sub(2) {
        if code[branch].conditional_target() != Some(branch + 2)
            || targets[branch + 1] != 0
            || targets[branch + 2] != 1
        {
            continue;
        }
        let body = &code[branch + 1];
        if !hoistable(body) || control(body) {
            continue;
        }
        let Some(written) = body.written_slot() else {
            continue;
        };
        if !matches!(
            written,
            ValueSlot::Int(_) | ValueSlot::Float(_) | ValueSlot::Bool(_) | ValueSlot::Color(_)
        ) {
            continue;
        }
        let Some(&[first, second]) = writes.get(&written).map(Vec::as_slice) else {
            continue;
        };
        if second != branch + 1
            || first >= branch
            || (first + 1..=branch).any(|ip| targets[ip] != 0)
            || code[first + 1..branch].iter().any(control)
        {
            continue;
        }
        let Some((predicate, condition)) = branch_predicate(&code[branch], layout) else {
            continue;
        };
        let fresh = |layout: &mut SlotLayout| {
            let slot = match written {
                ValueSlot::Int(_) => &mut layout.ints,
                ValueSlot::Float(_) => &mut layout.floats,
                ValueSlot::Bool(_) => &mut layout.bools,
                _ => &mut layout.colors,
            };
            *slot += 1;
            written.with_index(*slot - 1)
        };
        let (kept, assigned) = (fresh(layout), fresh(layout));
        let rename =
            |op: &mut Instruction, operands: &mut [ValueSlot], write_to: Option<ValueSlot>| {
                slots(op, operands, |slot, write| match (slot == written, write) {
                    (true, false) => kept,
                    (true, true) => write_to.unwrap_or(slot),
                    _ => slot,
                });
            };
        rename(&mut code[first], operands, Some(kept));
        for op in &mut code[first + 1..=branch] {
            rename(op, operands, None);
        }
        rename(&mut code[branch + 1], operands, Some(assigned));
        // The body runs when the branch is not taken.
        let (when_true, when_false) = if predicate.taken_when {
            (kept, assigned)
        } else {
            (assigned, kept)
        };
        let choose = Instruction::Choose {
            dst: written,
            condition,
            when_true: when_true.index(),
            when_false: when_false.index(),
        };
        // Replace the branch with the predicate (if any) and insert the choice
        // after the body; shift every later absolute target.
        let inserted = usize::from(predicate.compare.is_some());
        let mut replacement = Vec::with_capacity(code.len() + 1);
        for (ip, op) in code.drain(..).enumerate() {
            if ip == branch {
                replacement.extend(predicate.compare.clone());
            } else {
                replacement.push(op);
            }
            if ip == branch + 1 {
                replacement.push(choose.clone());
            }
        }
        let shift = |target: usize| {
            if target > branch + 1 {
                target + inserted
            } else if target > branch {
                target + inserted - 1
            } else {
                target
            }
        };
        for op in &mut replacement {
            if let Some(target) = op.jump_target_mut() {
                *target = shift(*target);
            }
        }
        *code = replacement;
        return true;
    }
    false
}

struct BranchPredicate {
    /// Instruction computing the tested predicate, or none for a boolean branch.
    compare: Option<Instruction>,
    /// The branch is taken when the predicate equals this value.
    taken_when: bool,
}

/// Express a conditional branch as a boolean predicate and its polarity.
fn branch_predicate(
    op: &Instruction,
    layout: &mut SlotLayout,
) -> Option<(BranchPredicate, BoolSlot)> {
    use super::bytecode::CompareOp;
    let mut fresh = || {
        layout.bools += 1;
        BoolSlot(layout.bools - 1)
    };
    let compare =
        |op: CompareOp, left: FloatSlot, right: FloatSlot, dst| Instruction::FloatCompare {
            dst,
            op,
            left,
            right,
        };
    let int_compare = |op: CompareOp, left: IntSlot, right: IntSlot, dst| Instruction::IntCompare {
        dst,
        op,
        left,
        right,
    };
    let constant =
        |op: CompareOp, value: FloatSlot, constant_bits: u32, dst| Instruction::FloatCompareConst {
            dst,
            op,
            value,
            constant_bits,
            constant_left: false,
        };
    let (compare, taken_when, condition) = match *op {
        Instruction::JumpIfFalse { condition, .. } => (None, false, condition),
        Instruction::JumpIfTrue { condition, .. } => (None, true, condition),
        Instruction::FloatJumpLess {
            left, right, when, ..
        } => {
            let d = fresh();
            (Some(compare(CompareOp::Less, left, right, d)), when, d)
        }
        Instruction::FloatJumpLessEqual {
            left, right, when, ..
        } => {
            let d = fresh();
            (Some(compare(CompareOp::LessEqual, left, right, d)), when, d)
        }
        Instruction::FloatJumpGreater {
            left, right, when, ..
        } => {
            let d = fresh();
            (Some(compare(CompareOp::Greater, left, right, d)), when, d)
        }
        Instruction::FloatJumpGreaterEqual {
            left, right, when, ..
        } => {
            let d = fresh();
            (
                Some(compare(CompareOp::GreaterEqual, left, right, d)),
                when,
                d,
            )
        }
        Instruction::FloatJumpLessConst {
            value,
            constant_bits,
            when,
            ..
        } => {
            let d = fresh();
            (
                Some(constant(CompareOp::Less, value, constant_bits, d)),
                when,
                d,
            )
        }
        Instruction::FloatJumpLessEqualConst {
            value,
            constant_bits,
            when,
            ..
        } => {
            let d = fresh();
            (
                Some(constant(CompareOp::LessEqual, value, constant_bits, d)),
                when,
                d,
            )
        }
        Instruction::FloatJumpGreaterConst {
            value,
            constant_bits,
            when,
            ..
        } => {
            let d = fresh();
            (
                Some(constant(CompareOp::Greater, value, constant_bits, d)),
                when,
                d,
            )
        }
        Instruction::FloatJumpGreaterEqualConst {
            value,
            constant_bits,
            when,
            ..
        } => {
            let d = fresh();
            (
                Some(constant(CompareOp::GreaterEqual, value, constant_bits, d)),
                when,
                d,
            )
        }
        Instruction::IntJumpLess {
            left, right, when, ..
        } => {
            let d = fresh();
            (Some(int_compare(CompareOp::Less, left, right, d)), when, d)
        }
        Instruction::IntJumpLessEqual {
            left, right, when, ..
        } => {
            let d = fresh();
            (
                Some(int_compare(CompareOp::LessEqual, left, right, d)),
                when,
                d,
            )
        }
        Instruction::IntJumpGreater {
            left, right, when, ..
        } => {
            let d = fresh();
            (
                Some(int_compare(CompareOp::Greater, left, right, d)),
                when,
                d,
            )
        }
        Instruction::IntJumpGreaterEqual {
            left, right, when, ..
        } => {
            let d = fresh();
            (
                Some(int_compare(CompareOp::GreaterEqual, left, right, d)),
                when,
                d,
            )
        }
        _ => return None,
    };
    Some((
        BranchPredicate {
            compare,
            taken_when,
        },
        condition,
    ))
}

/// Typed parameter sampling is total after admission. Missing curves yield NaN
/// and missing gradients yield black; neither performs a signal read.
fn parameter_sample(op: &Instruction) -> bool {
    matches!(
        op,
        Instruction::CurveParamSample { .. }
            | Instruction::GradientParamSample { .. }
            | Instruction::CurveParamCrossing { .. }
            | Instruction::CurveParamFloatClamped { .. }
            | Instruction::GradientParamColorScaled { .. }
    )
}

/// Fusion must not force query/target sampling to repeat in every pixel. Keep
/// the varying clamp/scale in the body and let the next hoist lift its sample.
fn split_uniform_samples(code: &mut Vec<Instruction>, layout: &mut SlotLayout, entry: usize) {
    let uniform: HashSet<_> = code[..entry]
        .iter()
        .filter_map(Instruction::written_slot)
        .collect();
    let mut offsets = Vec::with_capacity(code.len() + 1);
    let mut result = Vec::with_capacity(code.len());
    for (ip, op) in code.drain(..).enumerate() {
        offsets.push(result.len());
        match op {
            Instruction::GradientParamColorScaled {
                dst,
                param,
                source,
                position,
                scale,
            } if ip >= entry
                && uniform.contains(&ValueSlot::Float(position))
                && !uniform.contains(&ValueSlot::Float(scale)) =>
            {
                let sample = ColorSlot(layout.colors);
                layout.colors += 1;
                let clamped = FloatSlot(layout.floats);
                layout.floats += 1;
                result.extend([
                    Instruction::GradientParamSample {
                        dst: sample,
                        param,
                        source,
                        position,
                    },
                    Instruction::ClampConst {
                        dst: clamped,
                        value: scale,
                        min_bits: 0.0_f32.to_bits(),
                        max_bits: 1.0_f32.to_bits(),
                    },
                    Instruction::ColorScale {
                        dst,
                        color: sample,
                        scale: clamped,
                    },
                ]);
            }
            Instruction::CurveParamFloatClamped {
                dst,
                param,
                source,
                position,
                min,
                max,
            } if ip >= entry
                && uniform.contains(&ValueSlot::Float(position))
                && (!uniform.contains(&ValueSlot::Float(min))
                    || !uniform.contains(&ValueSlot::Float(max))) =>
            {
                let sample = FloatSlot(layout.floats);
                layout.floats += 1;
                result.extend([
                    Instruction::CurveParamSample {
                        dst: sample,
                        param,
                        source,
                        position,
                    },
                    Instruction::Clamp {
                        dst,
                        value: sample,
                        min,
                        max,
                    },
                ]);
            }
            _ => result.push(op),
        }
    }
    offsets.push(result.len());
    for op in &mut result {
        if let Some(target) = op.jump_target_mut() {
            *target = offsets[*target];
        }
    }
    *code = result;
}

/// Loads of immutable references. A load stays in the pixel body because
/// reference registers do not survive an invocation; lifted readers get their
/// own copy of the load in initialization.
fn reference_load(op: &Instruction) -> bool {
    matches!(
        op,
        Instruction::LoadMarksParam { .. }
            | Instruction::LoadMarksConst { .. }
            | Instruction::LoadArrayParam { .. }
            | Instruction::LoadArrayConst { .. }
            | Instruction::LoadCurveParam { .. }
            | Instruction::LoadCurveConst { .. }
            | Instruction::LoadGradientParam { .. }
            | Instruction::LoadGradientConst { .. }
    )
}

/// Move pure, single-assignment scalar expressions to query and target
/// initialization. Mutable locals stay in the pixel body. Hoisting may
/// cross branches only when evaluation is harmless. Typed parameter samples are
/// total; fallible signal reads remain in the body in their original order.
/// Total, pure instructions: safe to execute once for a whole query/target, or
/// unconditionally in place of a branch.
fn hoistable(op: &Instruction) -> bool {
    use super::bytecode::ContextRead;
    matches!(
        op,
        Instruction::LoadIntParam { .. }
            | Instruction::Move { .. }
            | Instruction::LoadFloatParam { .. }
            | Instruction::LoadBoolParam { .. }
            | Instruction::LoadColorParam { .. }
            | Instruction::LoadIntConst { .. }
            | Instruction::LoadFloatConst { .. }
            | Instruction::LoadBoolConst { .. }
            | Instruction::LoadColorConst { .. }
            | Instruction::ContextRead {
                read: ContextRead::Progress
                    | ContextRead::Seconds
                    | ContextRead::Duration
                    | ContextRead::PixelCount
                    | ContextRead::TargetMinX
                    | ContextRead::TargetMinY
                    | ContextRead::TargetMaxX
                    | ContextRead::TargetMaxY,
                ..
            }
            | Instruction::FloatAdd { .. }
            | Instruction::QuerySeconds { .. }
            | Instruction::QueryProgress { .. }
            | Instruction::FloatSubtract { .. }
            | Instruction::FloatMultiply { .. }
            | Instruction::FloatDivide { .. }
            | Instruction::FloatRemainder { .. }
            | Instruction::FloatAddConst { .. }
            | Instruction::FloatSubtractConst { .. }
            | Instruction::FloatMultiplyConst { .. }
            | Instruction::FloatDivideConst { .. }
            | Instruction::FloatRemainderConst { .. }
            | Instruction::FloatSubtractFromConst { .. }
            | Instruction::FloatDivideIntoConst { .. }
            | Instruction::FloatRemainderFromConst { .. }
            | Instruction::FloatBinary { .. }
            | Instruction::FloatBinaryConst { .. }
            | Instruction::FloatCompare { .. }
            | Instruction::IntCompare { .. }
            | Instruction::FloatCompareConst { .. }
            | Instruction::FloatUnary { .. }
            | Instruction::MixFloat { .. }
            | Instruction::MixColor { .. }
            | Instruction::ColorBinary { .. }
            | Instruction::ColorScale { .. }
            | Instruction::ColorComponent { .. }
            | Instruction::Clamp { .. }
            | Instruction::ClampConst { .. }
            | Instruction::Smoothstep { .. }
            | Instruction::Rand { .. }
            | Instruction::EnumParamEqualConst { .. }
            | Instruction::ColorInvert { .. }
            | Instruction::Rgb { .. }
            | Instruction::Hsv { .. }
            | Instruction::IntToFloat { .. }
            | Instruction::Not { .. }
            | Instruction::NegFloat { .. }
            | Instruction::Choose { .. }
            | Instruction::Mark { .. }
            | Instruction::Len { .. }
            | Instruction::CurveSample { .. }
            | Instruction::CurveCrossing { .. }
            | Instruction::CurveFloatClamped { .. }
            | Instruction::GradientSample { .. }
            | Instruction::GradientColorScaled { .. }
    ) || parameter_sample(op)
        || reference_load(op)
}

pub(super) fn hoist_uniform(
    code: &mut Vec<Instruction>,
    operands: &mut [ValueSlot],
    layout: &mut SlotLayout,
    array_types: &mut Vec<Type>,
) -> u32 {
    use super::bytecode::ContextRead;
    let mut writes = HashMap::<ValueSlot, usize>::new();
    let metadata = code
        .iter_mut()
        .map(|op| {
            let eligible = hoistable(op);
            let mut dst = None;
            let mut reads = Vec::new();
            slots(op, operands, |slot, write| {
                if write {
                    dst = Some(slot);
                    *writes.entry(slot).or_default() += 1;
                } else {
                    reads.push(slot);
                }
                slot
            });
            (eligible, dst, reads)
        })
        .collect::<Vec<_>>();
    let mut uniform = HashMap::new();
    // Uniform reference slot -> its load, and the initialization copy, if any.
    let mut references = HashMap::<ValueSlot, usize>::new();
    let mut copies = HashMap::<ValueSlot, ValueSlot>::new();
    let mut lifted = vec![false; code.len()];
    let mut prefix = Vec::new();
    for target_stage in [false, true] {
        loop {
            let mut changed = false;
            for (index, (eligible, dst, reads)) in metadata.iter().enumerate() {
                if let Some(dst) = dst
                    && reference_load(&code[index])
                    && writes[dst] == 1
                    && !references.contains_key(dst)
                {
                    references.insert(*dst, index);
                    uniform.insert(*dst, false);
                    changed = true;
                    continue;
                }
                if let Some(dst) = dst
                    && *eligible
                    && !lifted[index]
                    && matches!(
                        dst,
                        ValueSlot::Int(_)
                            | ValueSlot::Float(_)
                            | ValueSlot::Bool(_)
                            | ValueSlot::Color(_)
                    )
                    && writes[dst] == 1
                    && reads.iter().all(|slot| uniform.contains_key(slot))
                    && (target_stage
                        || !matches!(
                            code[index],
                            Instruction::ContextRead {
                                read: ContextRead::PixelCount
                                    | ContextRead::TargetMinX
                                    | ContextRead::TargetMinY
                                    | ContextRead::TargetMaxX
                                    | ContextRead::TargetMaxY,
                                ..
                            }
                        ))
                {
                    uniform.insert(*dst, target_stage);
                    lifted[index] = true;
                    let mut op = code[index].clone();
                    for slot in reads {
                        let Some(&load) = references.get(slot) else {
                            continue;
                        };
                        copies.entry(*slot).or_insert_with(|| {
                            let fresh = match *slot {
                                ValueSlot::Array(old) => {
                                    let ty = array_types[old.0 as usize].clone();
                                    array_types.push(ty.clone());
                                    ValueSlot::for_type(&ty, layout)
                                }
                                ValueSlot::Marks(_) => ValueSlot::for_type(&Type::Marks, layout),
                                ValueSlot::Curve(_) => ValueSlot::for_type(&Type::Curve, layout),
                                _ => ValueSlot::for_type(&Type::Gradient, layout),
                            };
                            let mut copy = code[load].clone();
                            slots(
                                &mut copy,
                                operands,
                                |slot, write| if write { fresh } else { slot },
                            );
                            prefix.push(copy);
                            fresh
                        });
                    }
                    slots(&mut op, operands, |slot, write| {
                        if write {
                            slot
                        } else {
                            copies.get(&slot).copied().unwrap_or(slot)
                        }
                    });
                    prefix.push(op);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
    // A load whose readers all moved to initialization is dead in the body.
    for (slot, &load) in &references {
        if copies.contains_key(slot)
            && !metadata
                .iter()
                .enumerate()
                .any(|(index, (_, _, reads))| !lifted[index] && reads.contains(slot))
        {
            lifted[load] = true;
        }
    }
    let mut frame_cache = 0u32;
    for op in &mut *code {
        if let Instruction::SignalSample {
            seconds,
            frame_cache: slot,
            ..
        } = op
        {
            *slot = if uniform.get(&ValueSlot::Float(*seconds)) == Some(&false) {
                let slot = frame_cache;
                frame_cache = frame_cache.saturating_add(1);
                slot
            } else {
                u32::MAX
            };
        }
    }
    let entry = prefix.len();
    let mut offsets = Vec::with_capacity(code.len() + 1);
    let mut kept = entry;
    for &lifted in &lifted {
        offsets.push(kept);
        kept += usize::from(!lifted);
    }
    offsets.push(kept);
    for (index, mut op) in code.drain(..).enumerate() {
        if lifted[index] {
            continue;
        }
        if let Some(target) = op.jump_target_mut() {
            *target = offsets[*target];
        }
        prefix.push(op);
    }
    *code = prefix;
    entry as u32
}

pub(super) fn cleanup(
    code: &mut Vec<Instruction>,
    array_constants: &mut Vec<std::sync::Arc<[Value]>>,
    operands: &mut Vec<ValueSlot>,
    layout: &mut SlotLayout,
    array_types: &mut Vec<Type>,
    enum_types: &mut Vec<Type>,
) {
    dataflow::run(code, operands);
    loops::run(code, operands, layout);
    let targets = code.iter().filter_map(jump_target).collect::<HashSet<_>>();
    let mut samples = HashMap::<(usize, FloatSlot, SignalPixel<IntSlot>), ColorSlot>::new();
    let mut reused_sample = false;
    for (offset, op) in code.iter_mut().enumerate() {
        if targets.contains(&offset) {
            samples.clear();
        }
        if let Some(dst) = op.written_slot() {
            samples.retain(|(_, time, pixel), color| {
                ValueSlot::Float(*time) != dst
                    && ValueSlot::Color(*color) != dst
                    && pixel
                        .index()
                        .is_none_or(|index| ValueSlot::Int(*index) != dst)
            });
        }
        // Signals are stateless: preserve the first read (and its errors), then
        // reuse its value while the time, coordinate, and result slots remain unchanged.
        if let Instruction::SignalSample {
            dst,
            input,
            seconds,
            pixel,
            ..
        } = *op
        {
            if let Some(&src) = samples.get(&(input, seconds, pixel)) {
                *op = Instruction::Move {
                    dst: ValueSlot::Color(dst),
                    src: src.0,
                };
                reused_sample = true;
            } else {
                samples.insert((input, seconds, pixel), dst);
            }
        }
        if jump_target(op).is_some() || matches!(op, Instruction::ReturnColor(_)) {
            samples.clear();
        }
    }

    if reused_sample {
        dataflow::run(code, operands);
    }

    compact_storage(
        code,
        array_constants,
        operands,
        layout,
        array_types,
        enum_types,
    );
}

/// Rebuild storage for surviving instructions without repeating control-flow
/// optimization. Prefix partial evaluation does not change the program body.
pub(super) fn compact_storage(
    code: &mut [Instruction],
    array_constants: &mut Vec<std::sync::Arc<[Value]>>,
    operands: &mut Vec<ValueSlot>,
    layout: &mut SlotLayout,
    array_types: &mut Vec<Type>,
    enum_types: &mut Vec<Type>,
) {
    let old_constants = core::mem::take(array_constants);
    let old_operands = core::mem::take(operands);
    let old_ref_types = core::mem::take(array_types);
    let old_enum_types = core::mem::take(enum_types);
    let mut constant_ids = HashMap::new();
    let mut registers = HashMap::new();
    *layout = SlotLayout::default();
    for op in code {
        match op {
            Instruction::LoadArrayConst { constant, .. } => {
                *constant = *constant_ids.entry(*constant).or_insert_with(|| {
                    let index = array_constants.len();
                    array_constants.push(old_constants[*constant].clone());
                    index
                });
            }
            Instruction::MakeArray { items: span, .. }
            | Instruction::Select { items: span, .. } => {
                let values = &old_operands[span.start as usize..(span.start + span.len) as usize];
                span.start = operands.len() as u32;
                operands.extend_from_slice(values);
            }
            _ => {}
        }
        slots(op, operands, |slot, _| {
            *registers.entry(slot).or_insert_with(|| {
                let mapped = ValueSlot::for_type(
                    &match slot {
                        ValueSlot::Int(_) => Type::Int,
                        ValueSlot::Float(_) => Type::Float,
                        ValueSlot::Bool(_) => Type::Bool,
                        ValueSlot::Color(_) => Type::Color,
                        ValueSlot::Marks(_) => Type::Marks,
                        ValueSlot::Curve(_) => Type::Curve,
                        ValueSlot::Gradient(_) => Type::Gradient,
                        ValueSlot::Array(slot) => old_ref_types[slot.0 as usize].clone(),
                        ValueSlot::Void => Type::Void,
                        ValueSlot::Enum(slot) => old_enum_types[slot.0 as usize].clone(),
                    },
                    layout,
                );
                if let ValueSlot::Enum(old) = slot {
                    enum_types.push(old_enum_types[old.0 as usize].clone());
                }
                if let (ValueSlot::Array(old), ValueSlot::Array(_)) = (slot, mapped) {
                    array_types.push(old_ref_types[old.0 as usize].clone());
                }
                mapped
            })
        });
    }
}

fn jump_target(op: &Instruction) -> Option<usize> {
    op.jump_target()
}

/// Control-flow cleanup can remove complete loops. Keep their private states
/// dense instead of retaining dead capacity or stale admission metadata.
pub(super) fn compact_loops(code: &mut [Instruction]) -> u32 {
    let mut ids = HashMap::new();
    for op in code {
        if let Instruction::LoopRangeStart { id, .. }
        | Instruction::LoopMarksStart { id, .. }
        | Instruction::LoopEnd { id, .. } = op
        {
            let next = ids.len() as u32;
            *id = *ids.entry(*id).or_insert(next);
        }
    }
    ids.len() as u32
}

/// Inlining cannot inherit the callee workspace's previous register contents.
pub(super) fn reads_initial_registers(code: &[Instruction], operands: &mut [ValueSlot]) -> bool {
    let Some(first) = code.first() else {
        return false;
    };
    let mut needed = dataflow::live_after(code, operands)[0].clone();
    if let Some(dst) = first.written_slot() {
        needed.remove(&dst);
    }
    slots(&mut first.clone(), operands, |slot, write| {
        if !write {
            needed.insert(slot);
        }
        slot
    });
    !needed.is_empty()
}

pub(super) fn comparison_branch(
    op: &Instruction,
    when: bool,
    target: usize,
) -> Option<Instruction> {
    use super::bytecode::CompareOp;
    Some(match *op {
        Instruction::FloatCompare {
            op, left, right, ..
        } => match op {
            CompareOp::Less => Instruction::FloatJumpLess {
                left,
                right,
                when,
                target,
            },
            CompareOp::LessEqual => Instruction::FloatJumpLessEqual {
                left,
                right,
                when,
                target,
            },
            CompareOp::Greater => Instruction::FloatJumpGreater {
                left,
                right,
                when,
                target,
            },
            CompareOp::GreaterEqual => Instruction::FloatJumpGreaterEqual {
                left,
                right,
                when,
                target,
            },
        },
        Instruction::IntCompare {
            op, left, right, ..
        } => match op {
            CompareOp::Less => Instruction::IntJumpLess {
                left,
                right,
                when,
                target,
            },
            CompareOp::LessEqual => Instruction::IntJumpLessEqual {
                left,
                right,
                when,
                target,
            },
            CompareOp::Greater => Instruction::IntJumpGreater {
                left,
                right,
                when,
                target,
            },
            CompareOp::GreaterEqual => Instruction::IntJumpGreaterEqual {
                left,
                right,
                when,
                target,
            },
        },
        Instruction::FloatCompareConst {
            op,
            value,
            constant_bits,
            constant_left,
            ..
        } => {
            let op = if constant_left {
                match op {
                    CompareOp::Less => CompareOp::Greater,
                    CompareOp::LessEqual => CompareOp::GreaterEqual,
                    CompareOp::Greater => CompareOp::Less,
                    CompareOp::GreaterEqual => CompareOp::LessEqual,
                }
            } else {
                op
            };
            match op {
                CompareOp::Less => Instruction::FloatJumpLessConst {
                    value,
                    constant_bits,
                    when,
                    target,
                },
                CompareOp::LessEqual => Instruction::FloatJumpLessEqualConst {
                    value,
                    constant_bits,
                    when,
                    target,
                },
                CompareOp::Greater => Instruction::FloatJumpGreaterConst {
                    value,
                    constant_bits,
                    when,
                    target,
                },
                CompareOp::GreaterEqual => Instruction::FloatJumpGreaterEqualConst {
                    value,
                    constant_bits,
                    when,
                    target,
                },
            }
        }
        Instruction::ValueEqual {
            left: ValueSlot::Float(left),
            right: ValueSlot::Float(right),
            negate,
            ..
        } => Instruction::FloatJumpEqual {
            left,
            right,
            when: when != negate,
            target,
        },
        Instruction::ValueEqual {
            left: ValueSlot::Int(left),
            right: ValueSlot::Int(right),
            negate,
            ..
        } => Instruction::IntJumpEqual {
            left,
            right,
            when: when != negate,
            target,
        },
        _ => return None,
    })
}

use std::collections::{HashMap, HashSet};

use super::bytecode::{
    ColorSlot, FloatSlot, Instruction, IntSlot, MarkOp, NumberSlot, SignalPixel, SlotLayout,
    ValueSlot,
};
use super::types::{Type, Value};

mod arithmetic;
mod dataflow;
mod loops;
mod registers;
pub(super) use arithmetic::fuse;
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
        if op.conditional_target().is_none() && !matches!(op, Instruction::LoopRangeStart { .. }) {
            continue;
        }
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
) -> u32 {
    let entry = hoist_uniform(code, operands);
    split_uniform_samples(code, layout, entry as usize);
    // Pixel execution is an implicit loop too. Invert proven bounded divisors
    // in query/target initialization even when there is only one textual use.
    loops::reciprocals(code, operands, layout, entry as usize);
    hoist_uniform(code, operands)
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

/// Move pure, single-assignment scalar expressions to query and target
/// initialization. Mutable locals and references stay in the pixel body. Hoisting may
/// cross branches only when evaluation is harmless. Typed parameter samples are
/// total; fallible signal reads remain in the body in their original order.
pub(super) fn hoist_uniform(code: &mut Vec<Instruction>, operands: &mut [ValueSlot]) -> u32 {
    use super::bytecode::ContextRead;
    let mut writes = HashMap::<ValueSlot, usize>::new();
    let metadata = code
        .iter_mut()
        .map(|op| {
            let eligible = matches!(
                op,
                Instruction::LoadIntParam { .. }
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
                    | Instruction::FloatMultiplyAdd { .. }
                    | Instruction::FloatMultiplyAddConst { .. }
                    | Instruction::FloatMultiplySmoothstep { .. }
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
            ) || parameter_sample(op);
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
    let mut lifted = vec![false; code.len()];
    let mut prefix = Vec::new();
    for target_stage in [false, true] {
        loop {
            let before = prefix.len();
            for (index, (eligible, dst, reads)) in metadata.iter().enumerate() {
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
                    prefix.push(code[index].clone());
                }
            }
            if prefix.len() == before {
                break;
            }
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

/// One exhaustive register-operand description, used only by compilation.
/// Operand spans are unique per instruction in compiler output.
pub(super) fn slots(
    op: &mut Instruction,
    operands: &mut [ValueSlot],
    mut visit: impl FnMut(ValueSlot, bool) -> ValueSlot,
) {
    macro_rules! typed {
        ($write:expr, $kind:ident, $($slot:ident),+) => {{$(
            let ValueSlot::$kind(mapped) = visit(ValueSlot::$kind(*$slot), $write) else { unreachable!("compiler register remapping preserves types") };
            *$slot = mapped;
        )+}};
    }
    macro_rules! number {
        ($operand:expr) => {
            match $operand {
                NumberSlot::Int(slot) => typed!(false, Int, slot),
                NumberSlot::Float(slot) => typed!(false, Float, slot),
            }
        };
    }
    match op {
        Instruction::LoadCurveConst { dst, .. } | Instruction::LoadCurveParam { dst, .. } => {
            typed!(true, Curve, dst)
        }
        Instruction::LoadGradientConst { dst, .. } | Instruction::LoadGradientParam { dst, .. } => {
            typed!(true, Gradient, dst)
        }
        Instruction::CurveSample {
            dst,
            curve,
            position,
        } => {
            typed!(false, Curve, curve);
            typed!(false, Float, position);
            typed!(true, Float, dst);
        }
        Instruction::GradientSample {
            dst,
            gradient,
            position,
        } => {
            typed!(false, Gradient, gradient);
            typed!(false, Float, position);
            typed!(true, Color, dst);
        }
        Instruction::ContextRead { dst, .. } => match dst {
            NumberSlot::Int(slot) => typed!(true, Int, slot),
            NumberSlot::Float(slot) => typed!(true, Float, slot),
        },
        Instruction::LoadIntConst { dst, .. } | Instruction::LoadIntParam { dst, .. } => {
            typed!(true, Int, dst)
        }
        Instruction::LoadFloatConst { dst, .. } | Instruction::LoadFloatParam { dst, .. } => {
            typed!(true, Float, dst)
        }
        Instruction::LoadBoolConst { dst, .. }
        | Instruction::LoadBoolParam { dst, .. }
        | Instruction::EnumParamEqualConst { dst, .. } => {
            typed!(true, Bool, dst)
        }
        Instruction::LoadColorConst { dst, .. } | Instruction::LoadColorParam { dst, .. } => {
            typed!(true, Color, dst)
        }
        Instruction::LoadEnumConst { dst, .. } | Instruction::LoadEnumParam { dst, .. } => {
            typed!(true, Enum, dst);
        }
        Instruction::LoadArrayConst { dst, .. } | Instruction::LoadArrayParam { dst, .. } => {
            typed!(true, Array, dst)
        }
        Instruction::LoadMarksConst { dst, .. } | Instruction::LoadMarksParam { dst, .. } => {
            typed!(true, Marks, dst)
        }
        Instruction::Move { dst, src } => {
            *src = visit(dst.with_index(*src), false).index();
            *dst = visit(*dst, true);
        }
        Instruction::MakeArray { dst, items } => {
            for slot in &mut operands[items.start as usize..(items.start + items.len) as usize] {
                *slot = visit(*slot, false);
            }
            typed!(true, Array, dst);
        }
        Instruction::Index {
            dst,
            target,
            index,
            default,
        } => {
            *default = visit(dst.with_index(*default), false).index();
            typed!(false, Array, target);
            number!(index);
            *dst = visit(*dst, true);
        }
        Instruction::Select {
            dst,
            items,
            index,
            default,
        } => {
            *default = visit(dst.with_index(*default), false).index();
            for slot in &mut operands[items.start as usize..(items.start + items.len) as usize] {
                *slot = visit(*slot, false);
            }
            number!(index);
            *dst = visit(*dst, true);
        }
        Instruction::CurveParamSample { dst, position, .. } => {
            typed!(false, Float, position);
            typed!(true, Float, dst);
        }
        Instruction::GradientParamSample { dst, position, .. } => {
            typed!(false, Float, position);
            typed!(true, Color, dst);
        }
        Instruction::SignalSample {
            dst,
            seconds,
            pixel,
            ..
        } => {
            typed!(false, Float, seconds);
            *pixel = pixel.map(|mut slot| {
                let index = &mut slot;
                typed!(false, Int, index);
                slot
            });
            typed!(true, Color, dst);
        }
        Instruction::IntToFloat { dst, src } => {
            typed!(false, Int, src);
            typed!(true, Float, dst);
        }
        Instruction::Not { dst, src } => {
            typed!(false, Bool, src);
            typed!(true, Bool, dst);
        }
        Instruction::NegInt { dst, src } => {
            typed!(false, Int, src);
            typed!(true, Int, dst);
        }
        Instruction::NegFloat { dst, src } => {
            typed!(false, Float, src);
            typed!(true, Float, dst);
        }
        Instruction::FloatAdd {
            dst, left, right, ..
        }
        | Instruction::FloatSubtract {
            dst, left, right, ..
        }
        | Instruction::FloatMultiply {
            dst, left, right, ..
        }
        | Instruction::FloatDivide {
            dst, left, right, ..
        }
        | Instruction::FloatRemainder {
            dst, left, right, ..
        }
        | Instruction::FloatBinary {
            dst, left, right, ..
        } => {
            typed!(false, Float, left, right);
            typed!(true, Float, dst);
        }
        Instruction::IntAdd {
            dst, left, right, ..
        }
        | Instruction::IntSubtract {
            dst, left, right, ..
        }
        | Instruction::IntMultiply {
            dst, left, right, ..
        }
        | Instruction::IntRemainder {
            dst, left, right, ..
        } => {
            typed!(false, Int, left, right);
            typed!(true, Int, dst);
        }
        Instruction::FloatMultiplyAdd {
            dst,
            left,
            right,
            addend,
        } => {
            typed!(false, Float, left, right, addend);
            typed!(true, Float, dst);
        }
        Instruction::FloatMultiplyAddConst {
            dst, value, addend, ..
        } => {
            typed!(false, Float, value, addend);
            typed!(true, Float, dst);
        }
        Instruction::FloatMultiplySmoothstep { dst, left, right } => {
            typed!(false, Float, left, right);
            typed!(true, Float, dst);
        }
        Instruction::FloatCompare {
            dst, left, right, ..
        } => {
            typed!(false, Float, left, right);
            typed!(true, Bool, dst);
        }
        Instruction::IntCompare {
            dst, left, right, ..
        } => {
            typed!(false, Int, left, right);
            typed!(true, Bool, dst);
        }
        Instruction::FloatCompareConst { dst, value, .. } => {
            typed!(false, Float, value);
            typed!(true, Bool, dst);
        }
        Instruction::ValueEqual {
            dst, left, right, ..
        } => {
            *left = visit(*left, false);
            *right = visit(*right, false);
            typed!(true, Bool, dst);
        }
        Instruction::IntJumpLess { left, right, .. }
        | Instruction::IntJumpLessEqual { left, right, .. }
        | Instruction::IntJumpGreater { left, right, .. }
        | Instruction::IntJumpGreaterEqual { left, right, .. }
        | Instruction::IntJumpEqual { left, right, .. } => {
            typed!(false, Int, left, right);
        }
        Instruction::FloatJumpLess { left, right, .. }
        | Instruction::FloatJumpLessEqual { left, right, .. }
        | Instruction::FloatJumpGreater { left, right, .. }
        | Instruction::FloatJumpGreaterEqual { left, right, .. }
        | Instruction::FloatJumpEqual { left, right, .. } => {
            typed!(false, Float, left, right);
        }
        Instruction::FloatJumpLessConst { value, .. }
        | Instruction::FloatJumpLessEqualConst { value, .. }
        | Instruction::FloatJumpGreaterConst { value, .. }
        | Instruction::FloatJumpGreaterEqualConst { value, .. }
        | Instruction::FloatJumpEqualConst { value, .. } => {
            typed!(false, Float, value);
        }
        Instruction::JumpIfFalse { condition, .. } | Instruction::JumpIfTrue { condition, .. } => {
            typed!(false, Bool, condition)
        }
        Instruction::LoopRangeStart { count, .. } => typed!(false, Int, count),
        Instruction::LoopMarksStart { marks, .. } => typed!(false, Marks, marks),
        Instruction::SectionQuery { dst, width, .. } => {
            typed!(false, Int, width);
            typed!(true, Int, dst);
        }
        Instruction::SectionPosition {
            dst,
            width,
            inverse,
        } => {
            typed!(false, Float, width, inverse);
            typed!(true, Float, dst);
        }
        Instruction::QuerySeconds {
            dst,
            seconds: value,
        }
        | Instruction::QueryProgress {
            dst,
            seconds: value,
        }
        | Instruction::FloatAddConst { dst, value, .. }
        | Instruction::FloatSubtractConst { dst, value, .. }
        | Instruction::FloatMultiplyConst { dst, value, .. }
        | Instruction::FloatDivideConst { dst, value, .. }
        | Instruction::FloatRemainderConst { dst, value, .. }
        | Instruction::FloatSubtractFromConst { dst, value, .. }
        | Instruction::FloatDivideIntoConst { dst, value, .. }
        | Instruction::FloatRemainderFromConst { dst, value, .. }
        | Instruction::FloatUnary { dst, value, .. }
        | Instruction::FloatBinaryConst { dst, value, .. }
        | Instruction::ClampConst { dst, value, .. } => {
            typed!(false, Float, value);
            typed!(true, Float, dst);
        }
        Instruction::Clamp {
            dst,
            value,
            min,
            max,
        } => {
            typed!(false, Float, value, min, max);
            typed!(true, Float, dst);
        }
        Instruction::Smoothstep { dst, value } => {
            typed!(false, Float, value);
            typed!(true, Float, dst);
        }
        Instruction::MixFloat {
            dst,
            left,
            right,
            amount,
        } => {
            typed!(false, Float, left, right, amount);
            typed!(true, Float, dst);
        }
        Instruction::MixColor {
            dst,
            left,
            right,
            amount,
        } => {
            typed!(false, Color, left, right);
            typed!(false, Float, amount);
            typed!(true, Color, dst);
        }
        Instruction::ColorBinary {
            dst, left, right, ..
        } => {
            typed!(false, Color, left, right);
            typed!(true, Color, dst);
        }
        Instruction::ColorScale { dst, color, scale } => {
            typed!(false, Color, color);
            typed!(false, Float, scale);
            typed!(true, Color, dst);
        }
        Instruction::ColorComponent { dst, color, .. } => {
            typed!(false, Color, color);
            typed!(true, Float, dst);
        }
        Instruction::ColorInvert { dst, color } => {
            typed!(false, Color, color);
            typed!(true, Color, dst);
        }
        Instruction::Rgb {
            dst,
            red,
            green,
            blue,
        } => {
            typed!(false, Float, red, green, blue);
            typed!(true, Color, dst);
        }
        Instruction::Hsv {
            dst,
            hue,
            saturation,
            value,
        } => {
            typed!(false, Float, hue, saturation, value);
            typed!(true, Color, dst);
        }
        Instruction::Rand { dst, seed } => {
            typed!(false, Float, seed);
            typed!(true, Float, dst);
        }
        Instruction::CurveFloatClamped {
            dst,
            curve,
            position,
            min,
            max,
        } => {
            typed!(false, Curve, curve);
            typed!(false, Float, position, min, max);
            typed!(true, Float, dst);
        }
        Instruction::CurveParamFloatClamped {
            dst,
            position,
            min,
            max,
            ..
        } => {
            typed!(false, Float, position, min, max);
            typed!(true, Float, dst);
        }
        Instruction::GradientColorScaled {
            dst,
            gradient,
            position,
            scale,
        } => {
            typed!(false, Gradient, gradient);
            typed!(false, Float, position, scale);
            typed!(true, Color, dst);
        }
        Instruction::GradientParamColorScaled {
            dst,
            position,
            scale,
            ..
        } => {
            typed!(false, Float, position, scale);
            typed!(true, Color, dst);
        }
        Instruction::CurveCrossing {
            dst,
            curve,
            value,
            before,
        } => {
            typed!(false, Curve, curve);
            typed!(false, Float, value);
            if let Some(before) = before {
                typed!(false, Float, before);
            }
            typed!(true, Float, dst);
        }
        Instruction::CurveParamCrossing {
            dst, value, before, ..
        } => {
            typed!(false, Float, value);
            if let Some(before) = before {
                typed!(false, Float, before);
            }
            typed!(true, Float, dst);
        }
        Instruction::Len { dst, value } => {
            typed!(false, Array, value);
            typed!(true, Int, dst);
        }
        Instruction::Mark { marks, op } => {
            typed!(false, Marks, marks);
            match op {
                MarkOp::Count { dst } => typed!(true, Int, dst),
                MarkOp::At { dst, index } => {
                    typed!(false, Int, index);
                    typed!(true, Float, dst);
                }
                MarkOp::Last { dst, seconds } => {
                    typed!(false, Float, seconds);
                    typed!(true, Float, dst);
                }
                MarkOp::LastIndex { dst, seconds } => {
                    typed!(false, Float, seconds);
                    typed!(true, Int, dst);
                }
            }
        }
        Instruction::ReturnColor(value) => typed!(false, Color, value),
        Instruction::Jump(_) | Instruction::LoopEnd { .. } => {}
    }
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

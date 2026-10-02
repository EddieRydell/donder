use std::collections::{HashMap, HashSet};

use super::bytecode::{
    ColorSlot, FloatSlot, Instruction, IntSlot, MarkOp, NumberSlot, SignalPixel, SlotLayout,
    TargetItemsOp, TargetSource, ValueSlot,
};
use super::types::{Type, Value};

/// Move pure, single-assignment scalar expressions to a frame initialization
/// prefix. Mutable locals and references stay in the pixel body. Hoisting may
/// cross branches only when evaluation is harmless; resource samples retain
/// their ordering relative to any earlier potentially failing instruction.
pub(super) fn hoist_uniform(code: &mut Vec<Instruction>, operands: &mut [ValueSlot]) -> u32 {
    use super::bytecode::ContextRead;
    let mut writes = HashMap::<ValueSlot, usize>::new();
    let mut entry_block = true;
    let metadata = code
        .iter_mut()
        .map(|op| {
            let ordered = matches!(
                op,
                Instruction::CurveParamSample { .. }
                    | Instruction::GradientParamSample { .. }
                    | Instruction::CurveParamCrossing { .. }
                    | Instruction::CurveParamFloatClamped { .. }
                    | Instruction::GradientParamColorScaled { .. }
            );
            let eligible = match op {
                Instruction::LoadIntParam { .. }
                | Instruction::LoadFloatParam { .. }
                | Instruction::LoadBoolParam { .. }
                | Instruction::LoadColorParam { .. } => entry_block,
                Instruction::LoadIntConst { .. }
                | Instruction::LoadFloatConst { .. }
                | Instruction::LoadBoolConst { .. }
                | Instruction::LoadColorConst { .. }
                | Instruction::ContextRead {
                    read: ContextRead::Progress | ContextRead::Seconds | ContextRead::Duration,
                    ..
                }
                | Instruction::FloatArithmetic { .. }
                | Instruction::FloatArithmeticConst { .. }
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
                | Instruction::ColorInvert { .. }
                | Instruction::Rgb { .. }
                | Instruction::Hsv { .. }
                | Instruction::IntToFloat { .. }
                | Instruction::Not { .. }
                | Instruction::NegFloat { .. } => true,
                _ => false,
            };
            if jump_target(op).is_some()
                || matches!(
                    op,
                    Instruction::ReturnColor(_) | Instruction::ReturnValues(_)
                )
            {
                entry_block = false;
            }
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
            (eligible, ordered, dst, reads)
        })
        .collect::<Vec<_>>();
    let mut uniform = HashSet::new();
    let mut lifted = vec![false; code.len()];
    let mut prefix = Vec::new();
    loop {
        let before = prefix.len();
        let mut ordered_prefix = true;
        for (index, (eligible, ordered, dst, reads)) in metadata.iter().enumerate() {
            if let Some(dst) = dst
                && (*eligible || (*ordered && ordered_prefix))
                && !lifted[index]
                && matches!(
                    dst,
                    ValueSlot::Int(_)
                        | ValueSlot::Float(_)
                        | ValueSlot::Bool(_)
                        | ValueSlot::Color(_)
                )
                && writes[dst] == 1
                && reads.iter().all(|slot| uniform.contains(slot))
            {
                uniform.insert(*dst);
                lifted[index] = true;
                prefix.push(code[index].clone());
            }
            // A nonuniform resource, signal read, index, jump, or other unknown
            // operation is a barrier. Ordinary pixel context reads are harmless.
            ordered_prefix &= lifted[index]
                || (*eligible
                    && !matches!(
                        code[index],
                        Instruction::LoadIntParam { .. }
                            | Instruction::LoadFloatParam { .. }
                            | Instruction::LoadBoolParam { .. }
                            | Instruction::LoadColorParam { .. }
                    ))
                || matches!(code[index], Instruction::ContextRead { .. });
        }
        if prefix.len() == before {
            break;
        }
    }
    let mut frame_cache = 0u32;
    for op in &mut *code {
        if let Instruction::SignalSample {
            seconds,
            frame_cache: slot,
            ..
        } = op
            && uniform.contains(&ValueSlot::Float(*seconds))
        {
            *slot = frame_cache;
            frame_cache = frame_cache.saturating_add(1);
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
        match &mut op {
            Instruction::Jump(target)
            | Instruction::JumpIfFalse { target, .. }
            | Instruction::JumpIfTrue { target, .. }
            | Instruction::LoopRangeStart { end: target, .. }
            | Instruction::LoopMarksStart { end: target, .. }
            | Instruction::LoopEnd { start: target, .. } => *target = offsets[*target],
            _ => {}
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
    let targets = code.iter().filter_map(jump_target).collect::<HashSet<_>>();
    let mut copies = HashMap::<ValueSlot, ValueSlot>::new();
    let mut samples = HashMap::<(usize, FloatSlot, SignalPixel<IntSlot>), ColorSlot>::new();
    for (offset, op) in code.iter_mut().enumerate() {
        if targets.contains(&offset) {
            copies.clear();
            samples.clear();
        }
        let mut written = None;
        slots(op, operands, |slot, write| {
            if write {
                written = Some(slot);
                slot
            } else {
                copies.get(&slot).copied().unwrap_or(slot)
            }
        });
        if let Some(dst) = written {
            copies.retain(|key, value| *key != dst && *value != dst);
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
            } else {
                samples.insert((input, seconds, pixel), dst);
            }
        }
        if let Instruction::Move { dst, src } = op
            && dst.index() != *src
        {
            copies.insert(*dst, dst.with_index(*src));
        }
        if jump_target(op).is_some()
            || matches!(
                op,
                Instruction::ReturnColor(_) | Instruction::ReturnValues(_)
            )
        {
            copies.clear();
            samples.clear();
        }
    }

    // Preserve operations that can fail or sample another signal, even when the
    // result is unused. Removing a container must not remove errors in its items.
    let mut needed = HashSet::new();
    let uses = code
        .iter_mut()
        .map(|op| {
            let removable = matches!(
                op,
                Instruction::LoadIntConst { .. }
                    | Instruction::LoadFloatConst { .. }
                    | Instruction::LoadBoolConst { .. }
                    | Instruction::LoadColorConst { .. }
                    | Instruction::LoadArrayConst { .. }
                    | Instruction::Move { .. }
                    | Instruction::MakeArray { .. }
            );
            let mut reads = Vec::new();
            let mut dst = None;
            slots(op, operands, |slot, write| {
                if write {
                    dst = Some(slot);
                } else {
                    reads.push(slot);
                }
                slot
            });
            if !removable {
                needed.extend(reads.iter().copied());
            }
            (removable, dst, reads)
        })
        .collect::<Vec<_>>();
    loop {
        let before = needed.len();
        for (removable, dst, reads) in uses.iter().rev() {
            if *removable && dst.is_some_and(|dst| needed.contains(&dst)) {
                needed.extend(reads.iter().copied());
            }
        }
        if needed.len() == before {
            break;
        }
    }
    let mut offsets = Vec::with_capacity(code.len() + 1);
    let mut index = 0;
    let mut kept = 0;
    code.retain(|op| {
        offsets.push(kept);
        let (removable, dst, _) = &uses[index];
        index += 1;
        let keep = (!removable || dst.is_some_and(|dst| needed.contains(&dst)))
            && !matches!(op, Instruction::Move { dst, src } if dst.index() == *src);
        kept += usize::from(keep);
        keep
    });
    offsets.push(kept);
    for op in code.iter_mut() {
        match op {
            Instruction::Jump(target)
            | Instruction::JumpIfFalse { target, .. }
            | Instruction::JumpIfTrue { target, .. }
            | Instruction::LoopRangeStart { end: target, .. }
            | Instruction::LoopMarksStart { end: target, .. }
            | Instruction::LoopEnd { start: target, .. } => *target = offsets[*target],
            _ => {}
        }
    }

    // Rebuild only resources and registers actually named by surviving code.
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
            | Instruction::ReturnValues(span)
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
                        ValueSlot::TargetItem(_) => Type::TargetItem,
                        ValueSlot::TargetItems(_) => Type::TargetItems,
                        ValueSlot::Target(_) => Type::Target,
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
    match op {
        Instruction::Jump(target)
        | Instruction::JumpIfFalse { target, .. }
        | Instruction::JumpIfTrue { target, .. }
        | Instruction::LoopRangeStart { end: target, .. }
        | Instruction::LoopMarksStart { end: target, .. }
        | Instruction::LoopEnd { start: target, .. } => Some(*target),
        _ => None,
    }
}

/// One exhaustive register-operand description, used only by compilation.
/// Operand spans are unique per instruction in compiler output.
fn slots(
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
        Instruction::LoadTargetConst { dst, .. } | Instruction::LoadTargetParam { dst, .. } => {
            typed!(true, Target, dst)
        }
        Instruction::LoadTargetItemsConst { dst, .. }
        | Instruction::LoadTargetItemsParam { dst, .. } => typed!(true, TargetItems, dst),
        Instruction::LoadTargetItemConst { dst, .. }
        | Instruction::LoadTargetItemParam { dst, .. } => typed!(true, TargetItem, dst),
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
        Instruction::MemberInt { dst, target, .. } => {
            typed!(false, TargetItem, target);
            typed!(true, Int, dst);
        }
        Instruction::MemberFraction { dst, target } => {
            typed!(false, TargetItem, target);
            typed!(true, Float, dst);
        }
        Instruction::TargetCount { dst, source } => {
            typed!(false, TargetItems, source);
            typed!(true, Int, dst);
        }
        Instruction::TargetPick { dst, source, index } => {
            typed!(false, TargetItems, source);
            number!(index);
            typed!(true, TargetItem, dst);
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
        Instruction::FloatArithmetic {
            dst, left, right, ..
        }
        | Instruction::FloatBinary {
            dst, left, right, ..
        } => {
            typed!(false, Float, left, right);
            typed!(true, Float, dst);
        }
        Instruction::IntArithmetic {
            dst, left, right, ..
        } => {
            typed!(false, Int, left, right);
            typed!(true, Int, dst);
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
        Instruction::JumpIfFalse { condition, .. } | Instruction::JumpIfTrue { condition, .. } => {
            typed!(false, Bool, condition)
        }
        Instruction::LoopRangeStart { count, .. } => typed!(false, Int, count),
        Instruction::LoopMarksStart { marks, .. } => typed!(false, Marks, marks),
        Instruction::SectionPosition { dst, width } => {
            typed!(false, Float, width);
            typed!(true, Float, dst);
        }
        Instruction::FloatArithmeticConst { dst, value, .. }
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
        Instruction::Smoothstep {
            dst,
            edge0,
            edge1,
            value,
        } => {
            typed!(false, Float, edge0, edge1, value);
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
            fallback,
        } => {
            typed!(false, Curve, curve);
            typed!(false, Float, value);
            if let Some(fallback) = fallback {
                typed!(false, Float, fallback);
            }
            typed!(true, Float, dst);
        }
        Instruction::CurveParamCrossing {
            dst,
            value,
            fallback,
            ..
        } => {
            typed!(false, Float, value);
            if let Some(fallback) = fallback {
                typed!(false, Float, fallback);
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
                MarkOp::At {
                    dst,
                    index,
                    fallback,
                } => {
                    number!(index);
                    if let Some(value) = fallback {
                        number!(value);
                    }
                    typed!(true, Float, dst);
                }
                MarkOp::Prev {
                    dst,
                    seconds,
                    fallback,
                } => {
                    if let Some(value) = seconds {
                        number!(value);
                    }
                    if let Some(value) = fallback {
                        number!(value);
                    }
                    typed!(true, Float, dst);
                }
                MarkOp::PrevIndex { dst, seconds } | MarkOp::NextIndex { dst, seconds } => {
                    if let Some(value) = seconds {
                        number!(value);
                    }
                    typed!(true, Int, dst);
                }
                MarkOp::Elapsed { dst, seconds } | MarkOp::Phase { dst, seconds } => {
                    if let Some(value) = seconds {
                        number!(value);
                    }
                    typed!(true, Float, dst);
                }
            }
        }
        Instruction::TargetItems { source, op } => {
            match source {
                TargetSource::Target(slot) => typed!(false, Target, slot),
                TargetSource::Items(slot) => typed!(false, TargetItems, slot),
                TargetSource::Item(slot) => typed!(false, TargetItem, slot),
            }
            match op {
                TargetItemsOp::Fixtures { dst } | TargetItemsOp::Pixels { dst } => {
                    typed!(true, TargetItems, dst)
                }
                TargetItemsOp::Sections { dst, width } => {
                    number!(width);
                    typed!(true, TargetItems, dst);
                }
            }
        }
        Instruction::ReturnValues(outputs) => {
            for value in
                &mut operands[outputs.start as usize..(outputs.start + outputs.len) as usize]
            {
                *value = visit(*value, false);
            }
        }
        Instruction::ReturnColor(value) => typed!(false, Color, value),
        Instruction::Jump(_) | Instruction::LoopEnd { .. } => {}
    }
}

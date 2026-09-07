use std::collections::{HashMap, HashSet};

use super::bytecode::{
    ColorSlot, FloatSlot, Instruction, IntSlot, SignalPixel, SlotLayout, ValueSlot,
};
use super::types::{Identifier, Type, Value};

/// Move pure, single-assignment scalar expressions to a frame initialization
/// prefix. Mutable locals and references stay in the pixel body. Hoisting may
/// cross branches only when evaluation is harmless; resource samples retain
/// their ordering relative to any earlier potentially failing instruction.
pub(super) fn hoist_uniform(
    code: &mut Vec<Instruction>,
    operands: &mut [ValueSlot],
    fields: &mut [(Identifier, ValueSlot)],
) -> u32 {
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
                Instruction::LoadConst { .. }
                | Instruction::ContextRead {
                    read: ContextRead::Progress | ContextRead::Seconds | ContextRead::Duration,
                    ..
                }
                | Instruction::FloatArithmetic { .. }
                | Instruction::FloatArithmeticConst { .. }
                | Instruction::FloatBinary { .. }
                | Instruction::FloatBinaryConst { .. }
                | Instruction::FloatCompare { .. }
                | Instruction::FloatCompareConst { .. }
                | Instruction::FloatUnary { .. }
                | Instruction::MixFloat { .. }
                | Instruction::MixColor { .. }
                | Instruction::ColorBinary { .. }
                | Instruction::ColorScale { .. }
                | Instruction::ColorIntensity { .. }
                | Instruction::ColorInvert { .. }
                | Instruction::Rgb { .. }
                | Instruction::Hsv { .. }
                | Instruction::IntToFloat { .. }
                | Instruction::Not { .. }
                | Instruction::NegFloat { .. } => true,
                _ => false,
            };
            if jump_target(op).is_some()
                || matches!(op, Instruction::Return(_) | Instruction::ReturnColor(_))
            {
                entry_block = false;
            }
            let mut dst = None;
            let mut reads = Vec::new();
            slots(op, operands, fields, |slot, write| {
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
                && !matches!(dst, ValueSlot::Ref(_))
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
            | Instruction::JumpIfTrue { target, .. } => *target = offsets[*target],
            _ => {}
        }
        prefix.push(op);
    }
    *code = prefix;
    entry as u32
}

pub(super) fn cleanup(
    code: &mut Vec<Instruction>,
    constants: &mut Vec<Value>,
    operands: &mut Vec<ValueSlot>,
    fields: &mut Vec<(Identifier, ValueSlot)>,
    layout: &mut SlotLayout,
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
        slots(op, operands, fields, |slot, write| {
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
                    src: ValueSlot::Color(src),
                };
            } else {
                samples.insert((input, seconds, pixel), dst);
            }
        }
        if let Instruction::Move { dst, src } = op
            && core::mem::discriminant(dst) == core::mem::discriminant(src)
            && dst != src
        {
            copies.insert(*dst, *src);
        }
        if jump_target(op).is_some()
            || matches!(op, Instruction::Return(_) | Instruction::ReturnColor(_))
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
                Instruction::LoadConst { .. }
                    | Instruction::Move { .. }
                    | Instruction::MakeArray { .. }
            );
            let mut reads = Vec::new();
            let mut dst = None;
            slots(op, operands, fields, |slot, write| {
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
            && !matches!(op, Instruction::Move { dst, src } if dst == src);
        kept += usize::from(keep);
        keep
    });
    offsets.push(kept);
    for op in code.iter_mut() {
        match op {
            Instruction::Jump(target)
            | Instruction::JumpIfFalse { target, .. }
            | Instruction::JumpIfTrue { target, .. } => *target = offsets[*target],
            _ => {}
        }
    }

    // Rebuild only resources and registers actually named by surviving code.
    let old_constants = core::mem::take(constants);
    let old_operands = core::mem::take(operands);
    let old_fields = core::mem::take(fields);
    let mut constant_ids = HashMap::new();
    let mut registers = HashMap::new();
    *layout = SlotLayout::default();
    for op in code {
        match op {
            Instruction::LoadConst { constant, .. }
            | Instruction::EnumParamEqualConst { constant, .. } => {
                *constant = *constant_ids.entry(*constant).or_insert_with(|| {
                    let index = constants.len();
                    constants.push(old_constants[*constant].clone());
                    index
                });
            }
            Instruction::MakeArray { items: span, .. }
            | Instruction::Select { items: span, .. }
            | Instruction::Rand { args: span, .. }
            | Instruction::Mark { args: span, .. }
            | Instruction::TargetItems { args: span, .. } => {
                let values = &old_operands[span.start as usize..(span.start + span.len) as usize];
                span.start = operands.len() as u32;
                operands.extend_from_slice(values);
            }
            Instruction::Emit { fields: span, .. } => {
                let values = &old_fields[span.start as usize..(span.start + span.len) as usize];
                span.start = fields.len() as u32;
                fields.extend_from_slice(values);
            }
            _ => {}
        }
        slots(op, operands, fields, |slot, _| {
            *registers.entry(slot).or_insert_with(|| {
                ValueSlot::for_type(
                    &match slot {
                        ValueSlot::Int(_) => Type::Int,
                        ValueSlot::Float(_) => Type::Float,
                        ValueSlot::Bool(_) => Type::Bool,
                        ValueSlot::Color(_) => Type::Color,
                        ValueSlot::Ref(_) => Type::Void,
                    },
                    layout,
                )
            })
        });
    }
}

fn jump_target(op: &Instruction) -> Option<usize> {
    match op {
        Instruction::Jump(target)
        | Instruction::JumpIfFalse { target, .. }
        | Instruction::JumpIfTrue { target, .. } => Some(*target),
        _ => None,
    }
}

/// One exhaustive register-operand description, used only by compilation.
/// Operand spans are unique per instruction in compiler output.
fn slots(
    op: &mut Instruction,
    operands: &mut [ValueSlot],
    fields: &mut [(Identifier, ValueSlot)],
    mut visit: impl FnMut(ValueSlot, bool) -> ValueSlot,
) {
    macro_rules! typed {
        ($write:expr, $kind:ident, $($slot:ident),+) => {{$(
            let ValueSlot::$kind(mapped) = visit(ValueSlot::$kind(*$slot), $write) else { unreachable!("compiler register remapping preserves types") };
            *$slot = mapped;
        )+}};
    }
    match op {
        Instruction::LoadConst { dst, .. }
        | Instruction::LoadGeneratorContext { dst, .. }
        | Instruction::ContextRead { dst, .. } => *dst = visit(*dst, true),
        Instruction::LoadIntParam { dst, .. } => typed!(true, Int, dst),
        Instruction::LoadFloatParam { dst, .. } => typed!(true, Float, dst),
        Instruction::LoadBoolParam { dst, .. } | Instruction::EnumParamEqualConst { dst, .. } => {
            typed!(true, Bool, dst)
        }
        Instruction::LoadColorParam { dst, .. } => typed!(true, Color, dst),
        Instruction::LoadRefParam { dst, .. } => typed!(true, Ref, dst),
        Instruction::Move { dst, src } => {
            *src = visit(*src, false);
            *dst = visit(*dst, true);
        }
        Instruction::MakeArray { dst, items } => {
            for slot in &mut operands[items.start as usize..(items.start + items.len) as usize] {
                *slot = visit(*slot, false);
            }
            typed!(true, Ref, dst);
        }
        Instruction::Index { dst, target, index } => {
            typed!(false, Ref, target);
            *index = visit(*index, false);
            *dst = visit(*dst, true);
        }
        Instruction::Select { dst, items, index } => {
            for slot in &mut operands[items.start as usize..(items.start + items.len) as usize] {
                *slot = visit(*slot, false);
            }
            *index = visit(*index, false);
            *dst = visit(*dst, true);
        }
        Instruction::Member { dst, target, .. } => {
            typed!(false, Ref, target);
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
        Instruction::ColorIntensity { dst, color } => {
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
        Instruction::Rand { dst, args } => {
            for slot in &mut operands[args.start as usize..(args.start + args.len) as usize] {
                *slot = visit(*slot, false);
            }
            typed!(true, Float, dst);
        }
        Instruction::CurveFloatClamped {
            dst,
            curve,
            position,
            min,
            max,
        } => {
            typed!(false, Ref, curve);
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
            typed!(false, Ref, gradient);
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
            typed!(false, Ref, curve);
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
            typed!(false, Ref, value);
            typed!(true, Int, dst);
        }
        Instruction::Mark { dst, args, .. } | Instruction::TargetItems { dst, args, .. } => {
            for slot in &mut operands[args.start as usize..(args.start + args.len) as usize] {
                *slot = visit(*slot, false);
            }
            *dst = visit(*dst, true);
        }
        Instruction::Emit { fields: span, .. } => {
            for (_, slot) in &mut fields[span.start as usize..(span.start + span.len) as usize] {
                *slot = visit(*slot, false);
            }
        }
        Instruction::Return(value) => *value = visit(*value, false),
        Instruction::ReturnColor(value) => typed!(false, Color, value),
        Instruction::Jump(_) | Instruction::CheckLoopLimit => {}
    }
}

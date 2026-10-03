//! Compile-time scalar propagation and liveness. No analysis state is retained
//! by a prepared program or allocated during playback.
use std::collections::{HashMap, HashSet, VecDeque};

use super::{comparison_branch, slots};
use crate::dsl::bytecode::{
    CompareOp, FloatBinary, FloatUnary as UnaryFloat, Instruction, ValueSlot,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Known {
    Int(i32),
    Float(u32),
    Bool(bool),
    Color(crate::values::Color),
    Copy(ValueSlot),
    /// A present scalar, with no compile-time numeric value. Missing resource
    /// samples use NaN and must reach value_or before annihilating identities.
    Real,
}

type State = HashMap<ValueSlot, Known>;

/// Partially evaluate the immutable initialization prefix using fixed bindings.
/// Folded results remain parameters, so their values do not create code variants.
/// Only results used outside the folded expression graph become retained inputs.
pub(in crate::dsl) fn prepare_bindings(
    code: &mut Vec<Instruction>,
    operands: &mut [ValueSlot],
    mut entry: usize,
    layout: &mut crate::dsl::bytecode::SlotLayout,
    types: &mut Vec<crate::dsl::Type>,
    values: &mut Vec<crate::dsl::Value>,
    dynamic: impl Fn(usize) -> bool,
) -> u32 {
    use crate::dsl::bytecode::{BoolSlot, ColorSlot, FloatSlot, IntSlot};
    use crate::dsl::{Type, Value};
    use Instruction::*;
    let mut state = State::new();
    let mut parameter_dependent = HashSet::new();
    let mut folded = vec![None; entry];
    let mut reads = Vec::with_capacity(code.len());
    for (ip, op) in code.iter_mut().enumerate() {
        let mut inputs = Vec::new();
        slots(op, operands, |slot, write| {
            if !write {
                inputs.push(slot);
            }
            slot
        });
        if ip < entry {
            let param = match *op {
                LoadIntParam { param, .. }
                | LoadFloatParam { param, .. }
                | LoadBoolParam { param, .. }
                | LoadColorParam { param, .. }
                    if !dynamic(param) =>
                {
                    Some(param)
                }
                _ => None,
            };
            let value = if let Some(param) = param {
                match values[param] {
                    Value::Int(value) => Some(Known::Int(value)),
                    Value::Float(value) => Some(Known::Float(value.to_bits())),
                    Value::Bool(value) => Some(Known::Bool(value)),
                    Value::Color(value) => Some(Known::Color(value)),
                    _ => unreachable!("admitted primitive parameter"),
                }
            } else {
                result(op, &state)
            };
            if let Some(dst) = op.written_slot()
                && let Some(
                    value @ (Known::Int(_) | Known::Float(_) | Known::Bool(_) | Known::Color(_)),
                ) = value
            {
                state.insert(dst, value);
                let depends =
                    param.is_some() || inputs.iter().any(|slot| parameter_dependent.contains(slot));
                if depends {
                    parameter_dependent.insert(dst);
                    // Existing parameter loads already are the desired operation.
                    if param.is_none() {
                        folded[ip] = Some(value);
                    }
                }
            }
        }
        reads.push(inputs);
    }
    // Preparation knows fixed divisors that range analysis could not prove at
    // compilation. Keep their inverses in bindings too, so numeric values do
    // not multiply the number of shared program variants.
    let mut parameters = HashMap::new();
    let mut aliases = HashMap::new();
    for op in &code[..entry] {
        if let LoadFloatParam { dst, param, .. } = *op {
            aliases.insert(dst, *parameters.entry(param).or_insert(dst));
        }
    }
    let mut inverses = HashMap::new();
    let mut prefix = Vec::new();
    for (ip, op) in code.iter_mut().enumerate().skip(entry) {
        let FloatDivide { dst, left, right } = *op else {
            continue;
        };
        // State contains only the admitted immutable prefix. A mutable body
        // register cannot have a known value here.
        let Some(divisor) = float(&state, right) else {
            continue;
        };
        let inverse = divisor.recip();
        if !divisor.is_finite() || divisor == 0.0 || !inverse.is_finite() {
            continue;
        }
        let denominator = aliases.get(&right).copied().unwrap_or(right);
        let inverse_slot = if let Some(&slot) = inverses.get(&denominator) {
            slot
        } else {
            let Some(next) = layout.floats.checked_add(1) else {
                continue;
            };
            let slot = FloatSlot(layout.floats);
            layout.floats = next;
            inverses.insert(denominator, slot);
            prefix.push(FloatDivideIntoConst {
                dst: slot,
                value: right,
                constant_bits: 1.0f32.to_bits(),
            });
            folded.push(Some(Known::Float(inverse.to_bits())));
            slot
        };
        *op = FloatMultiply {
            dst,
            left,
            right: inverse_slot,
        };
        reads[ip] = vec![ValueSlot::Float(left), ValueSlot::Float(inverse_slot)];
    }
    if !prefix.is_empty() {
        for op in code.iter_mut() {
            if let Some(target) = op.jump_target_mut()
                && *target >= entry
            {
                *target += prefix.len();
            }
        }
        let next_entry = entry + prefix.len();
        reads.splice(
            entry..entry,
            prefix.iter().map(|op| {
                let FloatDivideIntoConst { value, .. } = *op else {
                    unreachable!()
                };
                vec![ValueSlot::Float(value)]
            }),
        );
        code.splice(entry..entry, prefix);
        entry = next_entry;
    }
    let mut live: HashSet<_> = reads[entry..].iter().flatten().copied().collect();
    let mut keep = vec![true; code.len()];
    // Keep retained inputs in instruction order, independent of their values.
    let mut retained = Vec::new();
    for ip in (0..entry).rev() {
        let op = &code[ip];
        let Some(dst) = op.written_slot() else {
            live.extend(reads[ip].iter().copied());
            continue;
        };
        let static_load = matches!(*op,
            LoadIntParam { param, .. } | LoadFloatParam { param, .. }
                | LoadBoolParam { param, .. } | LoadColorParam { param, .. }
                if !dynamic(param));
        if !live.contains(&dst) && (removable(op) || static_load) {
            keep[ip] = false;
            continue;
        }
        if let Some(value) = folded[ip] {
            retained.push((ip, dst, value));
        } else {
            live.extend(reads[ip].iter().copied());
        }
    }
    let used_params: HashSet<_> = code
        .iter()
        .zip(&keep)
        .filter_map(|(op, keep)| {
            if !keep {
                return None;
            }
            match *op {
                LoadIntParam { param, .. }
                | LoadFloatParam { param, .. }
                | LoadBoolParam { param, .. }
                | LoadColorParam { param, .. } => Some(param),
                _ => None,
            }
        })
        .collect();
    let mut reusable: Vec<_> = (0..types.len())
        .filter(|index| !dynamic(*index) && !used_params.contains(index))
        .collect();
    for (ip, dst, value) in retained.into_iter().rev() {
        let ty = match value {
            Known::Int(_) => Type::Int,
            Known::Float(_) => Type::Float,
            Known::Bool(_) => Type::Bool,
            Known::Color(_) => Type::Color,
            _ => unreachable!("retained primitive result"),
        };
        // Declaration and automation addresses stay stable. A fixed input no
        // longer read by the prepared program can hold a result of the same type.
        let param = reusable
            .iter()
            .position(|index| types[*index] == ty)
            .map_or(values.len(), |index| reusable.remove(index));
        let source = types[..param]
            .iter()
            .filter(|candidate| **candidate == ty)
            .count() as u32;
        let (ty, value, instruction) = match (dst, value) {
            (ValueSlot::Int(dst), Known::Int(value)) => (
                Type::Int,
                Value::Int(value),
                LoadIntParam {
                    dst,
                    param,
                    source: IntSlot(source),
                },
            ),
            (ValueSlot::Float(dst), Known::Float(bits)) => (
                Type::Float,
                Value::Float(f32::from_bits(bits)),
                LoadFloatParam {
                    dst,
                    param,
                    source: FloatSlot(source),
                },
            ),
            (ValueSlot::Bool(dst), Known::Bool(value)) => (
                Type::Bool,
                Value::Bool(value),
                LoadBoolParam {
                    dst,
                    param,
                    source: BoolSlot(source),
                },
            ),
            (ValueSlot::Color(dst), Known::Color(value)) => (
                Type::Color,
                Value::Color(value),
                LoadColorParam {
                    dst,
                    param,
                    source: ColorSlot(source),
                },
            ),
            _ => unreachable!("constant propagation preserves primitive types"),
        };
        if param == values.len() {
            types.push(ty);
            values.push(value);
        } else {
            values[param] = value;
        }
        code[ip] = instruction;
    }
    let entry = keep[..entry].iter().filter(|keep| **keep).count() as u32;
    compact(code, &keep);
    entry
}

fn resolve(state: &State, slot: ValueSlot) -> ValueSlot {
    match state.get(&slot) {
        Some(Known::Copy(source)) => *source,
        _ => slot,
    }
}

fn known(state: &State, slot: ValueSlot) -> Option<Known> {
    state.get(&resolve(state, slot)).copied()
}

fn real(state: &State, slot: crate::dsl::bytecode::FloatSlot) -> bool {
    match known(state, ValueSlot::Float(slot)) {
        Some(Known::Real) => true,
        Some(Known::Float(bits)) => f32::from_bits(bits).is_finite(),
        _ => false,
    }
}

fn present_result(op: &Instruction, state: &State) -> Option<Known> {
    use Instruction::*;
    let present = match *op {
        IntToFloat { .. } => true,
        FloatBinary {
            op: crate::dsl::bytecode::FloatBinary::ValueOr,
            right,
            ..
        } => real(state, right),
        FloatBinaryConst {
            op: crate::dsl::bytecode::FloatBinary::ValueOr,
            constant_bits,
            ..
        } => f32::from_bits(constant_bits).is_finite(),
        NegFloat { src, .. } => real(state, src),
        _ => false,
    };
    present.then_some(Known::Real)
}

fn float(state: &State, slot: crate::dsl::bytecode::FloatSlot) -> Option<f32> {
    match known(state, ValueSlot::Float(slot))? {
        Known::Float(bits) => Some(f32::from_bits(bits)),
        _ => None,
    }
}

fn int(state: &State, slot: crate::dsl::bytecode::IntSlot) -> Option<i32> {
    match known(state, ValueSlot::Int(slot))? {
        Known::Int(value) => Some(value),
        _ => None,
    }
}

fn color(state: &State, slot: crate::dsl::bytecode::ColorSlot) -> Option<crate::values::Color> {
    match known(state, ValueSlot::Color(slot))? {
        Known::Color(value) => Some(value),
        _ => None,
    }
}

fn compare<T: PartialOrd>(op: CompareOp, left: T, right: T) -> bool {
    match op {
        CompareOp::Less => left < right,
        CompareOp::LessEqual => left <= right,
        CompareOp::Greater => left > right,
        CompareOp::GreaterEqual => left >= right,
    }
}

fn result(op: &Instruction, state: &State) -> Option<Known> {
    use Instruction::*;
    let f = |value: f32| Some(Known::Float(value.to_bits()));
    let i = |value| Some(Known::Int(value));
    let b = |value| Some(Known::Bool(value));
    match *op {
        LoadIntConst { value, .. } => i(value),
        LoadFloatConst { bits, .. } => Some(Known::Float(bits)),
        LoadBoolConst { value, .. } => b(value),
        LoadColorConst { value, .. } => Some(Known::Color(value)),
        Rgb {
            red, green, blue, ..
        } => Some(Known::Color(crate::sampling::rgb(
            float(state, red)?,
            float(state, green)?,
            float(state, blue)?,
        ))),
        Hsv {
            hue,
            saturation,
            value,
            ..
        } => Some(Known::Color(crate::sampling::hsv(
            float(state, hue)?,
            float(state, saturation)?,
            float(state, value)?,
        ))),
        ColorInvert { color: value, .. } => Some(Known::Color(crate::sampling::invert_color(
            color(state, value)?,
        ))),
        ColorScale {
            color: value,
            scale,
            ..
        } => Some(Known::Color(crate::sampling::scale_color(
            color(state, value)?,
            float(state, scale)?,
        ))),
        MixColor {
            left,
            right,
            amount,
            ..
        } => Some(Known::Color(crate::sampling::mix_colors(
            color(state, left)?,
            color(state, right)?,
            float(state, amount)?,
        ))),
        ColorBinary {
            op, left, right, ..
        } => {
            let left = color(state, left)?;
            let right = color(state, right)?;
            Some(Known::Color(match op {
                crate::dsl::bytecode::ColorBinary::Add => crate::sampling::add_colors(left, right),
                crate::dsl::bytecode::ColorBinary::Multiply => {
                    crate::sampling::multiply_colors(left, right)
                }
                crate::dsl::bytecode::ColorBinary::Max => crate::sampling::max_colors(left, right),
            }))
        }
        Move { dst, src } => {
            let source = resolve(state, dst.with_index(src));
            Some(known(state, source).unwrap_or(Known::Copy(source)))
        }
        IntToFloat { src, .. } => f(int(state, src)? as f32),
        Smoothstep { value, .. } => {
            let t = float(state, value)?.clamp(0.0, 1.0);
            f(t * t * (3.0 - 2.0 * t))
        }
        Clamp {
            value, min, max, ..
        } => f(crate::sampling::clamp_float(
            float(state, value)?,
            float(state, min)?,
            float(state, max)?,
        )),
        ClampConst {
            value,
            min_bits,
            max_bits,
            ..
        } => f(crate::sampling::clamp_float(
            float(state, value)?,
            f32::from_bits(min_bits),
            f32::from_bits(max_bits),
        )),
        Rand { seed, .. } => f(crate::sampling::deterministic_random_seed(float(
            state, seed,
        )?)),
        NegInt { src, .. } => i(int(state, src)?.wrapping_neg()),
        NegFloat { src, .. } => f(-float(state, src)?),
        Not { src, .. } => match known(state, ValueSlot::Bool(src))? {
            Known::Bool(value) => b(!value),
            _ => None,
        },
        IntAdd { left, right, .. } => i(int(state, left)?.wrapping_add(int(state, right)?)),
        IntSubtract { left, right, .. } => i(int(state, left)?.wrapping_sub(int(state, right)?)),
        IntMultiply { left, right, .. } => i(int(state, left)?.wrapping_mul(int(state, right)?)),
        IntRemainder { left, right, .. } => i(int(state, left)?
            .checked_rem(int(state, right)?)
            .unwrap_or(0)),
        FloatAdd { left, right, .. } => f(float(state, left)? + float(state, right)?),
        FloatSubtract { left, right, .. } => f(float(state, left)? - float(state, right)?),
        FloatMultiply { left, right, .. } => f(float(state, left)? * float(state, right)?),
        FloatDivide { left, right, .. } => f(float(state, left)? / float(state, right)?),
        FloatRemainder { left, right, .. } => f(float(state, left)? % float(state, right)?),
        FloatAddConst {
            value,
            constant_bits,
            ..
        } => f(float(state, value)? + f32::from_bits(constant_bits)),
        FloatSubtractConst {
            value,
            constant_bits,
            ..
        } => f(float(state, value)? - f32::from_bits(constant_bits)),
        FloatMultiplyConst {
            value,
            constant_bits,
            ..
        } => f(float(state, value)? * f32::from_bits(constant_bits)),
        FloatDivideConst {
            value,
            constant_bits,
            ..
        } => f(float(state, value)? / f32::from_bits(constant_bits)),
        FloatRemainderConst {
            value,
            constant_bits,
            ..
        } => f(float(state, value)? % f32::from_bits(constant_bits)),
        FloatSubtractFromConst {
            value,
            constant_bits,
            ..
        } => f(f32::from_bits(constant_bits) - float(state, value)?),
        FloatDivideIntoConst {
            value,
            constant_bits,
            ..
        } => f(f32::from_bits(constant_bits) / float(state, value)?),
        FloatRemainderFromConst {
            value,
            constant_bits,
            ..
        } => f(f32::from_bits(constant_bits) % float(state, value)?),
        FloatCompare {
            op, left, right, ..
        } => b(compare(op, float(state, left)?, float(state, right)?)),
        IntCompare {
            op, left, right, ..
        } => b(compare(op, int(state, left)?, int(state, right)?)),
        FloatCompareConst {
            op,
            value,
            constant_bits,
            constant_left,
            ..
        } => {
            let value = float(state, value)?;
            let constant = f32::from_bits(constant_bits);
            b(if constant_left {
                compare(op, constant, value)
            } else {
                compare(op, value, constant)
            })
        }
        FloatUnary { op, value, .. } => {
            let value = float(state, value)?;
            match op {
                UnaryFloat::Abs => f(value.abs()),
                UnaryFloat::Floor => f(value.floor()),
                UnaryFloat::Sqrt => f(libm::sqrtf(value)),
                // Transcendental implementations differ between host and device.
                UnaryFloat::Sin | UnaryFloat::Cos => None,
            }
        }
        FloatBinary {
            op, left, right, ..
        } => binary(op, float(state, left)?, float(state, right)?),
        FloatBinaryConst {
            op,
            value,
            constant_bits,
            ..
        } => binary(op, float(state, value)?, f32::from_bits(constant_bits)),
        ValueEqual {
            left,
            right,
            negate,
            ..
        } => {
            let equal = match (known(state, left)?, known(state, right)?) {
                (Known::Int(a), Known::Int(b)) => a == b,
                (Known::Float(a), Known::Float(b)) => f32::from_bits(a) == f32::from_bits(b),
                (Known::Bool(a), Known::Bool(b)) => a == b,
                (Known::Color(a), Known::Color(b)) => a == b,
                _ => return None,
            };
            b(equal != negate)
        }
        _ => None,
    }
}

fn binary(op: FloatBinary, left: f32, right: f32) -> Option<Known> {
    Some(Known::Float(
        match op {
            FloatBinary::Min | FloatBinary::Max if left.is_nan() || right.is_nan() => f32::NAN,
            FloatBinary::Min => left.min(right),
            FloatBinary::Max => left.max(right),
            FloatBinary::Atan2 => libm::atan2f(left, right),
            FloatBinary::ValueOr => {
                if left.is_nan() {
                    right
                } else {
                    left
                }
            }
        }
        .to_bits(),
    ))
}

pub(super) fn successors(code: &[Instruction], ip: usize) -> Vec<usize> {
    let mut next = match &code[ip] {
        Instruction::ReturnColor(_) => Vec::new(),
        Instruction::Jump(target) => vec![*target],
        Instruction::LoopRangeStart { end, .. } | Instruction::LoopMarksStart { end, .. } => {
            vec![ip + 1, end + 1]
        }
        Instruction::LoopEnd { start, .. } => vec![*start, ip + 1],
        instruction => instruction
            .conditional_target()
            .map_or_else(|| vec![ip + 1], |target| vec![ip + 1, target]),
    };
    next.retain(|&ip| ip < code.len());
    next
}

fn states(code: &[Instruction]) -> Vec<Option<State>> {
    let mut states = vec![None; code.len()];
    if code.is_empty() {
        return states;
    }
    states[0] = Some(State::new());
    let mut pending = VecDeque::from([0]);
    let mut queued = vec![false; code.len()];
    queued[0] = true;
    while let Some(ip) = pending.pop_front() {
        queued[ip] = false;
        let Some(mut outgoing) = states[ip].clone() else {
            unreachable!("only reachable instructions enter the worklist")
        };
        let op = &code[ip];
        if let Some(dst) = op.written_slot() {
            let value = result(op, &outgoing).or_else(|| present_result(op, &outgoing));
            outgoing.retain(|slot, value| *slot != dst && *value != Known::Copy(dst));
            if let Some(value) = value
                && value != Known::Copy(dst)
            {
                outgoing.insert(dst, value);
            }
        }
        for next in successors(code, ip) {
            let changed = if let Some(incoming) = &mut states[next] {
                let old_len = incoming.len();
                incoming.retain(|slot, value| outgoing.get(slot) == Some(value));
                old_len != incoming.len()
            } else {
                states[next] = Some(outgoing.clone());
                true
            };
            if changed && !queued[next] {
                pending.push_back(next);
                queued[next] = true;
            }
        }
    }
    states
}

fn branch_value(op: &Instruction, state: &State) -> Option<bool> {
    use Instruction::*;
    macro_rules! branch {
        ($left:expr, $op:tt, $right:expr, $when:expr) => { Some(($left $op $right) == $when) };
    }
    match *op {
        JumpIfFalse { condition, .. } | JumpIfTrue { condition, .. } => {
            let Known::Bool(value) = known(state, ValueSlot::Bool(condition))? else {
                return None;
            };
            Some(value == matches!(op, JumpIfTrue { .. }))
        }
        IntJumpLess {
            left, right, when, ..
        } => branch!(int(state, left)?, <, int(state, right)?, when),
        IntJumpLessEqual {
            left, right, when, ..
        } => branch!(int(state, left)?, <=, int(state, right)?, when),
        IntJumpGreater {
            left, right, when, ..
        } => branch!(int(state, left)?, >, int(state, right)?, when),
        IntJumpGreaterEqual {
            left, right, when, ..
        } => branch!(int(state, left)?, >=, int(state, right)?, when),
        IntJumpEqual {
            left, right, when, ..
        } => branch!(int(state, left)?, ==, int(state, right)?, when),
        FloatJumpLess {
            left, right, when, ..
        } => branch!(float(state, left)?, <, float(state, right)?, when),
        FloatJumpLessEqual {
            left, right, when, ..
        } => branch!(float(state, left)?, <=, float(state, right)?, when),
        FloatJumpGreater {
            left, right, when, ..
        } => branch!(float(state, left)?, >, float(state, right)?, when),
        FloatJumpGreaterEqual {
            left, right, when, ..
        } => branch!(float(state, left)?, >=, float(state, right)?, when),
        FloatJumpEqual {
            left, right, when, ..
        } => branch!(float(state, left)?, ==, float(state, right)?, when),
        FloatJumpLessConst {
            value,
            constant_bits,
            when,
            ..
        } => branch!(float(state, value)?, <, f32::from_bits(constant_bits), when),
        FloatJumpLessEqualConst {
            value,
            constant_bits,
            when,
            ..
        } => branch!(float(state, value)?, <=, f32::from_bits(constant_bits), when),
        FloatJumpGreaterConst {
            value,
            constant_bits,
            when,
            ..
        } => branch!(float(state, value)?, >, f32::from_bits(constant_bits), when),
        FloatJumpGreaterEqualConst {
            value,
            constant_bits,
            when,
            ..
        } => branch!(float(state, value)?, >=, f32::from_bits(constant_bits), when),
        FloatJumpEqualConst {
            value,
            constant_bits,
            when,
            ..
        } => branch!(float(state, value)?, ==, f32::from_bits(constant_bits), when),
        _ => None,
    }
}

fn simplify(op: &Instruction, state: &State) -> Option<Instruction> {
    use Instruction::*;
    if let Some(dst) = op.written_slot() {
        match (dst, result(op, state)) {
            (ValueSlot::Int(dst), Some(Known::Int(value))) => {
                return Some(LoadIntConst { dst, value });
            }
            (ValueSlot::Float(dst), Some(Known::Float(bits))) => {
                return Some(LoadFloatConst { dst, bits });
            }
            (ValueSlot::Bool(dst), Some(Known::Bool(value))) => {
                return Some(LoadBoolConst { dst, value });
            }
            (ValueSlot::Color(dst), Some(Known::Color(value))) => {
                return Some(LoadColorConst { dst, value });
            }
            _ => {}
        }
    }
    macro_rules! immediate {
        ($dst:expr, $left:expr, $right:expr, $normal:ident, $reverse:ident) => {{
            if let Some(value) = float(state, $right) {
                Some($normal {
                    dst: $dst,
                    value: $left,
                    constant_bits: value.to_bits(),
                })
            } else {
                float(state, $left).map(|value| $reverse {
                    dst: $dst,
                    value: $right,
                    constant_bits: value.to_bits(),
                })
            }
        }};
    }
    macro_rules! branch_immediate {
        ($left:expr, $right:expr, $when:expr, $target:expr, $normal:ident, $reverse:ident) => {{
            if let Some(value) = float(state, $right) {
                Some($normal {
                    value: $left,
                    constant_bits: value.to_bits(),
                    when: $when,
                    target: $target,
                })
            } else {
                float(state, $left).map(|value| $reverse {
                    value: $right,
                    constant_bits: value.to_bits(),
                    when: $when,
                    target: $target,
                })
            }
        }};
    }
    let copy = |dst, value| {
        Some(Move {
            dst: ValueSlot::Float(dst),
            src: value,
        })
    };
    match *op {
        FloatJumpLess {
            left,
            right,
            when,
            target,
        } => branch_immediate!(
            left,
            right,
            when,
            target,
            FloatJumpLessConst,
            FloatJumpGreaterConst
        ),
        FloatJumpLessEqual {
            left,
            right,
            when,
            target,
        } => branch_immediate!(
            left,
            right,
            when,
            target,
            FloatJumpLessEqualConst,
            FloatJumpGreaterEqualConst
        ),
        FloatJumpGreater {
            left,
            right,
            when,
            target,
        } => branch_immediate!(
            left,
            right,
            when,
            target,
            FloatJumpGreaterConst,
            FloatJumpLessConst
        ),
        FloatJumpGreaterEqual {
            left,
            right,
            when,
            target,
        } => branch_immediate!(
            left,
            right,
            when,
            target,
            FloatJumpGreaterEqualConst,
            FloatJumpLessEqualConst
        ),
        FloatJumpEqual {
            left,
            right,
            when,
            target,
        } => branch_immediate!(
            left,
            right,
            when,
            target,
            FloatJumpEqualConst,
            FloatJumpEqualConst
        ),
        FloatCompare {
            dst,
            op,
            left,
            right,
        } => {
            if let Some(value) = float(state, right) {
                Some(FloatCompareConst {
                    dst,
                    op,
                    value: left,
                    constant_bits: value.to_bits(),
                    constant_left: false,
                })
            } else {
                float(state, left).map(|value| FloatCompareConst {
                    dst,
                    op,
                    value: right,
                    constant_bits: value.to_bits(),
                    constant_left: true,
                })
            }
        }
        FloatAdd { dst, left, right } => immediate!(dst, left, right, FloatAddConst, FloatAddConst),
        FloatSubtract { dst, left, right } => {
            immediate!(dst, left, right, FloatSubtractConst, FloatSubtractFromConst)
        }
        FloatMultiply { dst, left, right } => {
            immediate!(dst, left, right, FloatMultiplyConst, FloatMultiplyConst)
        }
        FloatDivide { dst, left, right } => {
            immediate!(dst, left, right, FloatDivideConst, FloatDivideIntoConst)
        }
        FloatRemainder { dst, left, right } => immediate!(
            dst,
            left,
            right,
            FloatRemainderConst,
            FloatRemainderFromConst
        ),
        // Donder permits real-number algebra: identities and reciprocal
        // multiplication need not preserve signed zero or every rounding bit.
        FloatAddConst {
            dst,
            value,
            constant_bits,
        }
        | FloatSubtractConst {
            dst,
            value,
            constant_bits,
        } if f32::from_bits(constant_bits) == 0.0 => copy(dst, value.0),
        FloatMultiplyConst {
            dst,
            value,
            constant_bits,
        }
        | FloatDivideConst {
            dst,
            value,
            constant_bits,
        } if f32::from_bits(constant_bits) == 1.0 => copy(dst, value.0),
        FloatMultiplyConst {
            dst,
            value,
            constant_bits,
        } if f32::from_bits(constant_bits) == 0.0 && real(state, value) => Some(LoadFloatConst {
            dst,
            bits: 0.0f32.to_bits(),
        }),
        FloatDivideConst {
            dst,
            value,
            constant_bits,
        } => {
            let divisor = f32::from_bits(constant_bits);
            let reciprocal = divisor.recip();
            (divisor.is_finite() && divisor != 0.0 && reciprocal.is_finite()).then_some(
                FloatMultiplyConst {
                    dst,
                    value,
                    constant_bits: reciprocal.to_bits(),
                },
            )
        }
        _ => None,
    }
}

fn removable(op: &Instruction) -> bool {
    use Instruction::*;
    matches!(
        op,
        LoadIntConst { .. }
            | QuerySeconds { .. }
            | QueryProgress { .. }
            | LoadFloatConst { .. }
            | LoadBoolConst { .. }
            | LoadColorConst { .. }
            | LoadArrayConst { .. }
            | Move { .. }
            | MakeArray { .. }
            | IntToFloat { .. }
            | Not { .. }
            | NegInt { .. }
            | NegFloat { .. }
            | IntAdd { .. }
            | IntSubtract { .. }
            | IntMultiply { .. }
            | IntRemainder { .. }
            | FloatAdd { .. }
            | FloatSubtract { .. }
            | FloatMultiply { .. }
            | FloatMultiplyAdd { .. }
            | FloatMultiplyAddConst { .. }
            | FloatMultiplySmoothstep { .. }
            | FloatDivide { .. }
            | FloatRemainder { .. }
            | FloatAddConst { .. }
            | FloatSubtractConst { .. }
            | FloatMultiplyConst { .. }
            | FloatDivideConst { .. }
            | FloatRemainderConst { .. }
            | FloatSubtractFromConst { .. }
            | FloatDivideIntoConst { .. }
            | FloatRemainderFromConst { .. }
            | FloatCompare { .. }
            | FloatCompareConst { .. }
            | IntCompare { .. }
            | FloatUnary { .. }
            | FloatBinary { .. }
            | FloatBinaryConst { .. }
            | Clamp { .. }
            | ClampConst { .. }
            | Smoothstep { .. }
            | MixFloat { .. }
            | MixColor { .. }
            | ColorBinary { .. }
            | ColorScale { .. }
            | ColorComponent { .. }
            | ColorInvert { .. }
            | Rgb { .. }
            | Hsv { .. }
    ) || matches!(op, ValueEqual { left, right, .. } if matches!(left, ValueSlot::Int(_) | ValueSlot::Float(_) | ValueSlot::Bool(_) | ValueSlot::Color(_)) && matches!(right, ValueSlot::Int(_) | ValueSlot::Float(_) | ValueSlot::Bool(_) | ValueSlot::Color(_)))
}

pub(super) fn live_after(
    code: &[Instruction],
    operands: &mut [ValueSlot],
) -> Vec<HashSet<ValueSlot>> {
    let reads: Vec<_> = code
        .iter()
        .map(|op| {
            let mut reads = HashSet::new();
            slots(&mut op.clone(), operands, |slot, write| {
                if !write {
                    reads.insert(slot);
                }
                slot
            });
            reads
        })
        .collect();
    let mut before = vec![HashSet::new(); code.len()];
    let mut after = before.clone();
    loop {
        let mut changed = false;
        for ip in (0..code.len()).rev() {
            let mut live = HashSet::new();
            for next in successors(code, ip) {
                live.extend(before[next].iter().copied());
            }
            after[ip] = live.clone();
            let needed = code[ip].written_slot().is_some_and(|dst| live.remove(&dst));
            if needed || !removable(&code[ip]) {
                live.extend(reads[ip].iter().copied());
            }
            if before[ip] != live {
                before[ip] = live;
                changed = true;
            }
        }
        if !changed {
            return after;
        }
    }
}

fn compact(code: &mut Vec<Instruction>, keep: &[bool]) {
    let mut offsets = Vec::with_capacity(code.len() + 1);
    let mut kept = 0;
    for &keep in keep {
        offsets.push(kept);
        kept += usize::from(keep);
    }
    offsets.push(kept);
    let mut ip = 0;
    code.retain_mut(|op| {
        let retain = keep[ip];
        ip += 1;
        if retain && let Some(target) = op.jump_target_mut() {
            *target = offsets[*target];
        }
        retain
    });
}

pub(super) fn run(code: &mut Vec<Instruction>, operands: &mut [ValueSlot]) {
    loop {
        let previous = code.clone();
        let incoming = states(code);
        for (ip, op) in code.iter_mut().enumerate() {
            let Some(state) = &incoming[ip] else {
                continue;
            };
            slots(op, operands, |slot, write| {
                if write { slot } else { resolve(state, slot) }
            });
            if let (Some(taken), Some(target)) = (branch_value(op, state), op.conditional_target())
            {
                *op = Instruction::Jump(if taken { target } else { ip + 1 });
            } else if let Some(replacement) = simplify(op, state) {
                *op = replacement;
            }
        }
        let live = live_after(code, operands);
        let reachable = states(code);
        let mut keep: Vec<_> = code
            .iter()
            .enumerate()
            .map(|(ip, op)| {
                reachable[ip].is_some()
                    && (!removable(op)
                        || op.written_slot().is_some_and(|dst| live[ip].contains(&dst)))
                    && !matches!(op, Instruction::Move {dst, src} if dst.index() == *src)
                    && !matches!(op, Instruction::Jump(target) if *target == ip + 1)
            })
            .collect();
        // Structural loop admission still needs paired markers if an early
        // return makes the tail unreachable. Entire unreachable loops disappear.
        for (ip, op) in code.iter().enumerate() {
            if let Instruction::LoopRangeStart { end, .. } | Instruction::LoopMarksStart { end, .. } =
                op
                && (keep[ip] || keep[*end])
            {
                keep[ip] = true;
                keep[*end] = true;
            }
        }
        let targets: HashSet<_> = code.iter().filter_map(Instruction::jump_target).collect();
        for ip in 1..code.len() {
            if !keep[ip] || !keep[ip - 1] || targets.contains(&ip) {
                continue;
            }
            if let Instruction::Move { dst, src } = code[ip]
                && matches!(
                    dst,
                    ValueSlot::Int(_)
                        | ValueSlot::Float(_)
                        | ValueSlot::Bool(_)
                        | ValueSlot::Color(_)
                )
            {
                let source = dst.with_index(src);
                if code[ip - 1].written_slot() == Some(source) && !live[ip].contains(&source) {
                    slots(
                        &mut code[ip - 1],
                        operands,
                        |slot, write| if write { dst } else { slot },
                    );
                    keep[ip] = false;
                }
            } else if let Instruction::JumpIfFalse { condition, target }
            | Instruction::JumpIfTrue { condition, target } = code[ip]
                && code[ip - 1].written_slot() == Some(ValueSlot::Bool(condition))
                && !live[ip].contains(&ValueSlot::Bool(condition))
                && let Some(branch) = comparison_branch(
                    &code[ip - 1],
                    matches!(code[ip], Instruction::JumpIfTrue { .. }),
                    target,
                )
            {
                code[ip - 1] = branch;
                keep[ip] = false;
            }
        }
        compact(code, &keep);
        if *code == previous {
            break;
        }
    }
}

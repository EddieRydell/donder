//! Structured-loop optimization. All proofs and temporary maps are compiler-only.
//! A failed proof leaves the original instructions alone; playback has no guards,
//! analysis tables, or alternate execution paths for these optimizations.
use std::collections::{HashMap, HashSet, VecDeque};

use super::{dataflow, slots};
use crate::dsl::bytecode::{
    FloatBinary, FloatSlot, FloatUnary, Instruction, IntSlot, SlotLayout, ValueSlot,
};

type Definitions = HashMap<ValueSlot, usize>;

struct Flow {
    before: Vec<Option<Definitions>>,
    reads: Vec<HashSet<ValueSlot>>,
    live: Vec<HashSet<ValueSlot>>,
}

impl Flow {
    fn new(code: &[Instruction], operands: &mut [ValueSlot]) -> Self {
        let reads = code
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
        let mut before: Vec<Option<Definitions>> = vec![None; code.len()];
        let mut pending = VecDeque::new();
        let mut queued = vec![false; code.len()];
        if !code.is_empty() {
            before[0] = Some(Definitions::new());
            pending.push_back(0);
            queued[0] = true;
        }
        while let Some(ip) = pending.pop_front() {
            queued[ip] = false;
            let Some(mut outgoing) = before[ip].clone() else {
                unreachable!("the definition worklist contains reachable instructions")
            };
            if let Some(dst) = code[ip].written_slot() {
                outgoing.insert(dst, ip);
            }
            for next in dataflow::successors(code, ip) {
                let changed = if let Some(incoming) = &mut before[next] {
                    let previous = incoming.len();
                    incoming.retain(|slot, def| outgoing.get(slot) == Some(def));
                    previous != incoming.len()
                } else {
                    before[next] = Some(outgoing.clone());
                    true
                };
                if changed && !queued[next] {
                    queued[next] = true;
                    pending.push_back(next);
                }
            }
        }
        Self {
            before,
            reads,
            live: dataflow::live_after(code, operands),
        }
    }

    fn definition(&self, slot: ValueSlot, ip: usize) -> Option<usize> {
        self.before.get(ip)?.as_ref()?.get(&slot).copied()
    }

    fn integer(&self, code: &[Instruction], slot: IntSlot, ip: usize) -> Option<i32> {
        match code[self.definition(ValueSlot::Int(slot), ip)?] {
            Instruction::LoadIntConst { value, .. } => Some(value),
            _ => None,
        }
    }

    fn live_before(&self, code: &[Instruction], ip: usize) -> HashSet<ValueSlot> {
        let Some(op) = code.get(ip) else {
            return HashSet::new();
        };
        let mut live = self.live[ip].clone();
        if let Some(dst) = op.written_slot() {
            live.remove(&dst);
        }
        live.extend(self.reads[ip].iter().copied());
        live
    }
}

#[derive(Clone, Copy)]
struct Loop {
    header: usize,
    end: usize,
}

fn loops(code: &[Instruction]) -> Vec<Loop> {
    let mut result: Vec<_> =
        code.iter()
            .enumerate()
            .filter_map(|(header, op)| match *op {
                Instruction::LoopRangeStart { end, .. }
                | Instruction::LoopMarksStart { end, .. } => Some(Loop { header, end }),
                _ => None,
            })
            .collect();
    // Inner loops first. Hoisted inner expressions may subsequently leave their
    // enclosing loop too, but never skip its entry or initialization.
    result.sort_by_key(|region| region.end - region.header);
    result
}

/// Only total operations with scalar/color results, including admitted typed
/// parameter samples. Signal sampling and reference operations stay ordered.
pub(super) fn pure(op: &Instruction) -> bool {
    use Instruction::*;
    matches!(
        op,
        LoadIntConst { .. }
            | LoadFloatConst { .. }
            | LoadBoolConst { .. }
            | LoadColorConst { .. }
            | LoadIntParam { .. }
            | LoadFloatParam { .. }
            | LoadBoolParam { .. }
            | LoadColorParam { .. }
            | ContextRead { .. }
            | QuerySeconds { .. }
            | QueryProgress { .. }
            | IntToFloat { .. }
            | NegInt { .. }
            | NegFloat { .. }
            | Not { .. }
            | IntAdd { .. }
            | IntSubtract { .. }
            | IntMultiply { .. }
            | IntRemainder { .. }
            | FloatAdd { .. }
            | FloatSubtract { .. }
            | FloatMultiply { .. }
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
            | FloatUnary { .. }
            | FloatBinary { .. }
            | FloatBinaryConst { .. }
            | FloatCompare { .. }
            | FloatCompareConst { .. }
            | IntCompare { .. }
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
    ) || super::parameter_sample(op)
        || matches!(
            op,
            Move {
                dst: ValueSlot::Int(_)
                    | ValueSlot::Float(_)
                    | ValueSlot::Bool(_)
                    | ValueSlot::Color(_),
                ..
            }
        )
}

/// Non-NaN range. NaN remains NaN through reciprocal multiplication; ranges
/// here never authorize dropping missing values. Arithmetic follows the DSL's
/// real-number policy, while avoiding reciprocal overflow for tiny divisors.
#[derive(Clone, Copy)]
struct Range {
    low: f32,
    high: f32,
}

impl Range {
    const UNKNOWN: Self = Self {
        low: f32::NEG_INFINITY,
        high: f32::INFINITY,
    };
    fn new(low: f32, high: f32) -> Self {
        if low.is_nan() || high.is_nan() || low > high {
            Self::UNKNOWN
        } else {
            Self { low, high }
        }
    }
    fn constant(value: f32) -> Self {
        Self::new(value, value)
    }
    fn add(self, other: Self) -> Self {
        Self::new(self.low + other.low, self.high + other.high)
    }
    fn neg(self) -> Self {
        Self::new(-self.high, -self.low)
    }
    fn sub(self, other: Self) -> Self {
        self.add(other.neg())
    }
    fn mul(self, other: Self) -> Self {
        let values = [
            self.low * other.low,
            self.low * other.high,
            self.high * other.low,
            self.high * other.high,
        ];
        if values.iter().any(|v| v.is_nan()) {
            return Self::UNKNOWN;
        }
        Self::new(
            values.into_iter().fold(f32::INFINITY, f32::min),
            values.into_iter().fold(f32::NEG_INFINITY, f32::max),
        )
    }
    fn div(self, other: Self) -> Self {
        if other.low <= 0.0 && other.high >= 0.0 {
            return Self::UNKNOWN;
        }
        let values = [
            self.low / other.low,
            self.low / other.high,
            self.high / other.low,
            self.high / other.high,
        ];
        if values.iter().any(|v| v.is_nan()) {
            return Self::UNKNOWN;
        }
        Self::new(
            values.into_iter().fold(f32::INFINITY, f32::min),
            values.into_iter().fold(f32::NEG_INFINITY, f32::max),
        )
    }
    fn binary(self, op: FloatBinary, other: Self) -> Self {
        match op {
            FloatBinary::Min => Self::new(self.low.min(other.low), self.high.min(other.high)),
            FloatBinary::Max => Self::new(self.low.max(other.low), self.high.max(other.high)),
            FloatBinary::ValueOr => Self::new(self.low.min(other.low), self.high.max(other.high)),
            FloatBinary::Atan2 => Self::UNKNOWN,
        }
    }
    fn unary(self, op: FloatUnary) -> Self {
        match op {
            FloatUnary::Abs => Self::new(
                if self.low <= 0.0 && self.high >= 0.0 {
                    0.0
                } else {
                    self.low.abs().min(self.high.abs())
                },
                self.low.abs().max(self.high.abs()),
            ),
            FloatUnary::Floor => Self::new(self.low.floor(), self.high.floor()),
            FloatUnary::Sqrt if self.low >= 0.0 => {
                Self::new(libm::sqrtf(self.low), libm::sqrtf(self.high))
            }
            FloatUnary::Sqrt => Self::UNKNOWN,
            FloatUnary::Sin | FloatUnary::Cos => Self::new(-1.0, 1.0),
        }
    }
    fn reciprocal_safe(self) -> bool {
        self.low >= f32::MIN_POSITIVE || self.high <= -f32::MIN_POSITIVE
    }
}

fn range_of(op: &Instruction, mut input: impl FnMut(ValueSlot) -> Range) -> Range {
    use Instruction::*;
    match *op {
        LoadFloatConst { bits, .. } => Range::constant(f32::from_bits(bits)),
        LoadIntConst { value, .. } => Range::constant(value as f32),
        Move { dst, src } => input(dst.with_index(src)),
        IntToFloat { src, .. } => input(ValueSlot::Int(src)),
        NegFloat { src, .. } => input(ValueSlot::Float(src)).neg(),
        FloatAdd { left, right, .. } => {
            input(ValueSlot::Float(left)).add(input(ValueSlot::Float(right)))
        }
        FloatSubtract { left, right, .. } => {
            input(ValueSlot::Float(left)).sub(input(ValueSlot::Float(right)))
        }
        FloatMultiply { left, right, .. } => {
            input(ValueSlot::Float(left)).mul(input(ValueSlot::Float(right)))
        }
        FloatDivide { left, right, .. } => {
            input(ValueSlot::Float(left)).div(input(ValueSlot::Float(right)))
        }
        FloatAddConst {
            value,
            constant_bits,
            ..
        } => input(ValueSlot::Float(value)).add(Range::constant(f32::from_bits(constant_bits))),
        FloatSubtractConst {
            value,
            constant_bits,
            ..
        } => input(ValueSlot::Float(value)).sub(Range::constant(f32::from_bits(constant_bits))),
        FloatSubtractFromConst {
            value,
            constant_bits,
            ..
        } => Range::constant(f32::from_bits(constant_bits)).sub(input(ValueSlot::Float(value))),
        FloatMultiplyConst {
            value,
            constant_bits,
            ..
        } => input(ValueSlot::Float(value)).mul(Range::constant(f32::from_bits(constant_bits))),
        FloatDivideConst {
            value,
            constant_bits,
            ..
        } => input(ValueSlot::Float(value)).div(Range::constant(f32::from_bits(constant_bits))),
        FloatDivideIntoConst {
            value,
            constant_bits,
            ..
        } => Range::constant(f32::from_bits(constant_bits)).div(input(ValueSlot::Float(value))),
        FloatUnary { op, value, .. } => input(ValueSlot::Float(value)).unary(op),
        FloatBinary {
            op, left, right, ..
        } => input(ValueSlot::Float(left)).binary(op, input(ValueSlot::Float(right))),
        FloatBinaryConst {
            op,
            value,
            constant_bits,
            ..
        } => input(ValueSlot::Float(value))
            .binary(op, Range::constant(f32::from_bits(constant_bits))),
        ClampConst {
            min_bits, max_bits, ..
        } => Range::new(f32::from_bits(min_bits), f32::from_bits(max_bits)),
        _ => Range::UNKNOWN,
    }
}

struct Ranges<'a> {
    code: &'a [Instruction],
    flow: &'a Flow,
    values: HashMap<usize, Range>,
    visiting: HashSet<usize>,
}

impl<'a> Ranges<'a> {
    fn new(code: &'a [Instruction], flow: &'a Flow) -> Self {
        Self {
            code,
            flow,
            values: HashMap::new(),
            visiting: HashSet::new(),
        }
    }
    fn get(&mut self, slot: ValueSlot, ip: usize) -> Range {
        let Some(def) = self.flow.definition(slot, ip) else {
            return Range::UNKNOWN;
        };
        if let Some(&range) = self.values.get(&def) {
            return range;
        }
        if !self.visiting.insert(def) {
            return Range::UNKNOWN;
        }
        let range = range_of(&self.code[def].clone(), |source| self.get(source, def));
        self.visiting.remove(&def);
        self.values.insert(def, range);
        range
    }
}

/// Insertions execute when control reaches their label. Structural loop end
/// pointers name the actual LoopEnd, not any instructions inserted before it.
fn rewrite(
    code: &mut Vec<Instruction>,
    insert: &HashMap<usize, Vec<Instruction>>,
    remove: &HashSet<usize>,
) {
    let mut labels = vec![0; code.len() + 1];
    let mut positions = vec![0; code.len()];
    let mut output = Vec::new();
    let mut originals = Vec::new();
    for (ip, op) in code.iter().enumerate() {
        labels[ip] = output.len();
        if let Some(added) = insert.get(&ip) {
            output.extend_from_slice(added);
        }
        positions[ip] = output.len();
        if !remove.contains(&ip) {
            originals.push(output.len());
            output.push(op.clone());
        }
    }
    labels[code.len()] = output.len();
    for ip in originals {
        match &mut output[ip] {
            Instruction::LoopRangeStart { end, .. } | Instruction::LoopMarksStart { end, .. } => {
                *end = positions[*end]
            }
            op => {
                if let Some(target) = op.jump_target_mut() {
                    *target = labels[*target];
                }
            }
        }
    }
    *code = output;
}

fn hoist(code: &mut Vec<Instruction>, operands: &mut [ValueSlot]) {
    loop {
        let flow = Flow::new(code, operands);
        let mut changed = false;
        for region in loops(code) {
            let live_out = flow.live_before(code, region.end + 1);
            let Some(entry) = flow.before[region.header].as_ref() else {
                continue;
            };
            let mut writes = HashMap::<ValueSlot, usize>::new();
            for op in &code[region.header + 1..region.end] {
                if let Some(dst) = op.written_slot() {
                    *writes.entry(dst).or_default() += 1;
                }
            }
            let mut invariant: HashSet<_> = entry
                .keys()
                .filter(|slot| !writes.contains_key(slot))
                .copied()
                .collect();
            let mut moved = HashSet::new();
            let mut prefix = Vec::new();
            for (ip, op) in code
                .iter()
                .enumerate()
                .take(region.end)
                .skip(region.header + 1)
            {
                let Some(dst) = op.written_slot() else {
                    continue;
                };
                if !pure(op)
                    || writes[&dst] != 1
                    || live_out.contains(&dst)
                    || !flow.reads[ip].iter().all(|slot| invariant.contains(slot))
                {
                    continue;
                }
                // A conditional write cannot replace an earlier value observed
                // elsewhere in the loop. Every surviving read must use this def.
                if (region.header + 1..=region.end).any(|use_ip| {
                    flow.reads[use_ip].contains(&dst) && flow.definition(dst, use_ip) != Some(ip)
                }) {
                    continue;
                }
                invariant.insert(dst);
                moved.insert(ip);
                prefix.push(op.clone());
            }
            if !prefix.is_empty() {
                rewrite(code, &HashMap::from([(region.header, prefix)]), &moved);
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
}

pub(super) fn reciprocals(
    code: &mut Vec<Instruction>,
    operands: &mut [ValueSlot],
    layout: &mut SlotLayout,
    pixel_entry: usize,
) {
    let flow = Flow::new(code, operands);
    let regions = loops(code);
    let mut ranges = Ranges::new(code, &flow);
    let mut uses = HashMap::<(FloatSlot, usize), Vec<usize>>::new();
    for (ip, op) in code.iter().enumerate() {
        let Instruction::FloatDivide { right, .. } = *op else {
            continue;
        };
        let Some(def) = flow.definition(ValueSlot::Float(right), ip) else {
            continue;
        };
        if ranges.get(ValueSlot::Float(right), ip).reciprocal_safe() {
            uses.entry((right, def)).or_default().push(ip);
        }
    }
    let mut groups: Vec<_> = uses.into_iter().collect();
    groups.sort_by_key(|((slot, def), _)| (*def, slot.0));
    let mut insert = HashMap::<usize, Vec<Instruction>>::new();
    for ((denominator, def), uses) in groups {
        // Amortize either across distinct uses of one definition or across a
        // loop that cannot change it. Do not add a reciprocal for one ordinary
        // division on each iteration of a changing denominator.
        let repeated = uses.len() > 1
            || def < pixel_entry && uses.iter().any(|&ip| ip >= pixel_entry)
            || uses.iter().any(|&ip| {
                regions
                    .iter()
                    .any(|r| def < r.header && r.header < ip && ip < r.end)
            });
        if !repeated {
            continue;
        }
        let Some(next) = layout.floats.checked_add(1) else {
            continue;
        };
        let inverse = FloatSlot(layout.floats);
        layout.floats = next;
        insert
            .entry(def + 1)
            .or_default()
            .push(Instruction::FloatDivideIntoConst {
                dst: inverse,
                value: denominator,
                constant_bits: 1.0f32.to_bits(),
            });
        for ip in uses {
            let Instruction::FloatDivide { dst, left, .. } = code[ip] else {
                unreachable!("recorded division")
            };
            code[ip] = Instruction::FloatMultiply {
                dst,
                left,
                right: inverse,
            };
        }
    }
    if !insert.is_empty() {
        rewrite(code, &insert, &HashSet::new());
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Trend {
    Constant,
    Increasing,
    Decreasing,
    Unknown,
}

impl Trend {
    fn neg(self) -> Self {
        match self {
            Self::Increasing => Self::Decreasing,
            Self::Decreasing => Self::Increasing,
            other => other,
        }
    }
    fn add(self, other: Self) -> Self {
        match (self, other) {
            (Self::Constant, value) | (value, Self::Constant) => value,
            (Self::Increasing, Self::Increasing) => Self::Increasing,
            (Self::Decreasing, Self::Decreasing) => Self::Decreasing,
            _ => Self::Unknown,
        }
    }
    fn scale(self, range: Range) -> Self {
        if range.low > 0.0 {
            self
        } else if range.high < 0.0 {
            self.neg()
        } else {
            Self::Unknown
        }
    }
}

#[derive(Clone, Copy)]
struct Fact {
    trend: Trend,
    range: Range,
}

fn trend_of(op: &Instruction, mut fact: impl FnMut(ValueSlot) -> Fact) -> Trend {
    use crate::dsl::bytecode::{FloatBinary as Binary, FloatUnary as Unary};
    use Instruction::*;
    let multiply = |a: Fact, b: Fact| {
        if a.trend == Trend::Constant {
            b.trend.scale(a.range)
        } else if b.trend == Trend::Constant {
            a.trend.scale(b.range)
        } else {
            Trend::Unknown
        }
    };
    match *op {
        LoadFloatConst { .. }
        | LoadIntConst { .. }
        | LoadFloatParam { .. }
        | LoadIntParam { .. }
        | ContextRead { .. } => Trend::Constant,
        Move { dst, src } => fact(dst.with_index(src)).trend,
        IntToFloat { src, .. } => fact(ValueSlot::Int(src)).trend,
        NegFloat { src, .. } => fact(ValueSlot::Float(src)).trend.neg(),
        FloatAdd { left, right, .. } => fact(ValueSlot::Float(left))
            .trend
            .add(fact(ValueSlot::Float(right)).trend),
        FloatSubtract { left, right, .. } => fact(ValueSlot::Float(left))
            .trend
            .add(fact(ValueSlot::Float(right)).trend.neg()),
        FloatMultiply { left, right, .. } => {
            multiply(fact(ValueSlot::Float(left)), fact(ValueSlot::Float(right)))
        }
        FloatDivide { left, right, .. } => {
            let denominator = fact(ValueSlot::Float(right));
            if denominator.trend == Trend::Constant {
                fact(ValueSlot::Float(left)).trend.scale(denominator.range)
            } else {
                Trend::Unknown
            }
        }
        FloatAddConst { value, .. } | FloatSubtractConst { value, .. } => {
            fact(ValueSlot::Float(value)).trend
        }
        FloatSubtractFromConst { value, .. } => fact(ValueSlot::Float(value)).trend.neg(),
        FloatMultiplyConst {
            value,
            constant_bits,
            ..
        }
        | FloatDivideConst {
            value,
            constant_bits,
            ..
        } => fact(ValueSlot::Float(value))
            .trend
            .scale(Range::constant(f32::from_bits(constant_bits))),
        FloatUnary { op, value, .. } => {
            let value = fact(ValueSlot::Float(value));
            match op {
                Unary::Floor => value.trend,
                Unary::Abs if value.range.low >= 0.0 => value.trend,
                Unary::Abs if value.range.high <= 0.0 => value.trend.neg(),
                _ => Trend::Unknown,
            }
        }
        FloatBinary {
            op: Binary::Min | Binary::Max,
            left,
            right,
            ..
        } => fact(ValueSlot::Float(left))
            .trend
            .add(fact(ValueSlot::Float(right)).trend),
        FloatBinaryConst {
            op: Binary::Min | Binary::Max,
            value,
            ..
        }
        | ClampConst { value, .. } => fact(ValueSlot::Float(value)).trend,
        _ => Trend::Unknown,
    }
}

fn permanent_branch(op: &Instruction, mut fact: impl FnMut(ValueSlot) -> Fact) -> bool {
    use Instruction::*;
    let stable = |trend, less: bool, when: bool| match trend {
        Trend::Constant => true,
        Trend::Increasing => less != when,
        Trend::Decreasing => less == when,
        Trend::Unknown => false,
    };
    match *op {
        FloatJumpLess {
            left, right, when, ..
        }
        | FloatJumpLessEqual {
            left, right, when, ..
        } => stable(
            fact(ValueSlot::Float(left))
                .trend
                .add(fact(ValueSlot::Float(right)).trend.neg()),
            true,
            when,
        ),
        FloatJumpGreater {
            left, right, when, ..
        }
        | FloatJumpGreaterEqual {
            left, right, when, ..
        } => stable(
            fact(ValueSlot::Float(left))
                .trend
                .add(fact(ValueSlot::Float(right)).trend.neg()),
            false,
            when,
        ),
        FloatJumpLessConst { value, when, .. } | FloatJumpLessEqualConst { value, when, .. } => {
            stable(fact(ValueSlot::Float(value)).trend, true, when)
        }
        FloatJumpGreaterConst { value, when, .. }
        | FloatJumpGreaterEqualConst { value, when, .. } => {
            stable(fact(ValueSlot::Float(value)).trend, false, when)
        }
        IntJumpLess {
            left, right, when, ..
        }
        | IntJumpLessEqual {
            left, right, when, ..
        } => stable(
            fact(ValueSlot::Int(left))
                .trend
                .add(fact(ValueSlot::Int(right)).trend.neg()),
            true,
            when,
        ),
        IntJumpGreater {
            left, right, when, ..
        }
        | IntJumpGreaterEqual {
            left, right, when, ..
        } => stable(
            fact(ValueSlot::Int(left))
                .trend
                .add(fact(ValueSlot::Int(right)).trend.neg()),
            false,
            when,
        ),
        FloatJumpEqual { left, right, .. } => {
            fact(ValueSlot::Float(left)).trend == Trend::Constant
                && fact(ValueSlot::Float(right)).trend == Trend::Constant
        }
        FloatJumpEqualConst { value, .. } => fact(ValueSlot::Float(value)).trend == Trend::Constant,
        IntJumpEqual { left, right, .. } => {
            fact(ValueSlot::Int(left)).trend == Trend::Constant
                && fact(ValueSlot::Int(right)).trend == Trend::Constant
        }
        _ => false,
    }
}

fn shorten(code: &mut [Instruction], operands: &mut [ValueSlot]) {
    enum Step {
        Add(i64),
        Multiply(i64),
    }
    let flow = Flow::new(code, operands);
    let mut changes = Vec::new();
    for region in loops(code) {
        let live_out = flow.live_before(code, region.end + 1);
        let Instruction::LoopRangeStart { cap, count, .. } = code[region.header] else {
            continue;
        };
        let cap = flow
            .integer(code, count, region.header)
            .map_or(cap, |count| count.min(cap))
            .max(0);
        if region.end <= region.header + 1 {
            continue;
        }
        let update = region.end - 1;
        let (counter, step) = match code[update] {
            Instruction::IntAdd { dst, left, right } if dst == left => (
                dst,
                flow.integer(code, right, update)
                    .map(|v| Step::Add(i64::from(v))),
            ),
            Instruction::IntAdd { dst, left, right } if dst == right => (
                dst,
                flow.integer(code, left, update)
                    .map(|v| Step::Add(i64::from(v))),
            ),
            Instruction::IntSubtract { dst, left, right } if dst == left => (
                dst,
                flow.integer(code, right, update)
                    .map(|v| Step::Add(-i64::from(v))),
            ),
            Instruction::IntMultiply { dst, left, right } if dst == left => (
                dst,
                flow.integer(code, right, update)
                    .map(|v| Step::Multiply(i64::from(v))),
            ),
            Instruction::IntMultiply { dst, left, right } if dst == right => (
                dst,
                flow.integer(code, left, update)
                    .map(|v| Step::Multiply(i64::from(v))),
            ),
            _ => continue,
        };
        let (Some(step), Some(initial)) = (step, flow.integer(code, counter, region.header)) else {
            continue;
        };
        let initial = i64::from(initial);
        // A nonnegative factor is monotone for either sign of the initial
        // value. Prove the final update too: wrapping would invalidate both
        // the range and the permanent-rejection argument.
        let final_value = match step {
            Step::Add(step) => Some(initial + step * i64::from(cap.max(0))),
            Step::Multiply(factor) if factor >= 0 => factor
                .checked_pow(cap.max(0) as u32)
                .and_then(|power| initial.checked_mul(power)),
            Step::Multiply(_) => None,
        };
        let Some(final_value) = final_value else {
            continue;
        };
        if final_value < i64::from(i32::MIN) || final_value > i64::from(i32::MAX) {
            continue;
        }
        let counter = ValueSlot::Int(counter);
        if code[region.header + 1..update]
            .iter()
            .any(|op| op.written_slot() == Some(counter))
        {
            continue;
        }
        let writes: HashSet<_> = code[region.header + 1..=region.end]
            .iter()
            .filter_map(Instruction::written_slot)
            .collect();
        let mut ranges = Ranges::new(code, &flow);
        let mut facts = HashMap::from([(
            counter,
            Fact {
                trend: if final_value > initial {
                    Trend::Increasing
                } else if final_value < initial {
                    Trend::Decreasing
                } else {
                    Trend::Constant
                },
                range: Range::new(
                    initial.min(final_value) as f32,
                    initial.max(final_value) as f32,
                ),
            },
        )]);
        // The prefix may contain only total scalar work and guards that reject
        // into a side-effect-free tail. No sampled work may be skipped.
        for ip in region.header + 1..update {
            let lookup = |slot, facts: &HashMap<ValueSlot, Fact>, ranges: &mut Ranges<'_>| {
                facts.get(&slot).copied().unwrap_or_else(|| Fact {
                    trend: if !writes.contains(&slot)
                        && flow.definition(slot, region.header).is_some()
                    {
                        Trend::Constant
                    } else {
                        Trend::Unknown
                    },
                    range: ranges.get(slot, region.header),
                })
            };
            if let Some(target) = code[ip].conditional_target() {
                let safe_tail = target > ip
                    && target <= update
                    && code[target..region.end].iter().all(|op| {
                        pure(op)
                            && op
                                .written_slot()
                                .is_some_and(|dst| !live_out.contains(&dst))
                    });
                if !safe_tail {
                    break;
                }
                if permanent_branch(&code[ip], |slot| lookup(slot, &facts, &mut ranges)) {
                    changes.push((ip, region.end + 1));
                }
            } else {
                let Some(dst) = code[ip].written_slot() else {
                    break;
                };
                if !pure(&code[ip]) || live_out.contains(&dst) {
                    break;
                }
                let trend = trend_of(&code[ip], |slot| lookup(slot, &facts, &mut ranges));
                let range = range_of(&code[ip], |slot| lookup(slot, &facts, &mut ranges).range);
                facts.insert(dst, Fact { trend, range });
            }
        }
    }
    for (ip, destination) in changes {
        let Some(target) = code[ip].jump_target_mut() else {
            unreachable!("recorded conditional branch")
        };
        *target = destination;
    }
}

pub(super) fn run(
    code: &mut Vec<Instruction>,
    operands: &mut [ValueSlot],
    layout: &mut SlotLayout,
) {
    hoist(code, operands);
    shorten(code, operands);
    reciprocals(code, operands, layout, 0);
    dataflow::run(code, operands);
}

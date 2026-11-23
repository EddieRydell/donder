//! Value ranges of numeric nodes, from literals, declared parameter ranges and
//! array lengths. They prove reduction bounds and nonzero divisors, and fold
//! operations whose result they decide.
//!
//! One rule table serves two drivers: every graph node records its interval
//! from its operands' as it is built, with the parameter ranges its graph
//! declares, and [`interval`] recomputes a node's with ranges and lengths an
//! instance supplies.
use super::{Binary, Context, Graph, Node, Op, Ternary, Unary};
use donder_runtime_types::Value;
use std::collections::HashMap;

/// An inclusive range of a numeric value, and whether it may be NaN.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Interval {
    pub(crate) min: f64,
    pub(crate) max: f64,
    nan: bool,
}

impl Interval {
    pub(crate) const ANY: Self = Self {
        min: f64::NEG_INFINITY,
        max: f64::INFINITY,
        nan: true,
    };

    fn exact(value: f64) -> Self {
        Self {
            min: value,
            max: value,
            nan: value.is_nan(),
        }
        .normalized()
    }

    pub(crate) fn new(min: f64, max: f64, nan: bool) -> Self {
        Self { min, max, nan }.normalized()
    }

    /// A bound beyond the 32-bit float range is infinite: playback's
    /// arithmetic overflows there.
    fn normalized(self) -> Self {
        if self.min.is_nan() || self.max.is_nan() {
            return Self::ANY;
        }
        let limit = f64::from(f32::MAX);
        Self {
            min: if self.min < -limit {
                f64::NEG_INFINITY
            } else {
                self.min
            },
            max: if self.max > limit {
                f64::INFINITY
            } else {
                self.max
            },
            nan: self.nan,
        }
    }

    fn union(self, other: Self) -> Self {
        Self::new(
            self.min.min(other.min),
            self.max.max(other.max),
            self.nan || other.nan,
        )
    }

    fn excludes_zero(self) -> bool {
        self.min > 0.0 || self.max < 0.0
    }

    /// Never NaN.
    pub(crate) fn is_number(self) -> bool {
        !self.nan
    }

    /// A finite number, never NaN.
    pub(crate) fn is_finite(self) -> bool {
        !self.nan && self.min.is_finite() && self.max.is_finite()
    }

    /// The one finite value the range holds, if it holds one.
    pub(crate) fn single(self) -> Option<f64> {
        (!self.nan && self.min == self.max && self.min.is_finite()).then_some(self.min)
    }

    /// Wider by more than 32-bit rounding over a few operations.
    pub(crate) fn widened(self) -> Self {
        let margin = |bound: f64| 1e-5 * bound.abs().max(1.0);
        Self::new(
            self.min - margin(self.min),
            self.max + margin(self.max),
            self.nan,
        )
    }

    fn corners(self, other: Self, op: impl Fn(f64, f64) -> f64) -> Self {
        let values = [
            op(self.min, other.min),
            op(self.min, other.max),
            op(self.max, other.min),
            op(self.max, other.max),
        ];
        if values.iter().any(|value| value.is_nan()) {
            return Self::ANY;
        }
        Self::new(
            values.iter().copied().fold(f64::INFINITY, f64::min),
            values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            self.nan || other.nan,
        )
    }

    fn monotonic(self, op: impl Fn(f64) -> f64) -> Self {
        Self::new(op(self.min), op(self.max), self.nan)
    }

    /// An int result: ints wrap, so a range past the int range may hold any int.
    fn wrapped(self) -> Self {
        let (min, max) = (f64::from(i32::MIN), f64::from(i32::MAX));
        if self.min < min || self.max > max {
            Self::new(min, max, false)
        } else {
            self
        }
    }
}

/// What is known about parameters: declared numeric ranges, and the lengths of
/// arrays and marks once an instance supplies them.
pub(crate) struct Bounds<'a> {
    pub(crate) ranges: &'a [Option<(f64, f64)>],
    pub(crate) lengths: &'a [Option<usize>],
}

/// `node`'s interval with `bounds` in place of its graph's parameter ranges.
pub(crate) fn interval(graph: &Graph, node: Node, bounds: &Bounds<'_>) -> Interval {
    Analysis {
        graph,
        bounds,
        memo: HashMap::new(),
    }
    .of(node)
}

/// The interval of a node about to join `graph` as `op`, from its operands'
/// recorded intervals and the graph's declared parameter ranges.
pub(crate) fn recorded(graph: &Graph, op: &Op) -> Interval {
    compute(graph, op, &mut Recorded(graph))
}

/// Intervals of a node's operands, and what is known about its leaves.
trait Known {
    fn of(&mut self, node: Node) -> Interval;
    fn param(&self, index: u32) -> Interval;
    /// The length of an array or marks parameter, when known.
    fn param_length(&self, index: u32) -> Option<usize>;
}

struct Recorded<'a>(&'a Graph);

impl Known for Recorded<'_> {
    fn of(&mut self, node: Node) -> Interval {
        self.0.interval(node)
    }

    fn param(&self, index: u32) -> Interval {
        match self.0.params()[index as usize].range {
            Some((min, max)) => Interval::new(min, max, false),
            None => Interval::ANY,
        }
    }

    fn param_length(&self, _: u32) -> Option<usize> {
        None
    }
}

struct Analysis<'a> {
    graph: &'a Graph,
    bounds: &'a Bounds<'a>,
    memo: HashMap<Node, Interval>,
}

impl Known for Analysis<'_> {
    fn of(&mut self, node: Node) -> Interval {
        if let Some(&interval) = self.memo.get(&node) {
            return interval;
        }
        let graph = self.graph;
        let interval = compute(graph, graph.op(node), self);
        self.memo.insert(node, interval);
        interval
    }

    fn param(&self, index: u32) -> Interval {
        match self.bounds.ranges.get(index as usize) {
            Some(Some((min, max))) => Interval::new(*min, *max, false),
            _ => Interval::ANY,
        }
    }

    fn param_length(&self, index: u32) -> Option<usize> {
        self.bounds.lengths.get(index as usize).copied().flatten()
    }
}

fn compute(graph: &Graph, op: &Op, known: &mut impl Known) -> Interval {
    match op {
        Op::Constant(constant) => match constant.value {
            Value::Int(value) => Interval::exact(f64::from(value)),
            Value::Float(value) => Interval::exact(f64::from(value)),
            _ => Interval::ANY,
        },
        Op::Param(index) => known.param(*index),
        Op::LoopIndex(id) => {
            let data = graph.loop_(*id);
            let (start, end) = (known.of(data.start), known.of(data.end));
            Interval::new(start.min, end.max - 1.0, false)
        }
        Op::Context(Context::PixelIndex) => Interval::new(0.0, f64::INFINITY, false),
        Op::ParamIntegral(_)
        | Op::Context(_)
        | Op::Read { .. }
        | Op::Reduce(_)
        | Op::Items(_)
        | Op::Neighbor
        | Op::Tap { .. }
        | Op::Scan { .. } => Interval::ANY,
        Op::Pick { items, .. } => items
            .iter()
            .map(|&item| known.of(item))
            .reduce(Interval::union)
            .unwrap_or(Interval::ANY),
        Op::Unary(op, a) => unary(graph, *op, *a, known),
        Op::Binary(op, a, b) => binary(graph, *op, *a, *b, known),
        Op::Ternary(Ternary::Clamp, value, min, max) => {
            let (value, min, max) = (known.of(*value), known.of(*min), known.of(*max));
            // An inverted or NaN bound gives NaN.
            let nan = value.nan || min.nan || max.nan || min.max > max.min;
            Interval::new(
                value.min.max(min.min).min(max.max),
                value.max.min(max.max).max(min.min),
                nan,
            )
        }
        Op::Ternary(..) => Interval::ANY,
        Op::Select(_, yes, no) => known.of(*yes).union(known.of(*no)),
    }
}

fn unary(graph: &Graph, op: Unary, a: Node, known: &mut impl Known) -> Interval {
    let value = known.of(a);
    match op {
        Unary::Negate => Interval::new(-value.max, -value.min, value.nan),
        Unary::IntNegate => Interval::new(-value.max, -value.min, false).wrapped(),
        Unary::Abs | Unary::IntAbs => {
            let low = if value.min <= 0.0 && value.max >= 0.0 {
                0.0
            } else {
                value.min.abs().min(value.max.abs())
            };
            let abs = Interval::new(low, value.min.abs().max(value.max.abs()), value.nan);
            if op == Unary::IntAbs {
                abs.wrapped()
            } else {
                abs
            }
        }
        Unary::Floor => value.monotonic(f64::floor),
        Unary::Ceil => value.monotonic(f64::ceil),
        Unary::Trunc => value.monotonic(f64::trunc),
        Unary::Sqrt => Interval::new(
            value.min.max(0.0).sqrt(),
            value.max.max(0.0).sqrt(),
            value.nan || value.min < 0.0,
        ),
        Unary::IntToFloat => value,
        // Truncation saturates at the int range, and NaN becomes zero.
        Unary::FloatToInt => {
            let min = value.min.trunc().max(f64::from(i32::MIN));
            let max = value.max.trunc().min(f64::from(i32::MAX));
            let converted = Interval::new(min, max, false);
            if value.nan {
                converted.union(Interval::exact(0.0))
            } else {
                converted
            }
        }
        Unary::Len | Unary::MarkCount => match length(graph, a, known) {
            Some(length) => Interval::exact(length as f64),
            None => Interval::new(0.0, f64::INFINITY, false),
        },
        Unary::Smoothstep | Unary::Rand | Unary::Saturation | Unary::Intensity => {
            Interval::new(0.0, 1.0, value.nan || op == Unary::Smoothstep)
        }
        Unary::Hue | Unary::Red | Unary::Green | Unary::Blue => Interval::new(0.0, 1.0, false),
        Unary::Sin | Unary::Cos => Interval::new(-1.0, 1.0, true),
        _ => Interval::ANY,
    }
}

fn binary(graph: &Graph, op: Binary, a: Node, b: Node, known: &mut impl Known) -> Interval {
    let (left, right) = (known.of(a), known.of(b));
    match op {
        Binary::Add => left.corners(right, |a, b| a + b),
        Binary::Subtract => left.corners(right, |a, b| a - b),
        Binary::Multiply => left.corners(right, |a, b| a * b),
        Binary::IntAdd => left.corners(right, |a, b| a + b).wrapped(),
        Binary::IntSubtract => left.corners(right, |a, b| a - b).wrapped(),
        Binary::IntMultiply => left.corners(right, |a, b| a * b).wrapped(),
        Binary::Divide if right.excludes_zero() => left.corners(right, |a, b| a / b),
        Binary::Min | Binary::IntMin => Interval::new(
            left.min.min(right.min),
            left.max.min(right.max),
            left.nan || right.nan,
        ),
        Binary::Max | Binary::IntMax => Interval::new(
            left.min.max(right.min),
            left.max.max(right.max),
            left.nan || right.nan,
        ),
        Binary::ValueOr => {
            Interval::new(left.min.min(right.min), left.max.max(right.max), right.nan)
        }
        Binary::IntRemainder if right.excludes_zero() => {
            let magnitude = right.min.abs().max(right.max.abs()) - 1.0;
            Interval::new(-magnitude, magnitude, false)
        }
        Binary::MarkLastIndex => match length(graph, a, known) {
            Some(length) => Interval::new(-1.0, length as f64 - 1.0, false),
            None => Interval::new(-1.0, f64::INFINITY, false),
        },
        _ => Interval::ANY,
    }
}

/// The length of an array or marks value, when known.
fn length(graph: &Graph, node: Node, known: &impl Known) -> Option<usize> {
    match graph.op(node) {
        Op::Param(index) => known.param_length(*index),
        Op::Constant(constant) => match &constant.value {
            Value::Array(items) => Some(items.len()),
            Value::Marks(marks) => Some(marks.len()),
            _ => None,
        },
        _ => None,
    }
}

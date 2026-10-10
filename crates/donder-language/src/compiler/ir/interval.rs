//! Value ranges of numeric nodes, from literals, declared parameter ranges and
//! array lengths. They prove reduction bounds and nonzero divisors.
use super::{Binary, Context, Graph, Node, Op, Ternary, Unary};
use donder_runtime_types::Value;
use std::collections::HashMap;

/// An inclusive range of a numeric value, and whether it may be NaN.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Interval {
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) nan: bool,
}

impl Interval {
    const ANY: Self = Self {
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

    fn new(min: f64, max: f64, nan: bool) -> Self {
        Self { min, max, nan }.normalized()
    }

    fn normalized(self) -> Self {
        if self.min.is_nan() || self.max.is_nan() {
            return Self::ANY;
        }
        self
    }

    fn union(self, other: Self) -> Self {
        Self::new(
            self.min.min(other.min),
            self.max.max(other.max),
            self.nan || other.nan,
        )
    }

    pub(crate) fn excludes_zero(self) -> bool {
        self.min > 0.0 || self.max < 0.0
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
}

/// What is known about parameters: declared numeric ranges, and the lengths of
/// arrays and marks once an instance supplies them.
pub(crate) struct Bounds<'a> {
    pub(crate) ranges: &'a [Option<(f64, f64)>],
    pub(crate) lengths: &'a [Option<usize>],
}

pub(crate) fn interval(graph: &Graph, node: Node, bounds: &Bounds<'_>) -> Interval {
    Analysis {
        graph,
        bounds,
        memo: HashMap::new(),
    }
    .of(node)
}

struct Analysis<'a> {
    graph: &'a Graph,
    bounds: &'a Bounds<'a>,
    memo: HashMap<Node, Interval>,
}

impl Analysis<'_> {
    fn of(&mut self, node: Node) -> Interval {
        if let Some(&interval) = self.memo.get(&node) {
            return interval;
        }
        let interval = self.compute(node);
        self.memo.insert(node, interval);
        interval
    }

    fn compute(&mut self, node: Node) -> Interval {
        let graph = self.graph;
        match graph.op(node) {
            Op::Constant(constant) => match constant.value {
                Value::Int(value) => Interval::exact(f64::from(value)),
                Value::Float(value) => Interval::exact(f64::from(value)),
                _ => Interval::ANY,
            },
            Op::Param(index) => match self.bounds.ranges.get(*index as usize) {
                Some(Some((min, max))) => Interval::new(*min, *max, false),
                _ => Interval::ANY,
            },
            Op::LoopIndex(id) => {
                let data = graph.loop_(*id);
                let (start, end) = (self.of(data.start), self.of(data.end));
                Interval::new(start.min, end.max - 1.0, false)
            }
            Op::Context(Context::PixelIndex) => Interval::new(0.0, f64::INFINITY, false),
            Op::ParamIntegral(_)
            | Op::Context(_)
            | Op::Sample { .. }
            | Op::Reduce(_)
            | Op::Items(_)
            | Op::Source
            | Op::Tap { .. }
            | Op::Previous
            | Op::Scan { .. }
            | Op::ScanTap { .. } => Interval::ANY,
            Op::Pick { items, .. } => {
                let items = items.clone();
                items
                    .iter()
                    .map(|&item| self.of(item))
                    .reduce(Interval::union)
                    .unwrap_or(Interval::ANY)
            }
            Op::Unary(op, a) => {
                let a = *a;
                self.unary(*op, a)
            }
            Op::Binary(op, a, b) => {
                let (a, b) = (*a, *b);
                self.binary(*op, a, b)
            }
            Op::Ternary(Ternary::Clamp, value, min, max) => {
                let (value, min, max) = (self.of(*value), self.of(*min), self.of(*max));
                // An inverted or NaN bound gives NaN.
                let nan = value.nan || min.nan || max.nan || min.max > max.min;
                Interval::new(
                    value.min.max(min.min).min(max.max),
                    value.max.min(max.max).max(min.min),
                    nan,
                )
            }
            Op::Ternary(..) => Interval::ANY,
            Op::Select(_, yes, no) => {
                let (yes, no) = (*yes, *no);
                self.of(yes).union(self.of(no))
            }
        }
    }

    fn unary(&mut self, op: Unary, a: Node) -> Interval {
        let value = self.of(a);
        match op {
            Unary::Negate | Unary::IntNegate => Interval::new(-value.max, -value.min, value.nan),
            Unary::Abs => {
                let low = if value.min <= 0.0 && value.max >= 0.0 {
                    0.0
                } else {
                    value.min.abs().min(value.max.abs())
                };
                Interval::new(low, value.min.abs().max(value.max.abs()), value.nan)
            }
            Unary::Floor => value.monotonic(f64::floor),
            Unary::Ceil => value.monotonic(f64::ceil),
            Unary::Trunc => value.monotonic(f64::trunc),
            Unary::RoundEven => value.monotonic(|value| value.round_ties_even()),
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
            Unary::Len | Unary::MarkCount => match self.length(a) {
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

    fn binary(&mut self, op: Binary, a: Node, b: Node) -> Interval {
        let (left, right) = (self.of(a), self.of(b));
        match op {
            Binary::Add | Binary::IntAdd => left.corners(right, |a, b| a + b),
            Binary::Subtract | Binary::IntSubtract => left.corners(right, |a, b| a - b),
            Binary::Multiply | Binary::IntMultiply => left.corners(right, |a, b| a * b),
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
            Binary::MarkLastIndex => match self.length(a) {
                Some(length) => Interval::new(-1.0, length as f64 - 1.0, false),
                None => Interval::new(-1.0, f64::INFINITY, false),
            },
            _ => Interval::ANY,
        }
    }

    /// The length of an array or marks parameter, when known.
    fn length(&self, node: Node) -> Option<usize> {
        match self.graph.op(node) {
            Op::Param(index) => self.bounds.lengths.get(*index as usize).copied().flatten(),
            Op::Constant(constant) => match &constant.value {
                Value::Array(items) => Some(items.len()),
                Value::Marks(marks) => Some(marks.len()),
                _ => None,
            },
            _ => None,
        }
    }
}

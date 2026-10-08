//! Host semantics of IR operations. Scalar functions are shared with the
//! runtime through `donder_runtime_types::sampling`, so folding and preparation compute the
//! values playback would. Transcendental functions, context reads and signal
//! samples are never evaluated here: their values must come from playback.
use super::{Binary, Graph, Node, Op, Reducer, Ternary, Unary};
use donder_runtime_types::Color;
use donder_runtime_types::bytecode::MAX_ITERATIONS;
use donder_runtime_types::bytecode::{FloatBinary, FloatUnary};
use donder_runtime_types::sampling;
use donder_runtime_types::{Type, Value};
use std::collections::HashMap;

pub(super) fn unary(op: Unary, value: &Value) -> Option<Value> {
    use Unary::*;
    let float = |op| Some(Value::Float(sampling::float_unary(op, as_float(value)?)));
    match op {
        Negate => Some(Value::Float(-as_float(value)?)),
        IntNegate => Some(Value::Int(as_int(value)?.wrapping_neg())),
        Not => Some(Value::Bool(!as_bool(value)?)),
        Abs => float(FloatUnary::Abs),
        Floor => float(FloatUnary::Floor),
        Ceil => float(FloatUnary::Ceil),
        Trunc => float(FloatUnary::Trunc),
        RoundEven => float(FloatUnary::RoundEven),
        Sqrt => float(FloatUnary::Sqrt),
        IntToFloat => Some(Value::Float(as_int(value)? as f32)),
        // `as` truncates toward zero, saturates, and maps NaN to zero.
        FloatToInt => Some(Value::Int(as_float(value)? as i32)),
        Smoothstep => Some(Value::Float(sampling::smoothstep(as_float(value)?))),
        Rand => Some(Value::Float(sampling::deterministic_random_seed(as_float(
            value,
        )?))),
        Hue => Some(Value::Float(sampling::color_hue(as_color(value)?))),
        Red => Some(Value::Float(sampling::color_channel(as_color(value)?.red))),
        Green => Some(Value::Float(sampling::color_channel(
            as_color(value)?.green,
        ))),
        Blue => Some(Value::Float(sampling::color_channel(as_color(value)?.blue))),
        Saturation => Some(Value::Float(sampling::color_saturation(as_color(value)?))),
        Intensity => Some(Value::Float(sampling::color_intensity(as_color(value)?))),
        Invert => Some(Value::Color(sampling::invert_color(as_color(value)?))),
        Len => match value {
            Value::Array(items) => Some(Value::Int(sampling::length_int(items.len()))),
            _ => None,
        },
        MarkCount => match value {
            Value::Marks(marks) => Some(Value::Int(sampling::length_int(marks.as_slice().len()))),
            _ => None,
        },
        Sin | Cos | Tan | Exp | Log | SectionCount | SectionIndex | QuerySeconds
        | QueryProgress => None,
    }
}

pub(super) fn binary(op: Binary, left: &Value, right: &Value, ty: &Type) -> Option<Value> {
    use Binary::*;
    let floats = || Some((as_float(left)?, as_float(right)?));
    let ints = || Some((as_int(left)?, as_int(right)?));
    let colors = || Some((as_color(left)?, as_color(right)?));
    Some(match op {
        Add => floats().map(|(a, b)| Value::Float(a + b))?,
        Subtract => floats().map(|(a, b)| Value::Float(a - b))?,
        Multiply => floats().map(|(a, b)| Value::Float(a * b))?,
        Divide => floats().map(|(a, b)| Value::Float(a / b))?,
        Remainder => floats().map(|(a, b)| Value::Float(sampling::float_remainder(a, b)))?,
        IntAdd => ints().map(|(a, b)| Value::Int(a.wrapping_add(b)))?,
        IntSubtract => ints().map(|(a, b)| Value::Int(a.wrapping_sub(b)))?,
        IntMultiply => ints().map(|(a, b)| Value::Int(a.wrapping_mul(b)))?,
        IntRemainder => ints().map(|(a, b)| Value::Int(sampling::int_remainder(a, b)))?,
        IntFloorDivide => ints().map(|(a, b)| Value::Int(sampling::int_floor_divide(a, b)))?,
        IntMin => ints().map(|(a, b)| Value::Int(a.min(b)))?,
        IntMax => ints().map(|(a, b)| Value::Int(a.max(b)))?,
        Min => {
            floats().map(|(a, b)| Value::Float(sampling::float_binary(FloatBinary::Min, a, b)))?
        }
        Max => {
            floats().map(|(a, b)| Value::Float(sampling::float_binary(FloatBinary::Max, a, b)))?
        }
        ValueOr => floats()
            .map(|(a, b)| Value::Float(sampling::float_binary(FloatBinary::ValueOr, a, b)))?,
        Less => floats().map(|(a, b)| Value::Bool(a < b))?,
        LessEqual => floats().map(|(a, b)| Value::Bool(a <= b))?,
        Greater => floats().map(|(a, b)| Value::Bool(a > b))?,
        GreaterEqual => floats().map(|(a, b)| Value::Bool(a >= b))?,
        IntLess => ints().map(|(a, b)| Value::Bool(a < b))?,
        IntLessEqual => ints().map(|(a, b)| Value::Bool(a <= b))?,
        IntGreater => ints().map(|(a, b)| Value::Bool(a > b))?,
        IntGreaterEqual => ints().map(|(a, b)| Value::Bool(a >= b))?,
        Equal => Value::Bool(scalar_equal(left, right)?),
        NotEqual => Value::Bool(!scalar_equal(left, right)?),
        ColorAdd => colors().map(|(a, b)| Value::Color(sampling::add_colors(a, b)))?,
        ColorMultiply => colors().map(|(a, b)| Value::Color(sampling::multiply_colors(a, b)))?,
        ColorMax => colors().map(|(a, b)| Value::Color(sampling::max_colors(a, b)))?,
        ColorScale => Value::Color(sampling::scale_color(as_color(left)?, as_float(right)?)),
        CurveSample => match left {
            Value::Curve(curve) => Value::Float(sampling::sample_curve(curve, as_float(right)?)),
            _ => return None,
        },
        CurveIntegral => match left {
            Value::Curve(curve) => Value::Float(sampling::curve_integral(curve, as_float(right)?)),
            _ => return None,
        },
        GradientSample => match left {
            Value::Gradient(gradient) => {
                Value::Color(sampling::sample_gradient(gradient, as_float(right)?))
            }
            _ => return None,
        },
        Index => match left {
            Value::Array(items) if items.is_empty() => ty.default_value(),
            Value::Array(items) => {
                items[sampling::clamp_array_index(as_int(right)?, items.len())].clone()
            }
            _ => return None,
        },
        MarkAt => match left {
            Value::Marks(marks) => Value::Float(sampling::mark_at(marks, as_int(right)?)),
            _ => return None,
        },
        MarkLast => match left {
            Value::Marks(marks) => Value::Float(
                sampling::previous_mark(marks, as_float(right)?).map_or(f32::NAN, |(_, time)| time),
            ),
            _ => return None,
        },
        MarkLastIndex => match left {
            Value::Marks(marks) => {
                Value::Int(sampling::previous_mark_index(marks, as_float(right)?))
            }
            _ => return None,
        },
        CurveFirstCrossing => match left {
            Value::Curve(curve) => {
                Value::Float(sampling::curve_crossing(curve, as_float(right)?, f32::NAN))
            }
            _ => return None,
        },
        Power => Value::Float(power(as_float(left)?, as_int(right)?)),
        Atan2 | PowerFloat | SectionPosition => return None,
    })
}

pub(super) fn ternary(op: Ternary, a: &Value, b: &Value, c: &Value) -> Option<Value> {
    Some(match op {
        Ternary::Clamp => Value::Float(sampling::clamp_float(
            as_float(a)?,
            as_float(b)?,
            as_float(c)?,
        )),
        Ternary::Mix => {
            let (left, right, amount) = (as_float(a)?, as_float(b)?, as_float(c)?);
            Value::Float(left + (right - left) * amount)
        }
        Ternary::MixColor => Value::Color(sampling::mix_colors(
            as_color(a)?,
            as_color(b)?,
            as_float(c)?,
        )),
        Ternary::Rgb => Value::Color(sampling::rgb(as_float(a)?, as_float(b)?, as_float(c)?)),
        Ternary::Hsv => Value::Color(sampling::hsv(as_float(a)?, as_float(b)?, as_float(c)?)),
        Ternary::CurveLastCrossing => match a {
            Value::Curve(curve) => Value::Float(sampling::curve_last_crossing(
                curve,
                as_float(b)?,
                as_float(c)?,
            )),
            _ => return None,
        },
    })
}

/// `x` multiplied into one, `n` times; a nonpositive count gives one.
pub(crate) fn power(x: f32, n: i32) -> f32 {
    let mut result = 1.0;
    for _ in 0..n.max(0) {
        result *= x;
    }
    result
}

/// The value of a reduction with no contributions. `first` and `last` use
/// their default instead.
pub(crate) fn reduce_identity(reducer: Reducer, ty: &Type) -> Option<Value> {
    const WHITE: Color = Color {
        red: 255,
        green: 255,
        blue: 255,
    };
    Some(match (reducer, ty) {
        (Reducer::Max, Type::Float) => Value::Float(f32::NEG_INFINITY),
        (Reducer::Min, Type::Float) => Value::Float(f32::INFINITY),
        (Reducer::Sum, Type::Float) => Value::Float(0.0),
        (Reducer::Max, Type::Int) => Value::Int(i32::MIN),
        (Reducer::Min, Type::Int) => Value::Int(i32::MAX),
        (Reducer::Sum, Type::Int) => Value::Int(0),
        (Reducer::Max | Reducer::Sum, Type::Color) => Value::Color(Color::BLACK),
        (Reducer::Min, Type::Color) => Value::Color(WHITE),
        (Reducer::Any, Type::Bool) => Value::Bool(false),
        (Reducer::All, Type::Bool) => Value::Bool(true),
        _ => return None,
    })
}

/// The accumulation step of `max`, `min` and `sum`.
pub(crate) fn combine(reducer: Reducer, accumulator: &Value, value: &Value) -> Option<Value> {
    let ty = super::value_type(value);
    let op = match (reducer, ty) {
        (Reducer::Max, Type::Float) => Binary::Max,
        (Reducer::Min, Type::Float) => Binary::Min,
        (Reducer::Sum, Type::Float) => Binary::Add,
        (Reducer::Sum, Type::Int) => Binary::IntAdd,
        (Reducer::Max, Type::Color) => Binary::ColorMax,
        (Reducer::Sum, Type::Color) => Binary::ColorAdd,
        (Reducer::Max, Type::Int) => {
            return Some(Value::Int(as_int(accumulator)?.max(as_int(value)?)));
        }
        (Reducer::Min, Type::Int) => {
            return Some(Value::Int(as_int(accumulator)?.min(as_int(value)?)));
        }
        (Reducer::Min, Type::Color) => {
            let (a, b) = (as_color(accumulator)?, as_color(value)?);
            return Some(Value::Color(Color {
                red: a.red.min(b.red),
                green: a.green.min(b.green),
                blue: a.blue.min(b.blue),
            }));
        }
        _ => return None,
    };
    binary(op, accumulator, value, &Type::Void)
}

/// Evaluate `node` with the given parameter values; `None` where a value can
/// only come from playback: context, signals, automated parameters (`None`
/// entries) and functions the runtime must compute.
pub(crate) fn evaluate(graph: &Graph, node: Node, params: &[Option<Value>]) -> Option<Value> {
    Evaluator::new(graph, params).value(node)
}

/// An evaluator that keeps results across queries of one graph.
pub(crate) struct Evaluator<'a> {
    graph: &'a Graph,
    params: &'a [Option<Value>],
    memo: HashMap<Node, Option<Value>>,
    indices: Vec<Option<i32>>,
}

impl<'a> Evaluator<'a> {
    pub(crate) fn new(graph: &'a Graph, params: &'a [Option<Value>]) -> Self {
        Self {
            graph,
            params,
            memo: HashMap::new(),
            indices: vec![None; graph.loop_count()],
        }
    }

    pub(crate) fn value(&mut self, node: Node) -> Option<Value> {
        if let Some(value) = self.memo.get(&node) {
            return value.clone();
        }
        let value = self.compute(node);
        self.memo.insert(node, value.clone());
        value
    }

    fn compute(&mut self, node: Node) -> Option<Value> {
        let graph = self.graph;
        match graph.op(node) {
            Op::Constant(constant) => Some(constant.value.clone()),
            Op::Param(index) => self.params.get(*index as usize)?.clone(),
            Op::ParamIntegral(_) | Op::Context(_) | Op::Sample { .. } | Op::Items(_) => None,
            Op::Pick { index, items } => {
                let index = as_int(&self.value(*index)?)?;
                let item = items[sampling::clamp_array_index(index, items.len())];
                self.value(item)
            }
            Op::LoopIndex(id) => self.indices[id.index()].map(Value::Int),
            Op::Unary(op, a) => {
                let a = self.value(*a)?;
                unary(*op, &a)
            }
            Op::Binary(op, a, b) => {
                let (a, b) = (self.value(*a)?, self.value(*b)?);
                binary(*op, &a, &b, graph.ty(node))
            }
            Op::Ternary(op, a, b, c) => {
                let (a, b, c) = (self.value(*a)?, self.value(*b)?, self.value(*c)?);
                ternary(*op, &a, &b, &c)
            }
            Op::Select(condition, yes, no) => {
                if as_bool(&self.value(*condition)?)? {
                    self.value(*yes)
                } else {
                    self.value(*no)
                }
            }
            Op::Reduce(id) => self.reduce(*id),
        }
    }

    fn reduce(&mut self, id: super::LoopId) -> Option<Value> {
        let data = self.graph.loop_(id).clone();
        let start = as_int(&self.value(data.start)?)?;
        let end = as_int(&self.value(data.end)?)?;
        let count = i64::from(end) - i64::from(start);
        if count > MAX_ITERATIONS as i64 {
            return None;
        }
        let indices: Vec<i32> = match data.reducer {
            Reducer::Last => (start..end).rev().collect(),
            _ => (start..end).collect(),
        };
        let ty = self.graph.ty(data.body).clone();
        let mut accumulator = reduce_identity(data.reducer, &ty);
        for index in indices {
            self.indices[id.index()] = Some(index);
            let graph = self.graph;
            self.memo
                .retain(|node, _| !graph.loops_of(*node).contains(id));
            let keep = match data.filter {
                Some(filter) => as_bool(&self.value(filter)?)?,
                None => true,
            };
            match data.reducer {
                Reducer::Max | Reducer::Min | Reducer::Sum => {
                    if keep {
                        let value = self.value(data.body)?;
                        accumulator = Some(combine(data.reducer, &accumulator?, &value)?);
                    }
                }
                Reducer::Any => {
                    if as_bool(&self.value(data.body)?)? {
                        return Some(Value::Bool(true));
                    }
                }
                Reducer::All => {
                    if !as_bool(&self.value(data.body)?)? {
                        return Some(Value::Bool(false));
                    }
                }
                Reducer::First | Reducer::Last => {
                    if keep {
                        return self.value(data.body);
                    }
                }
            }
        }
        self.indices[id.index()] = None;
        match data.reducer {
            Reducer::First | Reducer::Last => self.value(data.default?),
            _ => accumulator,
        }
    }
}

/// Equality of two scalars of one type; resources are never compared.
fn scalar_equal(left: &Value, right: &Value) -> Option<bool> {
    Some(match (left, right) {
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Color(a), Value::Color(b)) => a == b,
        (Value::Enum(a), Value::Enum(b)) => a == b,
        _ => return None,
    })
}

pub(crate) fn as_float(value: &Value) -> Option<f32> {
    match value {
        Value::Float(value) => Some(*value),
        _ => None,
    }
}

pub(crate) fn as_int(value: &Value) -> Option<i32> {
    match value {
        Value::Int(value) => Some(*value),
        _ => None,
    }
}

pub(crate) fn as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        _ => None,
    }
}

pub(crate) fn as_color(value: &Value) -> Option<Color> {
    match value {
        Value::Color(value) => Some(*value),
        _ => None,
    }
}

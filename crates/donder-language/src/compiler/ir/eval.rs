//! Host semantics of IR operations. Scalar functions are shared with the
//! runtime through `donder_runtime_types::sampling`, so folding and preparation compute the
//! values playback would. Transcendental functions, context reads and signal
//! samples are never evaluated here: their values must come from playback.
//! Playback computes colors in floats and rounds them only when written, so a
//! color folds only when its float value is an 8-bit color.
use super::{Binary, Graph, Node, Op, Quaternary, Reducer, Ternary, Unary};
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
        Negate => match value {
            Value::Int(value) => Some(Value::Int(value.wrapping_neg())),
            _ => Some(Value::Float(-as_float(value)?)),
        },
        Not => Some(Value::Bool(!as_bool(value)?)),
        Abs => match value {
            Value::Int(value) => Some(Value::Int(value.wrapping_abs())),
            _ => float(FloatUnary::Abs),
        },
        Floor => float(FloatUnary::Floor),
        Ceil => float(FloatUnary::Ceil),
        Trunc => float(FloatUnary::Trunc),
        Sqrt => float(FloatUnary::Sqrt),
        IntToFloat => Some(Value::Float(as_int(value)? as f32)),
        // `as` truncates toward zero, saturates, and maps NaN to zero.
        FloatToInt => Some(Value::Int(as_float(value)? as i32)),
        Smoothstep => Some(Value::Float(sampling::smoothstep(as_float(value)?))),
        Rand => Some(Value::Float(sampling::deterministic_random_seed(as_float(
            value,
        )?))),
        Hue => Some(Value::Float(sampling::color_hue(as_color(value)?))),
        Red => Some(Value::Float(as_color(value)?.red)),
        Green => Some(Value::Float(as_color(value)?.green)),
        Blue => Some(Value::Float(as_color(value)?.blue)),
        Saturation => Some(Value::Float(sampling::color_saturation(as_color(value)?))),
        Intensity => Some(Value::Float(sampling::color_intensity(as_color(value)?))),
        Invert => folded(sampling::invert_color(as_color(value)?)),
        Len => match value {
            Value::Array(items) => Some(Value::Int(items.len() as i32)),
            Value::Marks(marks) => Some(Value::Int(marks.len() as i32)),
            _ => None,
        },
        ImageWidth | ImageHeight | ImageRate | ImageFrames => match value {
            Value::Image(image) => Some(match op {
                ImageWidth => Value::Float(image.width as f32),
                ImageHeight => Value::Float(image.height as f32),
                ImageRate => Value::Float(image.rate),
                _ => Value::Int(image.frames as i32),
            }),
            _ => None,
        },
        Live => None,
        FontHeight => match value {
            Value::Font(font) => Some(Value::Float(font.height)),
            _ => None,
        },
        Sin | Cos | Tan | Exp | Log => None,
    }
}

pub(super) fn binary(op: Binary, left: &Value, right: &Value, ty: &Type) -> Option<Value> {
    use Binary::*;
    let floats = || Some((as_float(left)?, as_float(right)?));
    let ints = || Some((as_int(left)?, as_int(right)?));
    let colors = || Some((as_color(left)?, as_color(right)?));
    let int_left = matches!(left, Value::Int(_));
    Some(match op {
        Add | Subtract | Multiply | Divide | Remainder | Min | Max if int_left => {
            let (a, b) = ints()?;
            Value::Int(match op {
                Add => a.wrapping_add(b),
                Subtract => a.wrapping_sub(b),
                Multiply => a.wrapping_mul(b),
                Divide => sampling::int_floor_divide(a, b),
                Remainder => sampling::int_remainder(a, b),
                Min => a.min(b),
                _ => a.max(b),
            })
        }
        Add | Multiply | Max if matches!(left, Value::Color(_)) => match (op, right) {
            (Add, _) => colors().and_then(|(a, b)| folded(sampling::add_colors(a, b)))?,
            (Max, _) => colors().and_then(|(a, b)| folded(sampling::max_colors(a, b)))?,
            (_, Value::Color(_)) => {
                colors().and_then(|(a, b)| folded(sampling::multiply_colors(a, b)))?
            }
            _ => folded(sampling::scale_color(as_color(left)?, as_float(right)?))?,
        },
        Add => floats().map(|(a, b)| Value::Float(a + b))?,
        Subtract => floats().map(|(a, b)| Value::Float(a - b))?,
        Multiply => floats().map(|(a, b)| Value::Float(a * b))?,
        Divide => floats().map(|(a, b)| Value::Float(a / b))?,
        Remainder => floats().map(|(a, b)| Value::Float(sampling::float_remainder(a, b)))?,
        Min => {
            floats().map(|(a, b)| Value::Float(sampling::float_binary(FloatBinary::Min, a, b)))?
        }
        Max => {
            floats().map(|(a, b)| Value::Float(sampling::float_binary(FloatBinary::Max, a, b)))?
        }
        ValueOr => floats()
            .map(|(a, b)| Value::Float(sampling::float_binary(FloatBinary::ValueOr, a, b)))?,
        Less | LessEqual | Greater | GreaterEqual if int_left => {
            let (a, b) = ints()?;
            Value::Bool(match op {
                Less => a < b,
                LessEqual => a <= b,
                Greater => a > b,
                _ => a >= b,
            })
        }
        Less => floats().map(|(a, b)| Value::Bool(a < b))?,
        LessEqual => floats().map(|(a, b)| Value::Bool(a <= b))?,
        Greater => floats().map(|(a, b)| Value::Bool(a > b))?,
        GreaterEqual => floats().map(|(a, b)| Value::Bool(a >= b))?,
        Equal => Value::Bool(scalar_equal(left, right)?),
        NotEqual => Value::Bool(!scalar_equal(left, right)?),
        GradientSample => match left {
            Value::Gradient(gradient) => {
                folded(sampling::sample_gradient(gradient, as_float(right)?))?
            }
            _ => return None,
        },
        CurveSample => match left {
            Value::Curve(curve) => Value::Float(
                sampling::CurveTables::new(&curve.points)
                    .view()
                    .sample(as_float(right)?),
            ),
            _ => return None,
        },
        CurveIntegral => match left {
            Value::Curve(curve) => Value::Float(
                sampling::CurveTables::new(&curve.points)
                    .view()
                    .integral(as_float(right)?),
            ),
            _ => return None,
        },
        Index | Entry => match left {
            Value::Array(items) if items.is_empty() => ty.default_value(),
            Value::Array(items) => {
                items[sampling::clamp_array_index(as_int(right)?, items.len())].clone()
            }
            _ => return None,
        },
        MarkAt => match left {
            Value::Marks(marks) => Value::Float(sampling::mark_at(&marks.view(), as_int(right)?)),
            _ => return None,
        },
        GlyphAdvance => match left {
            Value::Font(font) => {
                Value::Float(sampling::glyph_advance(&font.view(), as_int(right)?))
            }
            _ => return None,
        },
        MarkDuration => match left {
            Value::Marks(marks) => {
                Value::Float(sampling::mark_duration(&marks.view(), as_int(right)?))
            }
            _ => return None,
        },
        MarkLastIndex => match left {
            Value::Marks(marks) => Value::Int(sampling::previous_mark_index(
                &marks.view(),
                as_float(right)?,
                &core::cell::Cell::new(0),
            )),
            _ => return None,
        },
        Power => Value::Float(power(as_float(left)?, as_int(right)?)),
        Atan2 => return None,
    })
}

pub(super) fn ternary(op: Ternary, a: &Value, b: &Value, c: &Value) -> Option<Value> {
    Some(match op {
        Ternary::Clamp => Value::Float(sampling::clamp_float(
            as_float(a)?,
            as_float(b)?,
            as_float(c)?,
        )),
        Ternary::Mix if matches!(a, Value::Color(_)) => folded(sampling::mix_colors(
            as_color(a)?,
            as_color(b)?,
            as_float(c)?,
        ))?,
        Ternary::Mix => {
            let (left, right, amount) = (as_float(a)?, as_float(b)?, as_float(c)?);
            Value::Float(left + (right - left) * amount)
        }
        Ternary::Rgb => folded(sampling::rgb(as_float(a)?, as_float(b)?, as_float(c)?))?,
        Ternary::Hsv => folded(sampling::hsv(as_float(a)?, as_float(b)?, as_float(c)?))?,
        Ternary::MarkText => match a {
            Value::Marks(marks) => Value::Int(sampling::mark_label_char(
                &marks.view(),
                as_int(b)?,
                as_int(c)?,
            )),
            _ => return None,
        },
        Ternary::Partition => match a {
            Value::Array(entries) => {
                let len = (as_int(b)?.max(0) as usize).min(entries.len());
                let at_most = |entry: &Value| match (entry, c) {
                    (Value::Float(entry), Value::Float(value)) => Some(entry <= value),
                    _ => None,
                };
                let mut count = 0;
                for entry in &entries[..len] {
                    if !at_most(entry)? {
                        break;
                    }
                    count += 1;
                }
                Value::Int(count)
            }
            _ => return None,
        },
        Ternary::TrackAt => match a {
            Value::Track(track) => {
                Value::Float(sampling::track_at(&track.view(), as_int(b)?, as_float(c)?))
            }
            _ => return None,
        },
        Ternary::CurveFirstCrossing => match a {
            Value::Curve(curve) => Value::Float(
                sampling::CurveTables::new(&curve.points)
                    .view()
                    .crossing(as_float(b)?, as_float(c)?),
            ),
            _ => return None,
        },
        Ternary::CurveLastCrossing => match a {
            Value::Curve(curve) => Value::Float(
                sampling::CurveTables::new(&curve.points)
                    .view()
                    .last_crossing(as_float(b)?, as_float(c)?),
            ),
            _ => return None,
        },
    })
}

/// `x` multiplied into one, `n` times; a nonpositive count gives one.
fn power(x: f32, n: i32) -> f32 {
    let mut result = 1.0;
    for _ in 0..n.max(0) {
        result *= x;
    }
    result
}

/// The value of a reduction with no contributions. `first` and `last` use
/// their default instead.
pub(crate) fn reduce_identity(reducer: Reducer, ty: &Type) -> Option<Value> {
    Some(match (reducer, ty) {
        (Reducer::Max, Type::Float) => Value::Float(f32::NEG_INFINITY),
        (Reducer::Min, Type::Float) => Value::Float(f32::INFINITY),
        (Reducer::Sum, Type::Float) => Value::Float(0.0),
        (Reducer::Max, Type::Int) => Value::Int(i32::MIN),
        (Reducer::Min, Type::Int) => Value::Int(i32::MAX),
        (Reducer::Sum, Type::Int) => Value::Int(0),
        (Reducer::Max | Reducer::Sum, Type::Color) => Value::Color(Color::BLACK),
        (Reducer::Any, Type::Bool) => Value::Bool(false),
        (Reducer::All, Type::Bool) => Value::Bool(true),
        _ => return None,
    })
}

/// The accumulation step of `max`, `min` and `sum`.
fn combine(reducer: Reducer, accumulator: &Value, value: &Value) -> Option<Value> {
    let ty = super::value_type(value);
    let op = match (reducer, ty) {
        (Reducer::Max, Type::Float | Type::Int | Type::Color) => Binary::Max,
        (Reducer::Min, Type::Float | Type::Int) => Binary::Min,
        (Reducer::Sum, Type::Float | Type::Int | Type::Color) => Binary::Add,
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
            Op::ParamIntegral(_)
            | Op::Context(_)
            | Op::Read { .. }
            | Op::Items(_)
            | Op::Neighbor
            | Op::Tap { .. }
            | Op::Scan { .. } => None,
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
            Op::Quaternary(Quaternary::ImageColor, image, frame, x, y) => {
                let (image, frame) = (self.value(*image)?, self.value(*frame)?);
                let (x, y) = (self.value(*x)?, self.value(*y)?);
                match image {
                    Value::Image(image) => Some(Value::Color(sampling::image_color(
                        &image.view(),
                        as_int(&frame)?,
                        as_float(&x)?,
                        as_float(&y)?,
                    ))),
                    _ => None,
                }
            }
            Op::Quaternary(Quaternary::GlyphCoverage, font, code, x, y) => {
                let (font, code) = (self.value(*font)?, self.value(*code)?);
                let (x, y) = (self.value(*x)?, self.value(*y)?);
                match font {
                    Value::Font(font) => Some(Value::Float(sampling::glyph_coverage(
                        &font.view(),
                        as_int(&code)?,
                        as_float(&x)?,
                        as_float(&y)?,
                    ))),
                    _ => None,
                }
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

    /// A table's entries: the body at each index, or its running sums from
    /// zero.
    fn tabulate(
        &mut self,
        id: super::LoopId,
        data: &super::Loop,
        start: i32,
        end: i32,
    ) -> Option<Value> {
        let ty = self.graph.ty(data.body).clone();
        let mut entries = Vec::new();
        let mut sum = reduce_identity(Reducer::Sum, &ty);
        if data.reducer == Reducer::RunningSum {
            entries.push(sum.clone()?);
        }
        for index in start..end {
            self.indices[id.index()] = Some(index);
            let graph = self.graph;
            self.memo
                .retain(|node, _| !graph.loops_of(*node).contains(id));
            let value = self.value(data.body)?;
            match data.reducer {
                Reducer::RunningSum => {
                    sum = Some(combine(Reducer::Sum, &sum?, &value)?);
                    entries.push(sum.clone()?);
                }
                _ => entries.push(value),
            }
        }
        self.indices[id.index()] = None;
        Some(Value::Array(entries.into()))
    }

    fn reduce(&mut self, id: super::LoopId) -> Option<Value> {
        let data = self.graph.loop_(id).clone();
        let start = as_int(&self.value(data.start)?)?;
        let end = as_int(&self.value(data.end)?)?;
        let count = i64::from(end) - i64::from(start);
        if count > MAX_ITERATIONS as i64 {
            return None;
        }
        if data.reducer.tabulates() {
            return self.tabulate(id, &data, start, end);
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
                Reducer::Table | Reducer::RunningSum => {
                    unreachable!("tables are tabulated")
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

/// `color` as a constant: color math is exact in channels.
fn folded(color: Color) -> Option<Value> {
    Some(Value::Color(color))
}

pub(crate) fn as_color(value: &Value) -> Option<Color> {
    match value {
        Value::Color(value) => Some(*value),
        _ => None,
    }
}

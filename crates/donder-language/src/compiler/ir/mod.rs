//! Typed, pure dataflow graph of one effect or operator body.
//!
//! Nodes are hash-consed: building an operation that already exists returns
//! the existing node, so equal subexpressions are shared by construction.
//! Every node records its type, the domain it varies in and the reductions
//! whose index it depends on, all computed when it is built. Construction also
//! folds constants and simple identities.
mod eval;
mod fold;
pub(crate) mod interval;
mod rebuild;

pub(crate) use eval::{Evaluator, evaluate, reduce_identity};
pub(crate) use rebuild::{Rebuild, Substitute};

use donder_runtime_types::Color;
use donder_runtime_types::bytecode::SignalPixel;
use donder_runtime_types::{Type, Value};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Node(u32);

impl Node {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct LoopId(u32);

impl LoopId {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// What a value varies with. Domains join by union; an empty domain is a
/// compile-time constant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Domain(u8);

impl Domain {
    pub(crate) const CONSTANT: Self = Self(0);
    /// Fixed for one instance: bound parameters, the clip duration.
    pub(crate) const PARAM: Self = Self(1);
    /// The query time: `time`, `progress`, automated parameters.
    pub(crate) const TIME: Self = Self(2);
    /// The target run: pixel count of mixed fixtures and target bounds.
    pub(crate) const TARGET: Self = Self(4);
    /// The current pixel.
    pub(crate) const PIXEL: Self = Self(8);
    /// An upstream signal sample, which only playback can provide.
    pub(crate) const SIGNAL: Self = Self(16);

    pub(crate) fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub(crate) fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Fixed once an instance is bound: evaluable before playback.
    pub(crate) fn is_instance_fixed(self) -> bool {
        self.0 & !Self::PARAM.0 == 0
    }
}

/// The reductions whose index a value depends on. Reductions nest by
/// creation order, so the innermost is the highest identifier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct LoopSet(u64);

impl LoopSet {
    pub(crate) const LIMIT: usize = 64;

    fn single(id: LoopId) -> Self {
        Self(1 << id.0)
    }
    fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    fn without(self, id: LoopId) -> Self {
        Self(self.0 & !(1 << id.0))
    }
    pub(crate) fn contains(self, id: LoopId) -> bool {
        self.0 & (1 << id.0) != 0
    }
    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub(crate) fn innermost(self) -> Option<LoopId> {
        (self.0 != 0).then(|| LoopId(63 - self.0.leading_zeros()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Context {
    Time,
    Progress,
    Duration,
    PixelIndex,
    PixelFraction,
    PixelX,
    PixelY,
    TargetCount,
    TargetMinX,
    TargetMinY,
    TargetMaxX,
    TargetMaxY,
}

impl Context {
    pub(crate) fn ty(self) -> Type {
        match self {
            Self::PixelIndex | Self::TargetCount => Type::Int,
            _ => Type::Float,
        }
    }

    pub(crate) fn domain(self) -> Domain {
        match self {
            Self::Time | Self::Progress => Domain::TIME,
            Self::Duration => Domain::PARAM,
            Self::PixelIndex | Self::PixelFraction | Self::PixelX | Self::PixelY => Domain::PIXEL,
            Self::TargetCount
            | Self::TargetMinX
            | Self::TargetMinY
            | Self::TargetMaxX
            | Self::TargetMaxY => Domain::TARGET,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Unary {
    Negate,
    IntNegate,
    Not,
    Sin,
    Cos,
    Tan,
    Exp,
    /// Natural logarithm; NaN below zero.
    Log,
    Abs,
    Floor,
    Ceil,
    Trunc,
    RoundEven,
    Sqrt,
    IntToFloat,
    FloatToInt,
    /// Clamped cubic of an already normalized position.
    Smoothstep,
    Rand,
    Hue,
    Saturation,
    Intensity,
    Red,
    Green,
    Blue,
    Invert,
    Len,
    MarkCount,
    /// Sections of an integer width over the pixel's section population.
    SectionCount,
    SectionIndex,
    /// A fused source's clock at a consumer's query: quantized seconds and
    /// progress, NaN outside the sequence.
    QuerySeconds,
    QueryProgress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Binary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    IntAdd,
    IntSubtract,
    IntMultiply,
    IntRemainder,
    /// Floored quotient; division by zero is zero.
    IntFloorDivide,
    IntMin,
    IntMax,
    Min,
    Max,
    ValueOr,
    Atan2,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    IntLess,
    IntLessEqual,
    IntGreater,
    IntGreaterEqual,
    /// Same-typed scalar equality; floats compare by IEEE rules.
    Equal,
    NotEqual,
    ColorAdd,
    ColorMultiply,
    ColorMax,
    ColorScale,
    CurveSample,
    CurveIntegral,
    GradientSample,
    Index,
    MarkAt,
    MarkLast,
    MarkLastIndex,
    CurveFirstCrossing,
    /// Position within a section, from a width of at least one and its reciprocal.
    SectionPosition,
    /// A float multiplied by itself an integer number of times.
    Power,
    /// A float raised to a float exponent.
    PowerFloat,
}

impl Binary {
    pub(crate) fn commutative(self) -> bool {
        matches!(
            self,
            Self::Add
                | Self::Multiply
                | Self::IntAdd
                | Self::IntMultiply
                | Self::IntMin
                | Self::IntMax
                | Self::Min
                | Self::Max
                | Self::Equal
                | Self::NotEqual
                | Self::ColorAdd
                | Self::ColorMultiply
                | Self::ColorMax
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Ternary {
    Clamp,
    Mix,
    MixColor,
    Rgb,
    Hsv,
    CurveLastCrossing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Reducer {
    Max,
    Min,
    Sum,
    Any,
    All,
    First,
    Last,
}

/// A typed constant compared and hashed by bit pattern, so NaN and signed zero
/// are distinct values. The type names an enum's options or an empty array's item.
#[derive(Clone, Debug)]
pub(crate) struct Constant {
    pub(crate) value: Value,
    pub(crate) ty: Type,
}

impl PartialEq for Constant {
    fn eq(&self, other: &Self) -> bool {
        self.ty == other.ty && same_value(&self.value, &other.value)
    }
}

impl Eq for Constant {}

impl Hash for Constant {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_value(&self.value, state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Op {
    Constant(Constant),
    Param(u32),
    /// A float parameter integrated over the definition's time. Instantiation
    /// replaces it, so it never reaches preparation.
    ParamIntegral(u32),
    Context(Context),
    LoopIndex(LoopId),
    Unary(Unary, Node),
    Binary(Binary, Node, Node),
    Ternary(Ternary, Node, Node, Node),
    /// Pure choice. Whether an arm is evaluated lazily is a scheduling decision.
    Select(Node, Node, Node),
    Reduce(LoopId),
    Sample {
        input: u32,
        time: Node,
        pixel: SignalPixel<Node>,
    },
    /// An array literal. It exists only while it is indexed: playback never
    /// holds it as a value.
    Items(Box<[Node]>),
    /// The item at a clamped index.
    Pick {
        index: Node,
        items: Box<[Node]>,
    },
}

impl Op {
    /// Direct operands, in a stable order. A reduction's parts live in its loop.
    pub(crate) fn operands(&self) -> Vec<Node> {
        match *self {
            Self::Constant(_)
            | Self::Param(_)
            | Self::ParamIntegral(_)
            | Self::Context(_)
            | Self::LoopIndex(_)
            | Self::Reduce(_) => Vec::new(),
            Self::Unary(_, a) => vec![a],
            Self::Binary(_, a, b) => vec![a, b],
            Self::Ternary(_, a, b, c) | Self::Select(a, b, c) => vec![a, b, c],
            Self::Sample { time, pixel, .. } => match pixel.index() {
                Some(&index) => vec![time, index],
                None => vec![time],
            },
            Self::Items(ref items) => items.to_vec(),
            Self::Pick { index, ref items } => {
                let mut operands = vec![index];
                operands.extend_from_slice(items);
                operands
            }
        }
    }
}

#[derive(Clone, Debug, Hash)]
pub(crate) struct Loop {
    pub(crate) reducer: Reducer,
    pub(crate) start: Node,
    /// Exclusive.
    pub(crate) end: Node,
    pub(crate) index: Node,
    /// The contribution; a predicate for `any` and `all`.
    pub(crate) body: Node,
    /// Iterations whose filter is false contribute nothing.
    pub(crate) filter: Option<Node>,
    /// Result of `first` and `last` when no iteration matches.
    pub(crate) default: Option<Node>,
}

impl Loop {
    /// The loop's parts, which its index may reach.
    pub(crate) fn parts(&self) -> impl Iterator<Item = Node> + '_ {
        [Some(self.body), self.filter, self.default]
            .into_iter()
            .flatten()
    }
}

#[derive(Clone, Debug)]
struct Data {
    op: Op,
    ty: Type,
    domain: Domain,
    loops: LoopSet,
}

/// A parameter leaf: its type and the domain its value varies in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Param {
    pub(crate) ty: Type,
    pub(crate) domain: Domain,
}

#[derive(Clone, Debug)]
pub(crate) struct Graph {
    nodes: Vec<Data>,
    interned: HashMap<Op, Node>,
    loops: Vec<Loop>,
    params: Vec<Param>,
    inputs: u32,
}

/// More than [`LoopSet::LIMIT`] reductions in one graph.
#[derive(Debug)]
pub(crate) struct TooManyLoops;

impl Graph {
    pub(crate) fn new(params: Vec<Param>, inputs: u32) -> Self {
        Self {
            nodes: Vec::new(),
            interned: HashMap::new(),
            loops: Vec::new(),
            params,
            inputs,
        }
    }

    pub(crate) fn op(&self, node: Node) -> &Op {
        &self.nodes[node.index()].op
    }
    pub(crate) fn ty(&self, node: Node) -> &Type {
        &self.nodes[node.index()].ty
    }
    pub(crate) fn domain(&self, node: Node) -> Domain {
        self.nodes[node.index()].domain
    }
    pub(crate) fn loops_of(&self, node: Node) -> LoopSet {
        self.nodes[node.index()].loops
    }
    pub(crate) fn loop_(&self, id: LoopId) -> &Loop {
        &self.loops[id.index()]
    }
    pub(crate) fn loop_count(&self) -> usize {
        self.loops.len()
    }
    pub(crate) fn params(&self) -> &[Param] {
        &self.params
    }
    pub(crate) fn inputs(&self) -> u32 {
        self.inputs
    }
    /// A digest of the graph reachable from `root`, stable across runs.
    pub(crate) fn fingerprint(&self, root: Node) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for data in &self.nodes {
            data.op.hash(&mut hasher);
            data.ty.hash(&mut hasher);
        }
        self.loops.hash(&mut hasher);
        for param in &self.params {
            param.ty.hash(&mut hasher);
            param.domain.hash(&mut hasher);
        }
        (self.inputs, root).hash(&mut hasher);
        hasher.finish()
    }
    /// Every node, operands before their users.
    pub(crate) fn nodes(&self) -> impl Iterator<Item = Node> + use<> {
        (0..self.nodes.len() as u32).map(Node)
    }

    pub(crate) fn constant_value(&self, node: Node) -> Option<&Value> {
        match self.op(node) {
            Op::Constant(constant) => Some(&constant.value),
            _ => None,
        }
    }

    pub(crate) fn constant(&mut self, value: Value) -> Node {
        let ty = value_type(&value);
        self.typed_constant(value, ty)
    }
    pub(crate) fn typed_constant(&mut self, value: Value, ty: Type) -> Node {
        self.add(Op::Constant(Constant { value, ty }))
    }
    pub(crate) fn float(&mut self, value: f32) -> Node {
        self.constant(Value::Float(value))
    }
    pub(crate) fn int(&mut self, value: i32) -> Node {
        self.constant(Value::Int(value))
    }
    pub(crate) fn bool(&mut self, value: bool) -> Node {
        self.constant(Value::Bool(value))
    }
    pub(crate) fn color(&mut self, value: Color) -> Node {
        self.constant(Value::Color(value))
    }
    pub(crate) fn unary(&mut self, op: Unary, a: Node) -> Node {
        self.add(Op::Unary(op, a))
    }
    pub(crate) fn binary(&mut self, op: Binary, a: Node, b: Node) -> Node {
        self.add(Op::Binary(op, a, b))
    }
    pub(crate) fn ternary(&mut self, op: Ternary, a: Node, b: Node, c: Node) -> Node {
        self.add(Op::Ternary(op, a, b, c))
    }
    pub(crate) fn select(&mut self, condition: Node, yes: Node, no: Node) -> Node {
        self.add(Op::Select(condition, yes, no))
    }
    /// `a && b`: a choice, so `b` stays lazy where scheduling branches.
    pub(crate) fn and(&mut self, a: Node, b: Node) -> Node {
        let no = self.bool(false);
        self.select(a, b, no)
    }
    /// `a || b`.
    pub(crate) fn or(&mut self, a: Node, b: Node) -> Node {
        let yes = self.bool(true);
        self.select(a, yes, b)
    }

    /// The node for `op`, after folding; an equal operation is reused.
    pub(crate) fn add(&mut self, op: Op) -> Node {
        let op = fold::canonical(self, op);
        if let Some(&node) = self.interned.get(&op) {
            return node;
        }
        let ty = self.describe(&op).0;
        if let Some(node) = fold::simplify(self, &op, &ty) {
            return node;
        }
        self.intern(op)
    }

    fn intern(&mut self, op: Op) -> Node {
        if let Some(&node) = self.interned.get(&op) {
            return node;
        }
        let (ty, mut domain, mut loops) = self.describe(&op);
        for operand in op.operands() {
            domain = domain.union(self.domain(operand));
            loops = loops.union(self.loops_of(operand));
        }
        let node = Node(self.nodes.len() as u32);
        self.nodes.push(Data {
            op: op.clone(),
            ty,
            domain,
            loops,
        });
        self.interned.insert(op, node);
        node
    }

    /// Type, intrinsic domain and loops of an operation, before its operands.
    fn describe(&self, op: &Op) -> (Type, Domain, LoopSet) {
        let none = (Domain::CONSTANT, LoopSet::default());
        let (ty, (domain, loops)) = match op {
            Op::Constant(constant) => (constant.ty.clone(), none),
            Op::Param(index) => {
                let param = &self.params[*index as usize];
                (param.ty.clone(), (param.domain, LoopSet::default()))
            }
            Op::ParamIntegral(_) => (Type::Float, (Domain::TIME, LoopSet::default())),
            Op::Context(context) => (context.ty(), (context.domain(), LoopSet::default())),
            Op::LoopIndex(id) => (Type::Int, (Domain::CONSTANT, LoopSet::single(*id))),
            Op::Reduce(id) => {
                let data = &self.loops[id.index()];
                let mut domain = self.domain(data.start).union(self.domain(data.end));
                let mut loops = self.loops_of(data.start).union(self.loops_of(data.end));
                for part in data.parts() {
                    domain = domain.union(self.domain(part));
                    loops = loops.union(self.loops_of(part));
                }
                (self.ty(data.body).clone(), (domain, loops.without(*id)))
            }
            Op::Unary(op, _) => (unary_type(*op), none),
            Op::Binary(op, a, _) => (binary_type(*op, self.ty(*a)), none),
            Op::Ternary(op, ..) => (ternary_type(*op), none),
            Op::Select(_, a, _) => (self.ty(*a).clone(), none),
            Op::Items(items) => (Type::array(self.ty(items[0]).clone()), none),
            Op::Pick { items, .. } => (self.ty(items[0]).clone(), none),
            Op::Sample { .. } => (
                Type::Color,
                (Domain::SIGNAL.union(Domain::PIXEL), LoopSet::default()),
            ),
        };
        // Sections are counted per pixel: per-fixture sections differ by fixture.
        let domain = match op {
            Op::Unary(Unary::SectionCount | Unary::SectionIndex, _)
            | Op::Binary(Binary::SectionPosition, ..) => domain.union(Domain::PIXEL),
            _ => domain,
        };
        (ty, domain, loops)
    }

    /// Start a reduction over `start..end`; its index is in scope for the body.
    pub(crate) fn begin_loop(
        &mut self,
        start: Node,
        end: Node,
    ) -> Result<(LoopId, Node), TooManyLoops> {
        if self.loops.len() >= LoopSet::LIMIT {
            return Err(TooManyLoops);
        }
        let id = LoopId(self.loops.len() as u32);
        // A placeholder until the body exists; only `finish_loop` reads it.
        self.loops.push(Loop {
            reducer: Reducer::Max,
            start,
            end,
            index: start,
            body: start,
            filter: None,
            default: None,
        });
        let index = self.intern(Op::LoopIndex(id));
        // The index ranges over the bounds, so it varies wherever they do.
        let bounds = self.domain(start).union(self.domain(end));
        let bound_loops = self.loops_of(start).union(self.loops_of(end));
        let data = &mut self.nodes[index.index()];
        data.domain = data.domain.union(bounds);
        data.loops = data.loops.union(bound_loops);
        self.loops[id.index()].index = index;
        Ok((id, index))
    }

    pub(crate) fn finish_loop(
        &mut self,
        id: LoopId,
        reducer: Reducer,
        body: Node,
        filter: Option<Node>,
        default: Option<Node>,
    ) -> Node {
        let data = &mut self.loops[id.index()];
        data.reducer = reducer;
        data.body = body;
        data.filter = filter;
        data.default = default;
        if let Some(node) = fold::reduction(self, id) {
            return node;
        }
        self.intern(Op::Reduce(id))
    }
}

pub(crate) fn value_type(value: &Value) -> Type {
    match value {
        Value::Void => Type::Void,
        Value::Int(_) => Type::Int,
        Value::Float(_) => Type::Float,
        Value::Bool(_) => Type::Bool,
        Value::Color(_) => Type::Color,
        Value::Marks(_) => Type::Marks,
        Value::Curve(_) => Type::Curve,
        Value::Gradient(_) => Type::Gradient,
        Value::Array(items) => Type::array(items.first().map_or(Type::Void, value_type)),
        Value::Enum(name) => Type::Enum(vec![name.clone()]),
    }
}

fn unary_type(op: Unary) -> Type {
    match op {
        Unary::IntNegate
        | Unary::FloatToInt
        | Unary::Len
        | Unary::MarkCount
        | Unary::SectionCount
        | Unary::SectionIndex => Type::Int,
        Unary::Not => Type::Bool,
        Unary::Invert => Type::Color,
        Unary::Negate
        | Unary::Sin
        | Unary::Cos
        | Unary::Tan
        | Unary::Exp
        | Unary::Log
        | Unary::Abs
        | Unary::Floor
        | Unary::Ceil
        | Unary::Trunc
        | Unary::RoundEven
        | Unary::Sqrt
        | Unary::IntToFloat
        | Unary::Smoothstep
        | Unary::Rand
        | Unary::Hue
        | Unary::Red
        | Unary::Green
        | Unary::Blue
        | Unary::Saturation
        | Unary::Intensity
        | Unary::QuerySeconds
        | Unary::QueryProgress => Type::Float,
    }
}

fn binary_type(op: Binary, left: &Type) -> Type {
    match op {
        Binary::Add
        | Binary::Subtract
        | Binary::Multiply
        | Binary::Divide
        | Binary::Remainder
        | Binary::Min
        | Binary::Max
        | Binary::ValueOr
        | Binary::Atan2
        | Binary::CurveSample
        | Binary::CurveIntegral
        | Binary::MarkAt
        | Binary::MarkLast
        | Binary::CurveFirstCrossing
        | Binary::SectionPosition
        | Binary::Power
        | Binary::PowerFloat => Type::Float,
        Binary::IntAdd
        | Binary::IntSubtract
        | Binary::IntMultiply
        | Binary::IntRemainder
        | Binary::IntFloorDivide
        | Binary::IntMin
        | Binary::IntMax
        | Binary::MarkLastIndex => Type::Int,
        Binary::Less
        | Binary::LessEqual
        | Binary::Greater
        | Binary::GreaterEqual
        | Binary::IntLess
        | Binary::IntLessEqual
        | Binary::IntGreater
        | Binary::IntGreaterEqual
        | Binary::Equal
        | Binary::NotEqual => Type::Bool,
        Binary::ColorAdd
        | Binary::ColorMultiply
        | Binary::ColorMax
        | Binary::ColorScale
        | Binary::GradientSample => Type::Color,
        Binary::Index => match left {
            Type::Array(item) => item.as_ref().clone(),
            _ => Type::Void,
        },
    }
}

fn ternary_type(op: Ternary) -> Type {
    match op {
        Ternary::Clamp | Ternary::Mix | Ternary::CurveLastCrossing => Type::Float,
        Ternary::MixColor | Ternary::Rgb | Ternary::Hsv => Type::Color,
    }
}

/// Bitwise equality of constants and resources.
pub(crate) fn same_value(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Void, Value::Void) => true,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Color(a), Value::Color(b)) => a == b,
        (Value::Enum(a), Value::Enum(b)) => a == b,
        (Value::Marks(a), Value::Marks(b)) => {
            a.as_slice().len() == b.as_slice().len()
                && a.as_slice().iter().zip(b.as_slice()).all(|(a, b)| a == b)
        }
        (Value::Curve(a), Value::Curve(b)) => {
            a.points.len() == b.points.len()
                && a.points.iter().zip(&b.points).all(|(a, b)| {
                    a.position.to_bits() == b.position.to_bits()
                        && a.value.to_bits() == b.value.to_bits()
                })
        }
        (Value::Gradient(a), Value::Gradient(b)) => {
            a.stops.len() == b.stops.len()
                && a.stops.iter().zip(&b.stops).all(|(a, b)| {
                    a.position.to_bits() == b.position.to_bits() && a.color == b.color
                })
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| same_value(a, b))
        }
        _ => false,
    }
}

pub(crate) fn hash_value<H: Hasher>(value: &Value, state: &mut H) {
    core::mem::discriminant(value).hash(state);
    match value {
        Value::Void => {}
        Value::Int(value) => value.hash(state),
        Value::Float(value) => value.to_bits().hash(state),
        Value::Bool(value) => value.hash(state),
        Value::Color(value) => value.hash(state),
        Value::Enum(value) => value.hash(state),
        Value::Marks(value) => {
            for mark in value.as_slice() {
                mark.as_ticks().hash(state);
            }
        }
        Value::Curve(value) => {
            for point in &value.points {
                point.position.to_bits().hash(state);
                point.value.to_bits().hash(state);
            }
        }
        Value::Gradient(value) => {
            for stop in &value.stops {
                stop.position.to_bits().hash(state);
                stop.color.hash(state);
            }
        }
        Value::Array(items) => {
            items.len().hash(state);
            for item in items.iter() {
                hash_value(item, state);
            }
        }
    }
}

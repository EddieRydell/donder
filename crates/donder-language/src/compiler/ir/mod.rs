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
mod stencil;

pub(crate) use eval::{Evaluator, evaluate, reduce_identity};
pub(crate) use rebuild::{CurveQuery, Rebuild, Substitute};
pub(crate) use stencil::{StencilError, reads, scan_weight};

use donder_runtime_types::Color;
use donder_runtime_types::bytecode::{Direction, Edges, SignalPixel};
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
    const CONSTANT: Self = Self(0);
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
    /// Its loop, when it holds exactly one.
    pub(crate) fn only(self) -> Option<LoopId> {
        (self.0.count_ones() == 1).then(|| LoopId(self.0.trailing_zeros()))
    }
    pub(crate) fn innermost(self) -> Option<LoopId> {
        (self.0 != 0).then(|| LoopId(63 - self.0.leading_zeros()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Context {
    Time,
    Duration,
    FrameRate,
    PixelIndex,
    PixelX,
    PixelY,
    PixelZ,
    PixelMember,
    PixelStrand,
    PixelAlong,
    TargetCount,
    TargetMinX,
    TargetMinY,
    TargetMaxX,
    TargetMaxY,
    TargetMinZ,
    TargetMaxZ,
    TargetMembers,
    TargetStrands,
    TargetStrandLength,
    /// The show's seconds at which `time` is zero: an invocation's start.
    /// Automation reads it; no script names it.
    Origin,
}

impl Context {
    fn ty(self) -> Type {
        match self {
            Self::Origin => Type::Float,
            _ => crate::compiler::builtins::context_type(self),
        }
    }

    fn domain(self) -> Domain {
        match self {
            Self::Time | Self::Origin => Domain::TIME,
            Self::Duration | Self::FrameRate => Domain::PARAM,
            Self::PixelIndex
            | Self::PixelX
            | Self::PixelY
            | Self::PixelZ
            | Self::PixelMember
            | Self::PixelStrand
            | Self::PixelAlong => Domain::PIXEL,
            Self::TargetCount
            | Self::TargetMinX
            | Self::TargetMinY
            | Self::TargetMaxX
            | Self::TargetMaxY
            | Self::TargetMinZ
            | Self::TargetMaxZ
            | Self::TargetMembers
            | Self::TargetStrands
            | Self::TargetStrandLength => Domain::TARGET,
        }
    }
}

/// Operations of one operand. The operand's type chooses the semantics: `Negate`
/// and `Abs` of an int wrap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Unary {
    Negate,
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
    /// A font's line height.
    FontHeight,
    /// An image's width, height and frames a second.
    ImageWidth,
    ImageHeight,
    ImageRate,
    ImageFrames,
    /// The live store word an int names: a console value playback sets.
    Live,
}

/// Operations of two operands. The operands' types choose the semantics:
/// int arithmetic wraps, an int `Divide` is the floored quotient with
/// division by zero giving zero, color arithmetic works per channel, and a
/// color `Multiply` by a float scales it. `Power` raises a float to an int
/// count of multiplications or to a float exponent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Binary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Min,
    Max,
    ValueOr,
    Atan2,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    /// Same-typed scalar equality; floats compare by IEEE rules.
    Equal,
    NotEqual,
    CurveSample,
    CurveIntegral,
    GradientSample,
    Index,
    /// A table's entry at an index within its length.
    Entry,
    /// How far a character moves the pen in a font.
    GlyphAdvance,
    MarkAt,
    MarkDuration,
    MarkLastIndex,
    Power,
}

impl Binary {
    fn commutative(self) -> bool {
        matches!(
            self,
            Self::Add | Self::Multiply | Self::Min | Self::Max | Self::Equal | Self::NotEqual
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Ternary {
    Clamp,
    /// Of floats, or of colors by a float.
    Mix,
    Rgb,
    Hsv,
    /// The first position at or after a position where a curve reaches a value.
    CurveFirstCrossing,
    CurveLastCrossing,
    /// A track's channel at a time.
    TrackAt,
    /// A character of a mark's label.
    MarkText,
    /// How many of an ascending table's first entries are at most a value.
    Partition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Quaternary {
    /// A character's coverage in a font at a point.
    GlyphCoverage,
    /// An image frame's color at a point.
    ImageColor,
}

/// A reduction over a range of indexes; its keyword and summary are in
/// `builtins.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reducer {
    Max,
    Min,
    Sum,
    Any,
    All,
    First,
    Last,
    /// An array of the body at each index, computed once per query or
    /// target run.
    Table,
    /// `table sum`: an array one entry longer than the range; entry `k` sums
    /// the body at the first `k` indices.
    RunningSum,
}

impl Reducer {
    /// Whether the reduction makes an array rather than one value.
    pub(crate) fn tabulates(self) -> bool {
        matches!(self, Self::Table | Self::RunningSum)
    }
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
        self.ty == other.ty && self.value.same(&other.value)
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
    Quaternary(Quaternary, Node, Node, Node, Node),
    /// Pure choice. Whether an arm is evaluated lazily is a scheduling decision.
    Select(Node, Node, Node),
    Reduce(LoopId),
    /// An operator input at this frame and a pixel.
    Read {
        input: u32,
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
    /// A stencil's or scan's input sample at the pixel its weight is
    /// computed for.
    Neighbor,
    /// A trail along each fixture: `decay` (fixed for the query) times the
    /// previous pixel's value, plus the input at the current pixel times
    /// `weight`, which reads that sample as [`Op::Neighbor`] and otherwise
    /// values fixed for the query.
    Scan {
        direction: Direction,
        input: u32,
        decay: Node,
        weight: Option<Node>,
    },
    /// The contribution of a neighborhood reduction: the input at the loop's
    /// offset along the pixel's fixture, scaled by the product of the
    /// present factors. `weight` depends on that sample, as [`Op::Neighbor`],
    /// alone; `scale` on the offset but not the pixel; `pixel` not on the
    /// offset.
    Tap {
        id: LoopId,
        input: u32,
        edges: Edges,
        weight: Option<Node>,
        scale: Option<Node>,
        pixel: Option<Node>,
    },
}

impl Op {
    /// The operator input that a sample, scan or stencil reads.
    pub(crate) fn input(&self) -> Option<u32> {
        match *self {
            Self::Read { input, .. } | Self::Scan { input, .. } | Self::Tap { input, .. } => {
                Some(input)
            }
            _ => None,
        }
    }

    /// Direct operands, in a stable order. A reduction's parts live in its loop.
    pub(crate) fn operands(&self) -> Vec<Node> {
        match *self {
            Self::Constant(_)
            | Self::Param(_)
            | Self::ParamIntegral(_)
            | Self::Context(_)
            | Self::LoopIndex(_)
            | Self::Reduce(_)
            | Self::Neighbor => Vec::new(),
            Self::Scan { decay, weight, .. } => {
                [Some(decay), weight].into_iter().flatten().collect()
            }
            Self::Unary(_, a) => vec![a],
            Self::Binary(_, a, b) => vec![a, b],
            Self::Ternary(_, a, b, c) | Self::Select(a, b, c) => vec![a, b, c],
            Self::Quaternary(_, a, b, c, d) => vec![a, b, c, d],
            Self::Read { pixel, .. } => pixel.index().copied().into_iter().collect(),
            Self::Items(ref items) => items.to_vec(),
            Self::Pick { index, ref items } => {
                let mut operands = vec![index];
                operands.extend_from_slice(items);
                operands
            }
            Self::Tap {
                weight,
                scale,
                pixel,
                ..
            } => [weight, scale, pixel].into_iter().flatten().collect(),
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
    /// Every pixel shares the index: the loop runs the union of the pixels'
    /// ranges, each pixel taking part in its own, so work that depends only
    /// on the index runs once per iteration rather than once per pixel.
    shared: bool,
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
    interval: interval::Interval,
}

/// A parameter leaf: its type, the domain its value varies in, and the range
/// its declaration promises every value lies in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Param {
    pub(crate) ty: Type,
    pub(crate) domain: Domain,
    pub(crate) range: Option<(f64, f64)>,
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

    /// A parameter slot after the declared ones, its index.
    pub(crate) fn add_param(&mut self, param: Param) -> u32 {
        self.params.push(param);
        self.params.len() as u32 - 1
    }

    pub(crate) fn op(&self, node: Node) -> &Op {
        &self.nodes[node.index()].op
    }

    /// The range `node`'s value lies in, from the declared parameter ranges.
    pub(crate) fn interval(&self, node: Node) -> interval::Interval {
        self.nodes[node.index()].interval
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
    fn loop_count(&self) -> usize {
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

    fn constant(&mut self, value: Value) -> Node {
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
    pub(crate) fn quaternary(
        &mut self,
        op: Quaternary,
        a: Node,
        b: Node,
        c: Node,
        d: Node,
    ) -> Node {
        self.add(Op::Quaternary(op, a, b, c, d))
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
        let interval = interval::recorded(self, &op);
        let node = Node(self.nodes.len() as u32);
        self.nodes.push(Data {
            op: op.clone(),
            ty,
            domain,
            loops,
            interval,
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
                let body = self.ty(data.body).clone();
                let ty = if data.reducer.tabulates() {
                    Type::array(body)
                } else {
                    body
                };
                (ty, (domain, loops.without(*id)))
            }
            Op::Unary(Unary::Live, _) => (Type::Float, (Domain::TIME, LoopSet::default())),
            Op::Unary(op, a) => (unary_type(*op, self.ty(*a)), none),
            Op::Binary(op, a, _) => (binary_type(*op, self.ty(*a)), none),
            Op::Ternary(op, a, ..) => (ternary_type(*op, self.ty(*a)), none),
            Op::Quaternary(Quaternary::GlyphCoverage, ..) => (Type::Float, none),
            Op::Quaternary(Quaternary::ImageColor, ..) => (Type::Color, none),
            Op::Select(_, a, _) => (self.ty(*a).clone(), none),
            Op::Items(items) => (Type::array(self.ty(items[0]).clone()), none),
            Op::Pick { items, .. } => (self.ty(items[0]).clone(), none),
            Op::Read { .. } => (
                Type::Color,
                (Domain::SIGNAL.union(Domain::PIXEL), LoopSet::default()),
            ),
            Op::Neighbor => (Type::Color, (Domain::SIGNAL, LoopSet::default())),
            Op::Scan { .. } => (
                Type::Color,
                (Domain::SIGNAL.union(Domain::PIXEL), LoopSet::default()),
            ),
            Op::Tap { id, .. } => (
                Type::Color,
                (Domain::SIGNAL.union(Domain::PIXEL), LoopSet::single(*id)),
            ),
        };
        (ty, domain, loops)
    }

    /// Start a reduction over `start..end`; its index is in scope for the body.
    /// A `shared` index varies only with the loop, wherever the bounds vary.
    pub(crate) fn begin_loop(
        &mut self,
        start: Node,
        end: Node,
        shared: bool,
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
            shared,
        });
        let index = self.intern(Op::LoopIndex(id));
        // The index ranges over the bounds, so it varies wherever they do,
        // unless every pixel shares it.
        let bounds = if shared {
            Domain::CONSTANT
        } else {
            self.domain(start).union(self.domain(end))
        };
        let bound_loops = self.loops_of(start).union(self.loops_of(end));
        let data = &mut self.nodes[index.index()];
        data.domain = data.domain.union(bounds);
        data.loops = data.loops.union(bound_loops);
        self.loops[id.index()].index = index;
        Ok((id, index))
    }

    /// The tap replacing the contribution of `around` reduction `id`, or why
    /// it is not a stencil.
    pub(crate) fn tap(
        &mut self,
        id: LoopId,
        reducer: Reducer,
        body: Node,
        filter: Option<Node>,
    ) -> Result<Node, StencilError> {
        stencil::tap(self, id, reducer, body, filter)
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

fn value_type(value: &Value) -> Type {
    match value {
        Value::Void => Type::Void,
        Value::Int(_) => Type::Int,
        Value::Float(_) => Type::Float,
        Value::Bool(_) => Type::Bool,
        Value::Color(_) => Type::Color,
        Value::Marks(_) => Type::Marks,
        Value::Track(_) => Type::Track,
        Value::Font(_) => Type::Font,
        Value::Image(_) => Type::Image,
        Value::Curve(_) => Type::Curve,
        Value::Gradient(_) => Type::Gradient,
        Value::Array(items) => Type::array(items.first().map_or(Type::Void, value_type)),
        Value::Enum(name) => Type::Enum(vec![name.clone()]),
    }
}

fn unary_type(op: Unary, operand: &Type) -> Type {
    match op {
        Unary::Negate | Unary::Abs => operand.clone(),
        Unary::FloatToInt | Unary::Len | Unary::ImageFrames => Type::Int,
        Unary::Not => Type::Bool,
        Unary::FontHeight | Unary::ImageWidth | Unary::ImageHeight | Unary::ImageRate => {
            Type::Float
        }
        Unary::Invert => Type::Color,
        Unary::Sin
        | Unary::Cos
        | Unary::Tan
        | Unary::Exp
        | Unary::Log
        | Unary::Floor
        | Unary::Ceil
        | Unary::Trunc
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
        | Unary::Live => Type::Float,
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
        | Binary::Max => left.clone(),
        Binary::ValueOr
        | Binary::Atan2
        | Binary::CurveSample
        | Binary::CurveIntegral
        | Binary::MarkAt
        | Binary::MarkDuration
        | Binary::GlyphAdvance
        | Binary::Power => Type::Float,
        Binary::MarkLastIndex => Type::Int,
        Binary::Less
        | Binary::LessEqual
        | Binary::Greater
        | Binary::GreaterEqual
        | Binary::Equal
        | Binary::NotEqual => Type::Bool,
        Binary::GradientSample => Type::Color,
        Binary::Index | Binary::Entry => match left {
            Type::Array(item) => item.as_ref().clone(),
            Type::Text => Type::Int,
            _ => Type::Void,
        },
    }
}

fn ternary_type(op: Ternary, first: &Type) -> Type {
    match op {
        Ternary::Mix => first.clone(),
        Ternary::Clamp
        | Ternary::CurveFirstCrossing
        | Ternary::CurveLastCrossing
        | Ternary::TrackAt => Type::Float,
        Ternary::MarkText | Ternary::Partition => Type::Int,
        Ternary::Rgb | Ternary::Hsv => Type::Color,
    }
}

fn hash_value<H: Hasher>(value: &Value, state: &mut H) {
    core::mem::discriminant(value).hash(state);
    match value {
        Value::Void => {}
        Value::Int(value) => value.hash(state),
        Value::Float(value) => value.to_bits().hash(state),
        Value::Bool(value) => value.hash(state),
        Value::Color(value) => value.hash(state),
        Value::Enum(value) => value.hash(state),
        Value::Marks(value) => value.hash(state),
        Value::Track(value) => value.hash(state),
        Value::Font(value) => value.hash(state),
        Value::Image(value) => value.hash(state),
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

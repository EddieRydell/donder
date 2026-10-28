//! Stencils and scan weights, formed when the body is checked.
//!
//! An `around` reduction sums, or takes the max of, its input's neighbors
//! along the pixel's fixture, each scaled by factors that depend on the
//! neighbor alone, on the offset alone or on the pixel alone. It becomes a
//! tap: playback computes each neighbor's factors once, not once per offset
//! that reads it, and accumulates every offset in one instruction.
//!
//! A contribution is classified after flattening: the scales and guards
//! around the sample, then every product and quotient of the scales, so how
//! the author grouped or ordered the factors never decides whether it is a
//! stencil. `neighbor * a * b`, `a * (neighbor * b)` and `neighbor * (a * b)`
//! are the same stencil, and `if c { neighbor * w } else { black }` is the
//! guard `c`. Factor products reassociate, which only changes rounding.
//!
//! A scan adds its input at each pixel scaled by a weight: the same flattened
//! scales and guards, which may read the input's sample and values fixed for
//! the frame.
use super::{Binary, Domain, Graph, LoopId, Node, Op, Reducer};
use donder_runtime_types::bytecode::SignalPixel;
use donder_runtime_types::{Color, Type, Value};
use std::collections::HashMap;

/// Why a reduction's contribution is not a stencil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StencilError {
    /// Only sums and maxes of colors accumulate neighbors.
    Reducer,
    /// The contribution is not the neighbor, optionally scaled.
    Shape,
    /// A factor or condition depends on the neighbor and on something else
    /// that varies, or is not arithmetic.
    Neighbor,
    /// A factor or condition depends on the offset and on the pixel.
    Offset,
}

impl StencilError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Reducer => "`around` combines neighbors with `sum` or `max` into a color",
            Self::Shape => {
                "an `around` body produces the neighbor, optionally scaled by numbers: `neighbor * weight`; change its color after the reduction"
            }
            Self::Neighbor => {
                "a weight or guard that reads the neighbor may combine it only with values fixed for the frame; write the offset's part as a separate factor"
            }
            Self::Offset => {
                "a weight or guard that depends on the offset may not also depend on the pixel; write the pixel's part as a separate factor"
            }
        }
    }
}

/// What a factor or condition of the contribution depends on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    /// The neighbor, and values fixed for the query.
    Neighbor,
    /// The offset, and values fixed for the strip.
    Offset,
    /// Not the offset.
    Pixel,
}

/// A factor of the scale: a value, or the reciprocal of a divisor.
type Factor = (Node, bool);

/// The tap replacing `around` reduction `id`'s contribution, or why it is
/// not a stencil.
pub(super) fn tap(
    graph: &mut Graph,
    id: LoopId,
    reducer: Reducer,
    body: Node,
    filter: Option<Node>,
) -> Result<Node, StencilError> {
    if !matches!(reducer, Reducer::Sum | Reducer::Max) || *graph.ty(body) != Type::Color {
        return Err(StencilError::Reducer);
    }
    let mut scales = Vec::new();
    let mut conditions = Vec::new();
    if let Some(filter) = filter {
        conjuncts(graph, filter, &mut conditions);
    }
    let sample = scaled(graph, body, &mut scales, &mut conditions);
    let Op::Read {
        input,
        pixel: SignalPixel::Shifted(offset, edges),
    } = *graph.op(sample)
    else {
        return Err(StencilError::Shape);
    };
    if offset != graph.loop_(id).index {
        return Err(StencilError::Shape);
    }
    let mut factors = Vec::new();
    for scale in scales {
        product(graph, scale, false, &mut factors);
    }
    let mut reads = Reads {
        sample,
        own: Some(id),
        memo: HashMap::new(),
    };
    let mut parts: [(Vec<Factor>, Vec<Node>); 3] = Default::default();
    for (node, inverse) in factors {
        let part = part(graph, &mut reads, id, node)?;
        parts[part as usize].0.push((node, inverse));
    }
    for node in conditions {
        let part = part(graph, &mut reads, id, node)?;
        parts[part as usize].1.push(node);
    }
    let [neighbor, offset, pixel] =
        parts.map(|(factors, conditions)| assemble(graph, factors, conditions));
    let weight = neighbor.map(|node| weight_of(graph, node, sample, &mut HashMap::new()));
    Ok(graph.add(Op::Tap {
        id,
        input,
        edges,
        weight,
        scale: offset,
        pixel,
    }))
}

/// The weight a scan adds `light`, a color computed from `sample`, with:
/// `None` for the sample itself, or `None` inside when `light` is not the
/// sample scaled by numbers and guarded by conditions that read only it and
/// values fixed for the frame.
pub(crate) fn scan_weight(graph: &mut Graph, light: Node, sample: Node) -> Option<Option<Node>> {
    let mut scales = Vec::new();
    let mut conditions = Vec::new();
    if scaled(graph, light, &mut scales, &mut conditions) != sample {
        return None;
    }
    let mut reads = Reads {
        sample,
        own: None,
        memo: HashMap::new(),
    };
    if !scales
        .iter()
        .chain(&conditions)
        .all(|&node| reads.source_only(graph, node))
    {
        return None;
    }
    let factors = scales.into_iter().map(|scale| (scale, false)).collect();
    Some(
        assemble(graph, factors, conditions)
            .map(|weight| weight_of(graph, weight, sample, &mut HashMap::new())),
    )
}

/// Whether `node` reads `sample`.
pub(crate) fn reads(graph: &Graph, node: Node, sample: Node) -> bool {
    Reads {
        sample,
        own: None,
        memo: HashMap::new(),
    }
    .of(graph, node)
}

/// The color that `node` scales and guards: its scales, outermost last, are
/// added to `scales` and its guards to `conditions`.
fn scaled(graph: &Graph, node: Node, scales: &mut Vec<Node>, conditions: &mut Vec<Node>) -> Node {
    match *graph.op(node) {
        Op::Binary(Binary::ColorScale, color, scale) => {
            let color = scaled(graph, color, scales, conditions);
            scales.push(scale);
            color
        }
        Op::Select(condition, yes, no)
            if graph.constant_value(no) == Some(&Value::Color(Color::BLACK)) =>
        {
            conjuncts(graph, condition, conditions);
            scaled(graph, yes, scales, conditions)
        }
        _ => node,
    }
}

/// The product of `factors`, chosen where every condition holds and zero
/// elsewhere; `None` for no factors and no conditions.
fn assemble(graph: &mut Graph, factors: Vec<Factor>, conditions: Vec<Node>) -> Option<Node> {
    let mut value = None;
    for (node, inverse) in factors {
        let factor = if inverse {
            let one = graph.float(1.0);
            graph.binary(Binary::Divide, one, node)
        } else {
            node
        };
        value = Some(match value {
            Some(value) => graph.binary(Binary::Multiply, value, factor),
            None => factor,
        });
    }
    let condition = conditions.into_iter().reduce(|a, b| graph.and(a, b));
    match condition {
        Some(condition) => {
            let (one, zero) = (graph.float(1.0), graph.float(0.0));
            Some(graph.select(condition, value.unwrap_or(one), zero))
        }
        None => value,
    }
}

/// The same at every pixel. Without `own`, also fixed for the query:
/// computable before the strips. With it, a stencil's value may change
/// between iterations of an enclosing reduction, though not with the
/// stencil's own offset: the stencil runs again in each iteration.
fn uniform(graph: &Graph, node: Node, own: Option<LoopId>) -> bool {
    let loops = graph.loops_of(node);
    own.map_or(loops.is_empty(), |id| !loops.contains(id))
        && !graph
            .domain(node)
            .intersects(Domain::PIXEL.union(Domain::SIGNAL).union(Domain::TARGET))
}

/// The factors of a product; a divisor is a reciprocal factor.
fn product(graph: &Graph, node: Node, inverse: bool, factors: &mut Vec<Factor>) {
    match *graph.op(node) {
        Op::Binary(Binary::Multiply, a, b) => {
            product(graph, a, inverse, factors);
            product(graph, b, inverse, factors);
        }
        Op::Binary(Binary::Divide, a, b) => {
            product(graph, a, inverse, factors);
            product(graph, b, !inverse, factors);
        }
        _ => factors.push((node, inverse)),
    }
}

/// The conditions of a conjunction.
fn conjuncts(graph: &Graph, node: Node, conditions: &mut Vec<Node>) {
    match *graph.op(node) {
        Op::Select(a, b, no) if graph.constant_value(no) == Some(&Value::Bool(false)) => {
            conjuncts(graph, a, conditions);
            conjuncts(graph, b, conditions);
        }
        _ => conditions.push(node),
    }
}

fn part(graph: &Graph, reads: &mut Reads, id: LoopId, node: Node) -> Result<Part, StencilError> {
    if reads.of(graph, node) {
        return if reads.source_only(graph, node) {
            Ok(Part::Neighbor)
        } else {
            Err(StencilError::Neighbor)
        };
    }
    if graph.loops_of(node).contains(id) {
        let scalar = !graph
            .domain(node)
            .intersects(Domain::PIXEL.union(Domain::SIGNAL));
        return if scalar {
            Ok(Part::Offset)
        } else {
            Err(StencilError::Offset)
        };
    }
    Ok(Part::Pixel)
}

struct Reads {
    sample: Node,
    /// The stencil whose neighbor this is; none for a scan.
    own: Option<LoopId>,
    memo: HashMap<Node, bool>,
}

impl Reads {
    fn of(&mut self, graph: &Graph, node: Node) -> bool {
        if node == self.sample {
            return true;
        }
        if let Some(&reads) = self.memo.get(&node) {
            return reads;
        }
        // Everything reading a signal sample depends on a signal.
        let reads = graph.domain(node).intersects(Domain::SIGNAL)
            && crate::compiler::lower::structural_operands(graph, node)
                .into_iter()
                .any(|operand| self.of(graph, operand));
        self.memo.insert(node, reads);
        reads
    }

    /// Whether `node` is arithmetic on the sample and query values.
    fn source_only(&mut self, graph: &Graph, node: Node) -> bool {
        if node == self.sample || !self.of(graph, node) {
            return node == self.sample || uniform(graph, node, self.own);
        }
        matches!(
            graph.op(node),
            Op::Unary(..) | Op::Binary(..) | Op::Ternary(..) | Op::Select(..)
        ) && graph
            .op(node)
            .operands()
            .into_iter()
            .all(|operand| self.source_only(graph, operand))
    }
}

/// `node` with the sample replaced by the stencil's or scan's neighbor.
fn weight_of(graph: &mut Graph, node: Node, sample: Node, memo: &mut HashMap<Node, Node>) -> Node {
    if node == sample {
        return graph.add(Op::Neighbor);
    }
    if !reads(graph, node, sample) {
        return node;
    }
    if let Some(&built) = memo.get(&node) {
        return built;
    }
    let built = match graph.op(node).clone() {
        Op::Unary(op, a) => {
            let a = weight_of(graph, a, sample, memo);
            graph.unary(op, a)
        }
        Op::Binary(op, a, b) => {
            let a = weight_of(graph, a, sample, memo);
            let b = weight_of(graph, b, sample, memo);
            graph.binary(op, a, b)
        }
        Op::Ternary(op, a, b, c) => {
            let a = weight_of(graph, a, sample, memo);
            let b = weight_of(graph, b, sample, memo);
            let c = weight_of(graph, c, sample, memo);
            graph.ternary(op, a, b, c)
        }
        Op::Select(condition, yes, no) => {
            let condition = weight_of(graph, condition, sample, memo);
            let yes = weight_of(graph, yes, sample, memo);
            let no = weight_of(graph, no, sample, memo);
            graph.select(condition, yes, no)
        }
        _ => unreachable!("neighbor factors are arithmetic"),
    };
    memo.insert(node, built);
    built
}

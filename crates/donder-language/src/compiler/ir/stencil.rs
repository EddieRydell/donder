//! Neighborhood reductions as stencils. An `around` reduction sums, or takes
//! the max of, its input's neighbors along the pixel's fixture, each scaled by
//! factors that depend on the neighbor alone, on the offset alone or on the
//! pixel alone. It becomes a tap: playback computes each neighbor's factors
//! once, not once per offset that reads it, and accumulates every offset in
//! one instruction. Factor products reassociate, which only changes rounding.
use super::{Binary, Domain, Graph, LoopId, Node, Op, Reducer};
use donder_runtime_types::bytecode::{Edges, SignalPixel};
use donder_runtime_types::{Type, Value};
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
                "an `around` body produces the neighbor, optionally scaled by a number: `neighbor * weight`; change its color after the reduction"
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
    Source,
    /// The offset, and values fixed for the strip.
    Offset,
    /// Not the offset.
    Pixel,
}

/// A factor of the scale: a value, or the reciprocal of a divisor.
type Factor = (Node, bool);

/// A contribution split into its parts.
struct Parts {
    sample: Node,
    input: u32,
    time: Node,
    edges: Edges,
    /// Factors and conditions of each part, by [`Part`].
    parts: [(Vec<Factor>, Vec<Node>); 3],
}

/// Whether reduction `id`'s contribution is a stencil, and if not, why.
pub(crate) fn check(
    graph: &Graph,
    id: LoopId,
    reducer: Reducer,
    body: Node,
    filter: Option<Node>,
) -> Result<(), StencilError> {
    split(graph, id, reducer, body, filter).map(|_| ())
}

/// The tap replacing reduction `id`'s contribution.
pub(super) fn tap(
    graph: &mut Graph,
    id: LoopId,
    reducer: Reducer,
    body: Node,
    filter: Option<Node>,
) -> Option<Node> {
    let Parts {
        sample,
        input,
        time,
        edges,
        parts,
    } = split(graph, id, reducer, body, filter).ok()?;
    let [source, offset, pixel] = parts.map(|(factors, conditions)| {
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
    });
    let weight = source.map(|node| source_of(graph, node, sample, &mut HashMap::new()));
    Some(graph.add(Op::Tap {
        id,
        input,
        time,
        edges,
        weight,
        scale: offset,
        pixel,
    }))
}

/// Whether `node` reads `sample`.
pub(crate) fn reads(graph: &Graph, node: Node, sample: Node) -> bool {
    Reads {
        sample,
        memo: HashMap::new(),
    }
    .of(graph, node)
}

fn split(
    graph: &Graph,
    id: LoopId,
    reducer: Reducer,
    body: Node,
    filter: Option<Node>,
) -> Result<Parts, StencilError> {
    if !matches!(reducer, Reducer::Sum | Reducer::Max) || *graph.ty(body) != Type::Color {
        return Err(StencilError::Reducer);
    }
    let index = graph.loop_(id).index;
    let (sample, scale) = match *graph.op(body) {
        Op::Binary(Binary::ColorScale, color, scale) => (color, Some(scale)),
        _ => (body, None),
    };
    let Op::Sample {
        input,
        time,
        pixel: SignalPixel::Shifted(offset, edges),
    } = *graph.op(sample)
    else {
        return Err(StencilError::Shape);
    };
    if offset != index || !uniform(graph, time) {
        return Err(StencilError::Shape);
    }
    let mut reads = Reads {
        sample,
        memo: HashMap::new(),
    };
    let mut factors = Vec::new();
    if let Some(scale) = scale {
        product(graph, scale, false, &mut factors);
    }
    let mut conditions = Vec::new();
    if let Some(filter) = filter {
        conjuncts(graph, filter, &mut conditions);
    }
    let mut parts: [(Vec<Factor>, Vec<Node>); 3] = Default::default();
    for &(node, inverse) in &factors {
        let part = part(graph, &mut reads, id, node)?;
        parts[part as usize].0.push((node, inverse));
    }
    for &node in &conditions {
        let part = part(graph, &mut reads, id, node)?;
        parts[part as usize].1.push(node);
    }
    Ok(Parts {
        sample,
        input,
        time,
        edges,
        parts,
    })
}

/// Fixed for the query: computable before the strips.
fn uniform(graph: &Graph, node: Node) -> bool {
    graph.loops_of(node).is_empty()
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
            Ok(Part::Source)
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
            return node == self.sample || uniform(graph, node);
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

/// Whether `node` is arithmetic on `sample` and query values.
pub(crate) fn source_only(graph: &Graph, node: Node, sample: Node) -> bool {
    Reads {
        sample,
        memo: HashMap::new(),
    }
    .source_only(graph, node)
}

/// `node` with the sample replaced by the stencil's source.
pub(crate) fn source_of(
    graph: &mut Graph,
    node: Node,
    sample: Node,
    memo: &mut HashMap<Node, Node>,
) -> Node {
    if node == sample {
        return graph.add(Op::Source);
    }
    if !reads(graph, node, sample) {
        return node;
    }
    if let Some(&built) = memo.get(&node) {
        return built;
    }
    let built = match graph.op(node).clone() {
        Op::Unary(op, a) => {
            let a = source_of(graph, a, sample, memo);
            graph.unary(op, a)
        }
        Op::Binary(op, a, b) => {
            let a = source_of(graph, a, sample, memo);
            let b = source_of(graph, b, sample, memo);
            graph.binary(op, a, b)
        }
        Op::Ternary(op, a, b, c) => {
            let a = source_of(graph, a, sample, memo);
            let b = source_of(graph, b, sample, memo);
            let c = source_of(graph, c, sample, memo);
            graph.ternary(op, a, b, c)
        }
        Op::Select(condition, yes, no) => {
            let condition = source_of(graph, condition, sample, memo);
            let yes = source_of(graph, yes, sample, memo);
            let no = source_of(graph, no, sample, memo);
            graph.select(condition, yes, no)
        }
        _ => unreachable!("source factors are arithmetic"),
    };
    memo.insert(node, built);
    built
}

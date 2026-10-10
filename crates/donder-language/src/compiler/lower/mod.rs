//! Lowering an instance to a portable program. Values fixed for the instance
//! are computed now and become parameter slots; scheduling places every other
//! node in a stage and region; the backend emits strip bytecode.
mod emit;
mod explain;
mod schedule;
mod slots;

use super::instance::Instance;
use super::ir::interval::{Bounds, interval};
use super::ir::{
    Binary, Domain, Evaluator, Graph, Node, Op, Param, Rebuild, Substitute, TooManyLoops,
};
use donder_runtime_types::PreparedAutomation;
use donder_runtime_types::bytecode::BytecodeProgram;
use donder_runtime_types::bytecode::STRIP;
use donder_runtime_types::{Type, Value};
use std::collections::{HashMap, HashSet};

/// A lowered program and the parameter values it reads.
#[derive(Clone, Debug)]
pub(crate) struct Lowered {
    pub(crate) bytecode: BytecodeProgram,
    pub(crate) values: Vec<Value>,
    pub(crate) automation: Vec<PreparedAutomation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LowerError {
    /// Bytes of per-pixel rows the program needs, beyond the limit.
    Rows(u32),
    /// Selections the program holds open at once, beyond the limit.
    Depth(u16),
    /// More slots of one bank, or more code, than the format addresses.
    Slots,
    Code,
    Loops,
}

impl From<TooManyLoops> for LowerError {
    fn from(_: TooManyLoops) -> Self {
        Self::Loops
    }
}

impl Instance {
    /// The prepared graph, its plan and its bytecode, for inspection.
    pub(crate) fn explain(&self) -> Result<String, LowerError> {
        let prepared = prepare(self)?;
        let plan = schedule::schedule(&prepared.graph, prepared.root);
        let bytecode = emit::emit(&prepared.graph, prepared.root, &plan)?;
        Ok(explain::explain(
            &prepared,
            &plan,
            &donder_runtime_types::bytecode::listing(&bytecode),
        ))
    }

    pub(crate) fn lower(&self) -> Result<Lowered, LowerError> {
        let prepared = prepare(self)?;
        let plan = schedule::schedule(&prepared.graph, prepared.root);
        let bytecode = emit::emit(&prepared.graph, prepared.root, &plan)?;
        Ok(Lowered {
            bytecode,
            values: prepared.values,
            automation: prepared.automation,
        })
    }
}

/// An instance whose preparation-time values are computed.
pub(crate) struct Prepared {
    pub(crate) graph: Graph,
    pub(crate) root: Node,
    pub(crate) values: Vec<Value>,
    pub(crate) automation: Vec<PreparedAutomation>,
}

/// Evaluate every value fixed for the instance. Those that a playback-time
/// value reads (the frontier) become parameter slots, after the automated
/// parameters; nothing else fixed survives.
pub(crate) fn prepare(instance: &Instance) -> Result<Prepared, TooManyLoops> {
    let graph = &instance.graph;
    let automated: HashSet<u32> = instance
        .automation
        .iter()
        .map(|binding| u32::from(binding.param_index))
        .collect();
    let known: Vec<Option<Value>> = instance
        .values
        .iter()
        .enumerate()
        .map(|(index, value)| (!automated.contains(&(index as u32))).then(|| value.clone()))
        .collect();
    let reachable = reachable(graph, instance.root);
    let mut evaluator = Evaluator::new(graph, &known);
    let mut fixed: HashMap<Node, Value> = HashMap::new();
    for &node in &reachable {
        if graph.domain(node).is_instance_fixed()
            && graph.loops_of(node).is_empty()
            && !matches!(graph.op(node), Op::Constant(_))
            && let Some(value) = evaluator.value(node)
        {
            fixed.insert(node, value);
        }
    }
    // Fixed values read by playback-time nodes, or produced as the result.
    let mut frontier: Vec<Node> = Vec::new();
    let add = |node: Node, frontier: &mut Vec<Node>| {
        if fixed.contains_key(&node) && !frontier.contains(&node) {
            frontier.push(node);
        }
    };
    add(instance.root, &mut frontier);
    // Division by a fixed value multiplies by its prepared reciprocal.
    let mut reciprocals: Vec<Node> = Vec::new();
    for &node in &reachable {
        if fixed.contains_key(&node) {
            continue;
        }
        let divisor = match graph.op(node) {
            Op::Binary(Binary::Divide, _, divisor) => Some(*divisor),
            _ => None,
        };
        for operand in structural_operands(graph, node) {
            if Some(operand) == divisor && reciprocal(fixed.get(&operand)).is_some() {
                if !reciprocals.contains(&operand) {
                    reciprocals.push(operand);
                }
            } else {
                add(operand, &mut frontier);
            }
        }
    }
    frontier.sort();
    reciprocals.sort();
    let mut slots = Vec::new();
    let mut values = Vec::new();
    let mut remap = HashMap::new();
    let mut ordered: Vec<u32> = automated.iter().copied().collect();
    ordered.sort();
    for index in ordered {
        remap.insert(index, slots.len() as u32);
        slots.push(graph.params()[index as usize].clone());
        values.push(instance.values[index as usize].clone());
    }
    let mut frontier_slots = HashMap::new();
    for &node in &frontier {
        frontier_slots.insert(node, slots.len() as u32);
        slots.push(Param {
            ty: graph.ty(node).clone(),
            domain: Domain::PARAM,
        });
        values.push(fixed[&node].clone());
    }
    let mut reciprocal_slots = HashMap::new();
    for &node in &reciprocals {
        reciprocal_slots.insert(node, slots.len() as u32);
        slots.push(Param {
            ty: Type::Float,
            domain: Domain::PARAM,
        });
        values.push(Value::Float(
            reciprocal(fixed.get(&node)).unwrap_or_else(|| unreachable!("checked reciprocal")),
        ));
    }
    let automation = instance
        .automation
        .iter()
        .cloned()
        .map(|mut binding| {
            binding.param_index = remap[&u32::from(binding.param_index)] as u16;
            binding
        })
        .collect();
    // Fixed slots hold known values; automated ones change during playback.
    let ranges: Vec<Option<(f64, f64)>> = values
        .iter()
        .enumerate()
        .map(|(slot, value)| match value {
            _ if slot < remap.len() => None,
            Value::Int(value) => Some((f64::from(*value), f64::from(*value))),
            Value::Float(value) if value.is_finite() => {
                Some((f64::from(*value), f64::from(*value)))
            }
            _ => None,
        })
        .collect();
    let lengths: Vec<Option<usize>> = values
        .iter()
        .map(|value| match value {
            Value::Array(items) => Some(items.len()),
            Value::Marks(marks) => Some(marks.len()),
            _ => None,
        })
        .collect();
    let mut target = Graph::new(slots, graph.inputs());
    let mut substitute = Frontier {
        slots: frontier_slots,
        reciprocals: reciprocal_slots,
        remap,
        ranges,
        lengths,
    };
    let root = Rebuild::new(graph).node(&mut target, instance.root, &mut substitute)?;
    Ok(Prepared {
        graph: target,
        root,
        values,
        automation,
    })
}

struct Frontier {
    slots: HashMap<Node, u32>,
    reciprocals: HashMap<Node, u32>,
    remap: HashMap<u32, u32>,
    /// What the instance knows about each slot, for reduction bounds.
    ranges: Vec<Option<(f64, f64)>>,
    lengths: Vec<Option<usize>>,
}

/// The reciprocal of a fixed float divisor, when multiplying by it is exact
/// enough: a finite nonzero divisor with a normal reciprocal.
fn reciprocal(value: Option<&Value>) -> Option<f32> {
    let Some(Value::Float(divisor)) = value else {
        return None;
    };
    let inverse = 1.0 / divisor;
    (divisor.is_finite() && *divisor != 0.0 && inverse.is_normal()).then_some(inverse)
}

impl Substitute for Frontier {
    fn replace(&mut self, target: &mut Graph, node: Node) -> Option<Node> {
        let slot = *self.slots.get(&node)?;
        Some(target.add(Op::Param(slot)))
    }

    fn param(&mut self, target: &mut Graph, index: u32) -> Node {
        let slot = self.remap[&index];
        target.add(Op::Param(slot))
    }

    fn reciprocal(&mut self, target: &mut Graph, divisor: Node) -> Option<Node> {
        let slot = *self.reciprocals.get(&divisor)?;
        Some(target.add(Op::Param(slot)))
    }

    /// Per-pixel bounds within at most a strip of indices share one index.
    fn share_index(&mut self, target: &Graph, start: Node, end: Node) -> bool {
        let per_pixel = target
            .domain(start)
            .union(target.domain(end))
            .contains(Domain::PIXEL);
        per_pixel && self.within_strip(target, start, end)
    }

    fn stencils(&self) -> bool {
        true
    }
}

impl Frontier {
    /// Whether every range `start..end` lies within a strip's width of indices.
    fn within_strip(&self, target: &Graph, start: Node, end: Node) -> bool {
        let bounds = Bounds {
            ranges: &self.ranges,
            lengths: &self.lengths,
        };
        let (first, last) = (
            interval(target, start, &bounds).min,
            interval(target, end, &bounds).max,
        );
        first.is_finite() && last.is_finite() && last - first <= STRIP as f64
    }
}

/// Nodes reachable from `root`, through reductions, in node order.
pub(crate) fn reachable(graph: &Graph, root: Node) -> Vec<Node> {
    let mut seen = HashSet::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if seen.insert(node) {
            pending.extend(structural_operands(graph, node));
        }
    }
    let mut nodes: Vec<Node> = seen.into_iter().collect();
    nodes.sort();
    nodes
}

/// A node's operands, including a reduction's bounds and parts and a loop
/// index's bounds.
pub(crate) fn structural_operands(graph: &Graph, node: Node) -> Vec<Node> {
    match graph.op(node) {
        Op::Reduce(id) => {
            let data = graph.loop_(*id);
            let mut operands = vec![data.start, data.end];
            operands.extend(data.parts());
            operands
        }
        Op::LoopIndex(id) => {
            let data = graph.loop_(*id);
            vec![data.start, data.end]
        }
        op => op.operands(),
    }
}

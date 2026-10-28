//! Scheduling is global code motion over a region tree. The stages are the
//! outermost loops: a node runs once per query, once per target run, or per
//! pixel, as its domain and the backend allow. Within the pixel stage, branch
//! arms and reduction bodies are regions; a node is placed where its uses meet,
//! then hoisted out of every reduction it does not depend on. Work inside a
//! reduction that depends only on its index and on query or target values is
//! the same for every strip: it becomes a table, filled once in its stage and
//! read by index in the loop.
use super::structural_operands;
use crate::compiler::ir::{Binary, Context, Domain, Graph, Node, Op, Reducer, Unary};
use donder_runtime_types::bytecode::STRIP;
use donder_runtime_types::{Type, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Stage {
    /// Once per query: time and parameters.
    Query,
    /// Once per neighbor of a stencil: its neighbor weight.
    Neighbor,
    /// Once per run with the same target shape.
    Target,
    /// Once per index of a reduction, into a table its stage fills.
    Table,
    /// Per pixel.
    Body,
}

pub(crate) type RegionId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionKind {
    Body,
    Then(Node),
    Else(Node),
    /// One iteration of a reduction.
    Loop(Node),
    /// The part of an iteration that contributes, behind its filter.
    Contribute(Node),
}

#[derive(Clone, Debug)]
pub(crate) struct Region {
    pub(crate) parent: Option<RegionId>,
    pub(crate) kind: RegionKind,
    pub(crate) loop_depth: u32,
    /// Placed nodes, operands first.
    pub(crate) nodes: Vec<Node>,
}

/// A backend-neutral execution plan.
#[derive(Clone, Debug)]
pub(crate) struct Plan {
    pub(crate) query: Vec<Node>,
    /// Neighbor weights of every stencil, operands first.
    pub(crate) neighbor: Vec<Node>,
    pub(crate) target: Vec<Node>,
    pub(crate) regions: Vec<Region>,
    pub(crate) region_of: HashMap<Node, RegionId>,
    /// Selects whose arms are separate regions.
    pub(crate) branches: HashSet<Node>,
    /// Region children of each branch and reduction.
    pub(crate) children: HashMap<Node, Vec<RegionId>>,
    pub(crate) uses: HashMap<Node, u32>,
    pub(crate) stage: HashMap<Node, Stage>,
    /// The tables of reductions' index-only values.
    pub(crate) tables: Vec<Table>,
}

/// A reduction's values that depend only on its index and on values of
/// `stage`, the query or the target stage, which fills them. Each output is
/// read in the loop; its cone, operands first, computes it.
#[derive(Clone, Debug)]
pub(crate) struct Table {
    pub(crate) reduction: Node,
    pub(crate) stage: Stage,
    pub(crate) outputs: Vec<(Node, Vec<Node>)>,
    /// A sum over the offsets that have a neighbor in the pixel's fixture,
    /// with the pixel index and target count that bound them: the table holds
    /// its body's prefix sums, so the sum is a difference of two entries.
    pub(crate) interval: Option<(Node, Node)>,
}

impl Plan {
    /// The pixel index and target count bounding `reduction`, when its table
    /// replaces its loop.
    pub(crate) fn interval(&self, reduction: Node) -> Option<(Node, Node)> {
        self.tables
            .iter()
            .find(|table| table.reduction == reduction)
            .and_then(|table| table.interval)
    }
}

impl Plan {
    /// The table outputs read in `reduction`'s loop.
    pub(crate) fn table_outputs(&self, reduction: Node) -> Vec<Node> {
        self.tables
            .iter()
            .filter(|table| table.reduction == reduction)
            .flat_map(|table| table.outputs.iter().map(|(output, _)| *output))
            .collect()
    }
}

impl Plan {
    pub(crate) fn stage(&self, node: Node) -> Stage {
        self.stage.get(&node).copied().unwrap_or(Stage::Query)
    }
    pub(crate) fn uses(&self, node: Node) -> u32 {
        self.uses.get(&node).copied().unwrap_or(0)
    }
}

/// At most this much exclusive work in both arms makes a branch-free choice
/// on a per-pixel condition. A uniform condition branches for any work.
const CHOICE_WORK: usize = 2;

pub(crate) fn schedule(graph: &Graph, root: Node) -> Plan {
    let nodes = super::reachable(graph, root);
    let mut stage = stages(graph, &nodes);
    let tables = tables(graph, &nodes, &mut stage);
    let mut uses = HashMap::new();
    for &node in &nodes {
        for operand in structural_operands(graph, node) {
            *uses.entry(operand).or_insert(0) += 1;
        }
    }
    // Every body select starts as a branch; cheap ones become choices once
    // their exclusive work is known.
    let mut branches: HashSet<Node> = nodes
        .iter()
        .copied()
        .filter(|&node| stage[&node] == Stage::Body && matches!(graph.op(node), Op::Select(..)))
        .collect();
    let trial = place(graph, root, &nodes, &stage, &branches);
    branches.retain(|&select| {
        let Op::Select(condition, ..) = *graph.op(select) else {
            unreachable!("branches are selects")
        };
        let per_pixel = graph
            .domain(condition)
            .intersects(Domain::PIXEL.union(Domain::SIGNAL));
        let (work, heavy) = trial.work(graph, select);
        heavy || work > if per_pixel { CHOICE_WORK } else { 0 }
    });
    let mut plan = place(graph, root, &nodes, &stage, &branches);
    plan.uses = uses;
    plan.tables = tables;
    plan
}

/// The pixel index and target count of a filter that holds where the pixel
/// `index` offsets away is in the target, `0 <= pixel + index < count`, as
/// checking writes it for an `around` that ignores its neighbor.
fn present(graph: &Graph, filter: Option<Node>, index: Node) -> Option<(Node, Node)> {
    let Op::Select(low, high, no) = *graph.op(filter?) else {
        return None;
    };
    let (Op::Binary(Binary::IntGreaterEqual, at, zero), Op::Binary(Binary::IntLess, other, count)) =
        (graph.op(low).clone(), graph.op(high).clone())
    else {
        return None;
    };
    let Op::Binary(Binary::IntAdd, a, b) = *graph.op(at) else {
        return None;
    };
    let pixel = match (a == index, b == index) {
        (true, _) => b,
        (_, true) => a,
        _ => return None,
    };
    let fits = other == at
        && graph.constant_value(zero) == Some(&Value::Int(0))
        && graph.constant_value(no) == Some(&Value::Bool(false))
        && matches!(graph.op(pixel), Op::Context(Context::PixelIndex))
        && matches!(graph.op(count), Op::Context(Context::TargetCount));
    fits.then_some((pixel, count))
}

/// A stage of the blocks that run before the strips.
fn prefix(stage: Stage) -> bool {
    matches!(stage, Stage::Query | Stage::Target)
}

/// Fewest index-only nodes of a reduction worth a table: a single one costs
/// what its lookup does.
const TABLE_WORK: usize = 2;

/// The tables of every reduction whose bounds the query or target stage
/// knows and whose range spans at most a strip; their nodes move to the
/// table stage.
fn tables(graph: &Graph, nodes: &[Node], stage: &mut HashMap<Node, Stage>) -> Vec<Table> {
    let mut tables = Vec::new();
    for &reduction in nodes {
        let Op::Reduce(id) = *graph.op(reduction) else {
            continue;
        };
        let data = graph.loop_(id);
        let bounds = stage[&data.start].max(stage[&data.end]);
        // A stencil's offsets span at most a strip, as checking proves.
        let span = match (
            graph.constant_value(data.start),
            graph.constant_value(data.end),
        ) {
            (Some(Value::Int(start)), Some(Value::Int(end))) => {
                Some(i64::from(*end) - i64::from(*start))
            }
            _ => None,
        };
        let span_fits = span.is_some_and(|span| span <= STRIP as i64)
            || matches!(graph.op(data.body), Op::Tap { .. });
        if !prefix(bounds) || !span_fits {
            continue;
        }
        let mut members: HashSet<Node> = HashSet::new();
        let mut fills = bounds;
        for &node in nodes {
            let candidate = graph.loops_of(node).only() == Some(id)
                && stage[&node] == Stage::Body
                && !is_leaf(graph, node)
                && prefix_capable(graph, node)
                && matches!(graph.ty(node), Type::Int | Type::Float | Type::Bool)
                && !graph
                    .domain(node)
                    .intersects(Domain::PIXEL.union(Domain::SIGNAL))
                && structural_operands(graph, node).iter().all(|operand| {
                    members.contains(operand)
                        || matches!(graph.op(*operand), Op::LoopIndex(loop_) if *loop_ == id)
                        || prefix(stage[operand])
                });
            if candidate {
                for operand in structural_operands(graph, node) {
                    if !members.contains(&operand) && prefix(stage[&operand]) {
                        fills = fills.max(stage[&operand]);
                    }
                }
                members.insert(node);
            }
        }
        // A sum of an index-only body over the offsets with a neighbor is a
        // difference of prefix sums, which take one entry more than the range.
        let interval = present(graph, data.filter, data.index).filter(|_| {
            data.reducer == Reducer::Sum
                && members.contains(&data.body)
                && matches!(graph.ty(reduction), Type::Int | Type::Float)
                && span.is_some_and(|span| span < STRIP as i64)
        });
        if interval.is_none() && members.len() < TABLE_WORK {
            continue;
        }
        // Members read by code outside the table, each with its cone.
        let read_outside: HashSet<Node> = nodes
            .iter()
            .filter(|user| !members.contains(user))
            .flat_map(|&user| structural_operands(graph, user))
            .filter(|operand| members.contains(operand))
            .collect();
        let outputs = nodes
            .iter()
            .filter(|node| match interval {
                Some(_) => **node == data.body,
                None => read_outside.contains(node),
            })
            .map(|&output| {
                let mut cone = HashSet::new();
                let mut pending = vec![output];
                while let Some(node) = pending.pop() {
                    if members.contains(&node) && cone.insert(node) {
                        pending.extend(structural_operands(graph, node));
                    }
                }
                let mut cone: Vec<Node> = cone.into_iter().collect();
                cone.sort();
                (output, cone)
            })
            .collect();
        for &node in &members {
            stage.insert(node, Stage::Table);
        }
        tables.push(Table {
            reduction,
            stage: fills,
            outputs,
            interval,
        });
    }
    tables
}

fn is_primitive(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Float | Type::Bool | Type::Color)
}

/// Leaves that the backend materializes where they are used.
pub(crate) fn is_leaf(graph: &Graph, node: Node) -> bool {
    match graph.op(node) {
        Op::Constant(_) => true,
        Op::Param(_) => !is_primitive(graph.ty(node)),
        _ => false,
    }
}

fn stages(graph: &Graph, nodes: &[Node]) -> HashMap<Node, Stage> {
    let mut stage = HashMap::new();
    for &node in nodes {
        let domain = graph.domain(node);
        let operands = structural_operands(graph, node)
            .into_iter()
            .map(|operand| stage.get(&operand).copied().unwrap_or(Stage::Query))
            .max()
            .unwrap_or(Stage::Query);
        // A stencil's neighbor and the weight computed from it run per neighbor.
        if matches!(graph.op(node), Op::Neighbor)
            || (operands == Stage::Neighbor
                && !matches!(graph.op(node), Op::Tap { .. } | Op::Scan { .. }))
        {
            stage.insert(node, Stage::Neighbor);
            continue;
        }
        let own = if is_leaf(graph, node) {
            Stage::Query
        } else if !graph.loops_of(node).is_empty()
            || domain.intersects(Domain::PIXEL.union(Domain::SIGNAL))
            || !prefix_capable(graph, node)
        {
            Stage::Body
        } else if domain.intersects(Domain::TARGET) {
            Stage::Target
        } else {
            Stage::Query
        };
        stage.insert(node, own.max(operands));
    }
    stage
}

/// Operations the backend can run once, before the strips: everything but
/// pixel queries and the reductions, whose regions only the body has.
fn prefix_capable(graph: &Graph, node: Node) -> bool {
    !matches!(
        graph.op(node),
        Op::Context(
            Context::PixelIndex | Context::PixelFraction | Context::PixelX | Context::PixelY
        ) | Op::Unary(Unary::SectionCount | Unary::SectionIndex, _)
            | Op::Binary(Binary::SectionPosition, ..)
            | Op::Reduce(_)
            | Op::LoopIndex(_)
            | Op::Read { .. }
            | Op::Items(_)
    )
}

/// How a node uses an operand.
#[derive(Clone, Copy)]
enum Use {
    Here,
    Then(Node),
    Else(Node),
    Loop(Node),
    Contribute(Node),
}

fn operand_uses(graph: &Graph, node: Node, branches: &HashSet<Node>) -> Vec<(Node, Use)> {
    match *graph.op(node) {
        Op::Select(condition, yes, no) if branches.contains(&node) => vec![
            (condition, Use::Here),
            (yes, Use::Then(node)),
            (no, Use::Else(node)),
        ],
        Op::Reduce(id) => {
            let data = graph.loop_(id);
            let mut uses = vec![(data.start, Use::Here), (data.end, Use::Here)];
            if let Some(default) = data.default {
                uses.push((default, Use::Here));
            }
            match data.filter {
                Some(filter) => {
                    uses.push((filter, Use::Loop(node)));
                    uses.push((data.body, Use::Contribute(node)));
                }
                None => uses.push((data.body, Use::Loop(node))),
            }
            uses
        }
        Op::LoopIndex(_) => Vec::new(),
        ref op => op
            .operands()
            .into_iter()
            .map(|operand| (operand, Use::Here))
            .collect(),
    }
}

fn place(
    graph: &Graph,
    root: Node,
    nodes: &[Node],
    stage: &HashMap<Node, Stage>,
    branches: &HashSet<Node>,
) -> Plan {
    let mut plan = Plan {
        query: Vec::new(),
        neighbor: Vec::new(),
        target: Vec::new(),
        regions: vec![Region {
            parent: None,
            kind: RegionKind::Body,
            loop_depth: 0,
            nodes: Vec::new(),
        }],
        region_of: HashMap::new(),
        branches: branches.clone(),
        children: HashMap::new(),
        uses: HashMap::new(),
        stage: stage.clone(),
        tables: Vec::new(),
    };
    // Where each body node is used, by region.
    let mut use_regions: HashMap<Node, Vec<Use>> = HashMap::new();
    let mut users: HashMap<Node, Vec<Node>> = HashMap::new();
    for &node in nodes {
        for (operand, kind) in operand_uses(graph, node, branches) {
            use_regions.entry(operand).or_default().push(kind);
            users.entry(operand).or_default().push(node);
        }
    }
    let mut loop_regions: HashMap<crate::compiler::ir::LoopId, RegionId> = HashMap::new();
    for &node in nodes.iter().rev() {
        match stage[&node] {
            Stage::Query if !is_leaf(graph, node) => plan.query.push(node),
            Stage::Neighbor => plan.neighbor.push(node),
            Stage::Target => plan.target.push(node),
            Stage::Body => {
                let region = if node == root {
                    0
                } else if let Op::LoopIndex(id) = graph.op(node) {
                    loop_regions[id]
                } else {
                    let late = users[&node]
                        .iter()
                        .zip(&use_regions[&node])
                        .map(|(&user, &kind)| plan.use_region(user, kind))
                        .reduce(|a, b| plan.common(a, b))
                        .unwrap_or(0);
                    let early = graph
                        .loops_of(node)
                        .innermost()
                        .map_or(0, |id| loop_regions[&id]);
                    plan.hoist(late, early)
                };
                plan.region_of.insert(node, region);
                plan.regions[region].nodes.push(node);
                let depth = plan.regions[region].loop_depth;
                let child = |plan: &mut Plan, kind, depth| {
                    plan.regions.push(Region {
                        parent: Some(region),
                        kind,
                        loop_depth: depth,
                        nodes: Vec::new(),
                    });
                    let id = plan.regions.len() - 1;
                    plan.children.entry(node).or_default().push(id);
                    id
                };
                match graph.op(node) {
                    Op::Select(..) if branches.contains(&node) => {
                        child(&mut plan, RegionKind::Then(node), depth);
                        child(&mut plan, RegionKind::Else(node), depth);
                    }
                    Op::Reduce(id) => {
                        let body = child(&mut plan, RegionKind::Loop(node), depth + 1);
                        loop_regions.insert(*id, body);
                        if graph.loop_(*id).filter.is_some() {
                            let contribute = plan.regions.len();
                            plan.regions.push(Region {
                                parent: Some(body),
                                kind: RegionKind::Contribute(node),
                                loop_depth: depth + 1,
                                nodes: Vec::new(),
                            });
                            plan.children.entry(node).or_default().push(contribute);
                        }
                    }
                    _ => {}
                }
            }
            Stage::Query | Stage::Table => {}
        }
    }
    plan.query.reverse();
    plan.neighbor.reverse();
    plan.target.reverse();
    for region in &mut plan.regions {
        region.nodes.reverse();
    }
    plan
}

impl Plan {
    fn use_region(&self, user: Node, kind: Use) -> RegionId {
        let child = |construct: Node, index: usize| self.children[&construct][index];
        match kind {
            Use::Here => self.region_of.get(&user).copied().unwrap_or(0),
            Use::Then(select) => child(select, 0),
            Use::Else(select) => child(select, 1),
            Use::Loop(reduce) => child(reduce, 0),
            Use::Contribute(reduce) => child(reduce, 1),
        }
    }

    fn depth(&self, mut region: RegionId) -> usize {
        let mut depth = 0;
        while let Some(parent) = self.regions[region].parent {
            region = parent;
            depth += 1;
        }
        depth
    }

    /// The innermost region enclosing both.
    fn common(&self, mut a: RegionId, mut b: RegionId) -> RegionId {
        let (mut depth_a, mut depth_b) = (self.depth(a), self.depth(b));
        while depth_a > depth_b {
            a = self.regions[a].parent.unwrap_or(0);
            depth_a -= 1;
        }
        while depth_b > depth_a {
            b = self.regions[b].parent.unwrap_or(0);
            depth_b -= 1;
        }
        while a != b {
            a = self.regions[a].parent.unwrap_or(0);
            b = self.regions[b].parent.unwrap_or(0);
        }
        a
    }

    /// The region between `late` and its ancestor `early` with the fewest
    /// enclosing reductions, nearest to `late`.
    fn hoist(&self, late: RegionId, early: RegionId) -> RegionId {
        let target = self.regions[early].loop_depth;
        let mut region = late;
        while self.regions[region].loop_depth > target {
            let Some(parent) = self.regions[region].parent else {
                break;
            };
            region = parent;
        }
        region
    }

    /// Nodes placed in a branch's arms, and whether they include a reduction
    /// or a signal sample.
    fn work(&self, graph: &Graph, select: Node) -> (usize, bool) {
        let mut count = 0;
        let mut heavy = false;
        let mut pending: Vec<RegionId> = self.children.get(&select).cloned().unwrap_or_default();
        while let Some(region) = pending.pop() {
            for node in &self.regions[region].nodes {
                count += 1;
                if let Some(children) = self.children.get(node) {
                    heavy |= !matches!(self.regions[children[0]].kind, RegionKind::Then(_));
                    pending.extend(children);
                }
            }
            heavy |= self.regions[region]
                .nodes
                .iter()
                .any(|node| matches!(graph.op(*node), Op::Read { .. }));
        }
        (count, heavy)
    }
}

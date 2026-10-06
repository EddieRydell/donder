//! Scheduling is global code motion over a region tree. The stages are the
//! outermost loops: a node runs once per query, once per target run, or per
//! pixel, as its domain and the backend allow. Within the pixel stage, branch
//! arms and reduction bodies are regions; a node is placed where its uses meet,
//! then hoisted out of every reduction it does not depend on.
use super::structural_operands;
use crate::dsl::ir::{Binary, Context, Domain, Graph, Node, Op, Unary};
use crate::dsl::types::Type;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Stage {
    /// Once per query: time and parameters.
    Query,
    /// Once per run with the same target shape.
    Target,
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
    pub(crate) target: Vec<Node>,
    pub(crate) regions: Vec<Region>,
    pub(crate) region_of: HashMap<Node, RegionId>,
    /// Selects whose arms are separate regions.
    pub(crate) branches: HashSet<Node>,
    /// Region children of each branch and reduction.
    pub(crate) children: HashMap<Node, Vec<RegionId>>,
    pub(crate) uses: HashMap<Node, u32>,
    pub(crate) stage: HashMap<Node, Stage>,
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
    let stage = stages(graph, &nodes);
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
    plan
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
        let operands = structural_operands(graph, node)
            .into_iter()
            .map(|operand| stage.get(&operand).copied().unwrap_or(Stage::Query))
            .max()
            .unwrap_or(Stage::Query);
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
            | Op::Sample { .. }
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
    let mut loop_regions: HashMap<crate::dsl::ir::LoopId, RegionId> = HashMap::new();
    for &node in nodes.iter().rev() {
        match stage[&node] {
            Stage::Query if !is_leaf(graph, node) => plan.query.push(node),
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
            Stage::Query => {}
        }
    }
    plan.query.reverse();
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
                .any(|node| matches!(graph.op(*node), Op::Sample { .. }));
        }
        (count, heavy)
    }
}

//! A readable execution plan: every scheduled node with its domain, in the
//! stage and region where it runs, followed by the lowered bytecode.
use super::schedule::{Plan, RegionId, RegionKind, is_leaf};
use super::{Prepared, structural_operands};
use crate::dsl::ir::{Domain, Graph, Node, Op};
use core::fmt::Write;

pub(crate) fn explain(prepared: &Prepared, plan: &Plan, listing: &str) -> String {
    let graph = &prepared.graph;
    let mut text = String::new();
    let _ = writeln!(text, "slots:");
    for (index, (param, value)) in graph.params().iter().zip(&prepared.values).enumerate() {
        let _ = writeln!(
            text,
            "  p{index}: {:?} {} = {value:?}",
            param.ty,
            domain(param.domain)
        );
    }
    for (title, nodes) in [("query", &plan.query), ("target", &plan.target)] {
        let _ = writeln!(text, "{title}:");
        for &node in nodes {
            let _ = writeln!(text, "  {}", line(graph, node));
        }
    }
    let _ = writeln!(text, "pixel:");
    region(graph, plan, 0, 1, &mut text);
    let _ = writeln!(text, "bytecode:\n{listing}");
    text
}

fn region(graph: &Graph, plan: &Plan, region_id: RegionId, depth: usize, text: &mut String) {
    let indent = "  ".repeat(depth);
    for &node in &plan.regions[region_id].nodes {
        let mode = match graph.op(node) {
            Op::Select(..) if plan.branches.contains(&node) => " (branch)",
            Op::Select(..) => " (choose)",
            Op::Reduce(_) => " (loop)",
            _ => "",
        };
        let _ = writeln!(text, "{indent}{}{mode}", line(graph, node));
        for &child in plan.children.get(&node).into_iter().flatten() {
            let name = match plan.regions[child].kind {
                RegionKind::Then(_) => "then",
                RegionKind::Else(_) => "else",
                RegionKind::Loop(_) => "each",
                RegionKind::Contribute(_) => "contribute",
                RegionKind::Body => "body",
            };
            let _ = writeln!(text, "{indent}  {name}:");
            region(graph, plan, child, depth + 2, text);
        }
    }
}

fn line(graph: &Graph, node: Node) -> String {
    let operands: Vec<String> = structural_operands(graph, node)
        .into_iter()
        .map(|operand| name(graph, operand))
        .collect();
    let op = match graph.op(node) {
        Op::Constant(constant) => format!("{:?}", constant.value),
        Op::Param(index) => format!("p{index}"),
        Op::ParamIntegral(index) => format!("integral p{index}"),
        Op::Context(context) => format!("{context:?}"),
        Op::LoopIndex(id) => format!("index L{}", id.index()),
        Op::Unary(op, _) => format!("{op:?}"),
        Op::Binary(op, ..) => format!("{op:?}"),
        Op::Ternary(op, ..) => format!("{op:?}"),
        Op::Select(..) => "Select".into(),
        Op::Reduce(id) => format!("{:?} L{}", graph.loop_(*id).reducer, id.index()),
        Op::Sample { input, pixel, .. } => format!("Sample input{input} {pixel:?}"),
        Op::Items(_) => "Items".into(),
        Op::Pick { .. } => "Pick".into(),
    };
    format!(
        "n{:<4} = {op} {}   [{}{}]",
        node.index(),
        operands.join(" "),
        domain(graph.domain(node)),
        if graph.loops_of(node).is_empty() {
            ""
        } else {
            " in loop"
        }
    )
}

fn name(graph: &Graph, node: Node) -> String {
    match graph.op(node) {
        Op::Constant(constant) if is_leaf(graph, node) => format!("{:?}", constant.value),
        _ => format!("n{}", node.index()),
    }
}

fn domain(domain: Domain) -> String {
    let parts: Vec<&str> = [
        (Domain::PARAM, "param"),
        (Domain::TIME, "time"),
        (Domain::TARGET, "target"),
        (Domain::PIXEL, "pixel"),
        (Domain::SIGNAL, "signal"),
    ]
    .into_iter()
    .filter(|(bit, _)| domain.contains(*bit))
    .map(|(_, name)| name)
    .collect();
    if parts.is_empty() {
        "constant".into()
    } else {
        parts.join("|")
    }
}

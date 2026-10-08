//! Construction-time simplification. Every rewrite preserves the value of the
//! expression, except where the float policy allows: real-number algebra may
//! change rounding and signed zero, but never hides a missing (NaN) value.
use super::eval::{self, as_bool, as_color, as_float, as_int};
use super::{Binary, Graph, LoopId, Node, Op, Reducer, Ternary, Unary};
use donder_runtime_types::Color;
use donder_runtime_types::{Type, Value};

/// Commutative operations put a constant operand last and otherwise order
/// their operands, so equal expressions intern to one node.
pub(super) fn canonical(graph: &Graph, op: Op) -> Op {
    match op {
        Op::Binary(binary, a, b) if binary.commutative() => {
            let a_constant = graph.constant_value(a).is_some();
            let b_constant = graph.constant_value(b).is_some();
            if (a_constant && !b_constant) || (a_constant == b_constant && b < a) {
                Op::Binary(binary, b, a)
            } else {
                Op::Binary(binary, a, b)
            }
        }
        op => op,
    }
}

pub(super) fn simplify(graph: &mut Graph, op: &Op, ty: &Type) -> Option<Node> {
    match *op {
        Op::Pick { index, ref items } => match graph.constant_value(index).and_then(as_int) {
            Some(index) => {
                Some(items[donder_runtime_types::sampling::clamp_array_index(index, items.len())])
            }
            None if items.iter().all(|item| *item == items[0]) => Some(items[0]),
            None => None,
        },
        Op::Unary(unary, a) => simplify_unary(graph, unary, a, ty),
        Op::Binary(binary, a, b) => simplify_binary(graph, binary, a, b, ty),
        // Zero value is black for every hue and saturation, even NaN.
        Op::Ternary(Ternary::Hsv, _, _, c)
            if graph.constant_value(c).and_then(as_float) == Some(0.0) =>
        {
            Some(graph.color(Color::BLACK))
        }
        Op::Ternary(ternary, a, b, c) => {
            let values = (
                graph.constant_value(a)?.clone(),
                graph.constant_value(b)?.clone(),
                graph.constant_value(c)?.clone(),
            );
            let value = eval::ternary(ternary, &values.0, &values.1, &values.2)?;
            Some(graph.typed_constant(value, ty.clone()))
        }
        Op::Select(condition, yes, no) => simplify_select(graph, condition, yes, no),
        _ => None,
    }
}

fn simplify_unary(graph: &mut Graph, op: Unary, a: Node, ty: &Type) -> Option<Node> {
    // The length of a literal, or of a choice between arrays, is fixed per arm.
    if op == Unary::Len {
        match graph.op(a).clone() {
            Op::Items(items) => return Some(graph.int(items.len() as i32)),
            Op::Select(condition, yes, no) if is_items(graph, yes) || is_items(graph, no) => {
                let (yes, no) = (graph.unary(op, yes), graph.unary(op, no));
                return Some(graph.select(condition, yes, no));
            }
            _ => {}
        }
    }
    if let Some(value) = graph.constant_value(a) {
        let value = eval::unary(op, value)?;
        return Some(graph.typed_constant(value, ty.clone()));
    }
    match (op, graph.op(a)) {
        (Unary::Not, Op::Unary(Unary::Not, inner)) => Some(*inner),
        (Unary::Negate, Op::Unary(Unary::Negate, inner)) => Some(*inner),
        _ => None,
    }
}

fn simplify_binary(graph: &mut Graph, op: Binary, a: Node, b: Node, ty: &Type) -> Option<Node> {
    use Binary::*;
    let left = graph.constant_value(a).cloned();
    let right = graph.constant_value(b).cloned();
    if let (Some(left), Some(right)) = (&left, &right) {
        let value = eval::binary(op, left, right, ty)?;
        return Some(graph.typed_constant(value, ty.clone()));
    }
    let float = |value: &Option<Value>| value.as_ref().and_then(as_float);
    let int = |value: &Option<Value>| value.as_ref().and_then(as_int);
    let color = |value: &Option<Value>| value.as_ref().and_then(as_color);
    let black = Some(Color::BLACK);
    // Indexing a literal picks one of its items.
    if op == Index {
        match graph.op(a).clone() {
            Op::Items(items) => return Some(graph.add(Op::Pick { index: b, items })),
            Op::Select(condition, yes, no) if is_items(graph, yes) || is_items(graph, no) => {
                let (yes, no) = (graph.binary(op, yes, b), graph.binary(op, no, b));
                return Some(graph.select(condition, yes, no));
            }
            _ => {}
        }
    }
    match op {
        Add if float(&right) == Some(0.0) => Some(a),
        Subtract if float(&right) == Some(0.0) => Some(a),
        Multiply | Divide if float(&right) == Some(1.0) => Some(a),
        // Division by a fixed divisor multiplies by its reciprocal.
        Divide => {
            let divisor = float(&right)?;
            let inverse = 1.0 / divisor;
            (divisor.is_finite() && divisor != 0.0 && inverse.is_normal()).then(|| {
                let inverse = graph.float(inverse);
                graph.binary(Multiply, a, inverse)
            })
        }
        IntAdd | IntSubtract if int(&right) == Some(0) => Some(a),
        IntMultiply if int(&right) == Some(1) => Some(a),
        Min | Max | ColorMax if a == b => Some(a),
        Equal | NotEqual if a == b && !matches!(graph.ty(a), Type::Float) => {
            Some(graph.bool(op == Equal))
        }
        ColorAdd | ColorMax if color(&right) == black => Some(a),
        ColorScale if color(&left) == black => Some(a),
        ColorScale if float(&right) == Some(1.0) => Some(a),
        ColorScale if float(&right) == Some(0.0) => Some(graph.color(Color::BLACK)),
        ColorMultiply if color(&right) == black => Some(b),
        Power if int(&right) == Some(1) => Some(a),
        _ => None,
    }
}

fn simplify_select(graph: &mut Graph, condition: Node, yes: Node, no: Node) -> Option<Node> {
    if let Some(value) = graph.constant_value(condition).and_then(as_bool) {
        return Some(if value { yes } else { no });
    }
    if yes == no {
        return Some(yes);
    }
    match (
        graph.constant_value(yes).and_then(as_bool),
        graph.constant_value(no).and_then(as_bool),
    ) {
        (Some(true), Some(false)) => return Some(condition),
        (Some(false), Some(true)) => return Some(graph.unary(Unary::Not, condition)),
        _ => {}
    }
    if let Op::Unary(Unary::Not, inner) = *graph.op(condition) {
        return Some(graph.select(inner, no, yes));
    }
    // A choice on `a && b` or `a || b` chooses on `a` first, so `b` is only
    // evaluated when it decides.
    if let Op::Select(first, then, otherwise) = *graph.op(condition) {
        let constant = |node| graph.constant_value(node).and_then(as_bool);
        let pick = |value: bool| if value { yes } else { no };
        match (constant(then), constant(otherwise)) {
            (_, Some(value)) => {
                let inner = graph.select(then, yes, no);
                return Some(graph.select(first, inner, pick(value)));
            }
            (Some(value), _) => {
                let inner = graph.select(otherwise, yes, no);
                return Some(graph.select(first, pick(value), inner));
            }
            _ => {}
        }
    }
    // A nested choice on the same condition already knows its outcome.
    if let Op::Select(inner, chosen, _) = *graph.op(yes)
        && inner == condition
    {
        return Some(graph.select(condition, chosen, no));
    }
    if let Op::Select(inner, _, chosen) = *graph.op(no)
        && inner == condition
    {
        return Some(graph.select(condition, yes, chosen));
    }
    None
}

/// A reduction that needs no loop: an empty range, an invariant body, or a
/// body that only contributes the identity.
pub(super) fn reduction(graph: &mut Graph, id: LoopId) -> Option<Node> {
    let data = graph.loop_(id).clone();
    let ty = graph.ty(data.body).clone();
    let identity = |graph: &mut Graph| match data.default {
        Some(default) => default,
        None => {
            let value = eval::reduce_identity(data.reducer, &ty)
                .unwrap_or_else(|| unreachable!("checked reduction type"));
            graph.constant(value)
        }
    };
    let filter = data
        .filter
        .map(|filter| graph.constant_value(filter).and_then(as_bool));
    if filter == Some(Some(false)) {
        return Some(identity(graph));
    }
    let empty = graph.binary(Binary::IntLessEqual, data.end, data.start);
    if graph.constant_value(empty).and_then(as_bool) == Some(true) {
        return Some(identity(graph));
    }
    let body_identity = eval::reduce_identity(data.reducer, &ty)
        .zip(graph.constant_value(data.body).cloned())
        .is_some_and(|(identity, body)| super::same_value(&identity, &body));
    if body_identity && matches!(data.reducer, Reducer::Max | Reducer::Min | Reducer::Sum) {
        return Some(identity(graph));
    }
    let invariant = data
        .parts()
        .filter(|&part| Some(part) != data.default)
        .all(|part| !graph.loops_of(part).contains(id));
    let idempotent = !matches!(data.reducer, Reducer::Sum);
    if !(invariant && idempotent) {
        return None;
    }
    // Every iteration contributes the same value: one is enough.
    let nonempty = graph.unary(Unary::Not, empty);
    let contributes = match (data.reducer, data.filter) {
        (Reducer::Any | Reducer::All, _) | (_, None) => nonempty,
        (_, Some(filter)) => graph.and(nonempty, filter),
    };
    Some(match data.reducer {
        Reducer::Any => graph.and(contributes, data.body),
        Reducer::All => {
            let empty = graph.unary(Unary::Not, contributes);
            graph.or(empty, data.body)
        }
        _ => {
            let identity = identity(graph);
            graph.select(contributes, data.body, identity)
        }
    })
}

/// An array literal, or a choice involving one.
fn is_items(graph: &Graph, node: Node) -> bool {
    match *graph.op(node) {
        Op::Items(_) => true,
        Op::Select(_, yes, no) => is_items(graph, yes) || is_items(graph, no),
        _ => false,
    }
}

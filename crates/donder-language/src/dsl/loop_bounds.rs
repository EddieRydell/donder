use super::ast::{BinaryOp, UnaryOp};
use super::checked::{CheckedBlock, CheckedExpr, CheckedExprKind, CheckedStmt};
use super::types::{Identifier, Type, Value};
use donder_runtime::MAX_DSL_LOOP_ITERATIONS;

pub(super) fn fixed_for_iterations(
    initializer: &CheckedStmt,
    condition: &CheckedExpr,
    update: &CheckedStmt,
    body: &CheckedBlock,
) -> Option<usize> {
    let CheckedStmt::Local {
        ty: Type::Int,
        name,
        initializer: Some(initial),
    } = initializer
    else {
        return None;
    };
    let mut index = constant_int(initial)?;
    let CheckedExprKind::Binary {
        op: comparison,
        left,
        right,
    } = &condition.kind
    else {
        return None;
    };
    if !is_variable(left, name) {
        return None;
    }
    let bound = constant_int(right)?;
    let CheckedStmt::Assign {
        name: updated,
        value,
    } = update
    else {
        return None;
    };
    if updated != name {
        return None;
    }
    let CheckedExprKind::Binary {
        op: operation,
        left,
        right,
    } = &value.kind
    else {
        return None;
    };
    if !is_variable(left, name) || block_assigns_name(body, name) {
        return None;
    }
    let step = constant_int(right)?;
    for count in 0..=MAX_DSL_LOOP_ITERATIONS {
        let active = match comparison {
            BinaryOp::Less => index < bound,
            BinaryOp::LessEqual => index <= bound,
            BinaryOp::Greater => index > bound,
            BinaryOp::GreaterEqual => index >= bound,
            _ => return None,
        };
        if !active {
            return Some(count);
        }
        index = match operation {
            BinaryOp::Add => index.wrapping_add(step),
            BinaryOp::Subtract => index.wrapping_sub(step),
            BinaryOp::Multiply => index.wrapping_mul(step),
            _ => return None,
        };
    }
    None
}

fn is_variable(expr: &CheckedExpr, name: &Identifier) -> bool {
    matches!(&expr.kind, CheckedExprKind::Variable(candidate) if candidate == name)
}

fn constant_int(expr: &CheckedExpr) -> Option<i32> {
    match &expr.kind {
        CheckedExprKind::Literal(Value::Int(value)) => Some(*value),
        CheckedExprKind::Unary {
            op: UnaryOp::Negate,
            expr,
        } => constant_int(expr).map(i32::wrapping_neg),
        _ => None,
    }
}

pub(super) fn block_assigns_name(block: &CheckedBlock, name: &Identifier) -> bool {
    block_assigns_name_in_scope(block, name, false)
}

fn block_assigns_name_in_scope(
    block: &CheckedBlock,
    name: &Identifier,
    mut shadowed: bool,
) -> bool {
    for statement in &block.statements {
        match statement {
            CheckedStmt::Local { name: local, .. } if local == name => shadowed = true,
            CheckedStmt::Assign { name: assigned, .. } if assigned == name && !shadowed => {
                return true;
            }
            CheckedStmt::If {
                then_block,
                else_block,
                ..
            } => {
                if block_assigns_name_in_scope(then_block, name, shadowed)
                    || else_block
                        .as_ref()
                        .is_some_and(|block| block_assigns_name_in_scope(block, name, shadowed))
                {
                    return true;
                }
            }
            CheckedStmt::For {
                initializer,
                update,
                body,
                ..
            } => {
                let loop_shadowed = shadowed
                    || matches!(initializer.as_ref(), CheckedStmt::Local { name: local, .. } if local == name);
                if (!loop_shadowed
                    && (matches!(initializer.as_ref(), CheckedStmt::Assign { name: assigned, .. } if assigned == name)
                        || matches!(update.as_ref(), CheckedStmt::Assign { name: assigned, .. } if assigned == name)))
                    || block_assigns_name_in_scope(body, name, loop_shadowed)
                {
                    return true;
                }
            }
            CheckedStmt::ForMarks { index, body, .. }
            | CheckedStmt::ForRange { index, body, .. }
                if block_assigns_name_in_scope(body, name, shadowed || index == name) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

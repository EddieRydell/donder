//! Hash the actual specialization program, not a second compilation of its source.
use super::{Calculation, Expression, GeneratorProgram, Statement};
use crate::dsl::{hash_bytecode, hash_param_decls, hash_value};
use std::hash::{Hash, Hasher};

pub(in crate::dsl) fn hash_semantics(program: &GeneratorProgram, state: &mut impl Hasher) {
    hash_param_decls(program.params(), state);
    program.slot_count().hash(state);
    for emission in program.emissions() {
        hash_param_decls(emission, state);
    }
    block(program.body(), state);
}

fn calculation<O: crate::dsl::CalculationOutput, I: Hash>(
    value: &Calculation<O, I>,
    state: &mut impl Hasher,
) {
    value.inputs.hash(state);
    value.program.input_types().hash(state);
    value.program.output_types().hash(state);
    hash_bytecode(value.program.bytecode(), state);
}

fn expression(value: &Expression, state: &mut impl Hasher) {
    std::mem::discriminant(value).hash(state);
    match value {
        Expression::Constant(value) => hash_value(value, state),
        Expression::Read(slot) => slot.hash(state),
        Expression::Calculate(value) => calculation(value, state),
    }
}

fn block(statements: &[Statement], state: &mut impl Hasher) {
    statements.len().hash(state);
    for value in statements {
        statement(value, state);
    }
}

fn statement(value: &Statement, state: &mut impl Hasher) {
    std::mem::discriminant(value).hash(state);
    match value {
        Statement::Assign { slot, value } => {
            slot.hash(state);
            expression(value, state);
        }
        Statement::Expression(value) => expression(value, state),
        Statement::Branch {
            condition,
            then_block,
            else_block,
        } => {
            calculation(condition, state);
            block(then_block, state);
            block(else_block, state);
        }
        Statement::For {
            initializer,
            iterations,
            update,
            body,
        } => {
            statement(initializer, state);
            iterations.hash(state);
            statement(update, state);
            block(body, state);
        }
        Statement::Marks { index, marks, body } => {
            index.hash(state);
            calculation(marks, state);
            block(body, state);
        }
        Statement::Range {
            index,
            count,
            cap,
            body,
        } => {
            index.hash(state);
            calculation(count, state);
            cap.hash(state);
            block(body, state);
        }
        Statement::Calculate {
            assigned,
            calculation: value,
        } => {
            assigned.hash(state);
            calculation(value, state);
        }
        Statement::Emit {
            slot,
            start,
            duration,
            target,
            params,
        } => {
            slot.hash(state);
            calculation(start, state);
            calculation(duration, state);
            calculation(target, state);
            params.len().hash(state);
            for (name, value) in params {
                name.hash(state);
                expression(value, state);
            }
        }
    }
}

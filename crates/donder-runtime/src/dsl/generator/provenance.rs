//! Builder-bound generators may transform inherited targets, but may not embed
//! independently addressed pixel records in constants or declaration defaults.
use super::*;
use crate::dsl::bytecode::BytecodeProgram;

pub(super) fn value(value: &Value) -> bool {
    match value {
        Value::Target(target) => target.groups.iter().all(|group| group.pixels.is_empty()),
        Value::TargetItems(target) => target.groups.iter().all(|group| group.pixels.is_empty()),
        Value::TargetItem(target) => target.pixels.is_empty(),
        Value::Array(values) => values.iter().all(self::value),
        _ => true,
    }
}

pub(super) fn declarations(params: &[ParamDecl]) -> bool {
    params
        .iter()
        .all(|param| !param.ty.is_context_only() && param.default.as_ref().is_none_or(value))
}

pub(super) fn bytecode<C, S, A, B>(program: &BytecodeProgram<C, S, A, B>) -> bool {
    program
        .targets
        .iter()
        .all(|target| target.groups.iter().all(|group| group.pixels.is_empty()))
        && program
            .target_lists
            .iter()
            .all(|target| target.groups.iter().all(|group| group.pixels.is_empty()))
        && program
            .target_items
            .iter()
            .all(|target| target.pixels.is_empty())
        && program
            .array_constants
            .iter()
            .all(|values| values.iter().all(value))
}

pub(super) fn program(program: &GeneratorProgram) -> bool {
    declarations(&program.params)
        && program.emissions.iter().all(|params| declarations(params))
        && block(&program.body)
}

fn calculation<O: super::super::CalculationOutput, I>(calculation: &Calculation<O, I>) -> bool {
    bytecode(calculation.program.bytecode())
}

fn expression(expression: &Expression) -> bool {
    match expression {
        Expression::Constant(constant) => value(constant),
        Expression::Read(_) => true,
        Expression::Calculate(computed) => calculation(computed),
    }
}

fn block(block: &Block) -> bool {
    block.iter().all(statement)
}

fn statement(statement: &Statement) -> bool {
    match statement {
        Statement::Assign { value, .. } | Statement::Expression(value) => expression(value),
        Statement::Calculate {
            calculation: computed,
            ..
        } => calculation(computed),
        Statement::Branch {
            condition,
            then_block,
            else_block,
        } => calculation(condition) && block(then_block) && block(else_block),
        Statement::For {
            initializer,
            update,
            body,
            ..
        } => self::statement(initializer) && self::statement(update) && block(body),
        Statement::Marks { marks, body, .. } => calculation(marks) && block(body),
        Statement::Range { count, body, .. } => calculation(count) && block(body),
        Statement::Emit {
            start,
            duration,
            target,
            params,
            ..
        } => {
            calculation(start)
                && calculation(duration)
                && calculation(target)
                && params.iter().all(|(_, parameter)| expression(parameter))
        }
    }
}

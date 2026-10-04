#[cfg(feature = "host")]
mod array_lowering;
#[cfg(feature = "host")]
mod ast;
#[cfg(feature = "host")]
pub use ast::{DeclarationKind, DeclarationSpan};
mod bindings;
mod blocks;
pub mod bytecode;
mod invocation;
mod operator;
mod sample;
pub use bindings::{BindingError, BoundParams};
pub use blocks::{BatchPlan, LaneOp, NO_REGISTER, lane_registers};
pub use bytecode::BytecodeProgram;
pub use invocation::{OperatorDefinition, OperatorInvocation, SampleDefinition, SampleInvocation};
pub use operator::{OperatorProgram, SignalAccess};
pub use sample::SampleProgram;
pub const MAX_DSL_LOOP_ITERATIONS: usize = 10_000;
#[cfg(feature = "host")]
mod checked;
#[cfg(feature = "host")]
mod compiled_effect;
#[cfg(feature = "host")]
mod compiler;
#[cfg(feature = "host")]
pub use compiled_effect::CompiledEffect;
#[cfg(feature = "host")]
mod declarations;
#[cfg(feature = "host")]
pub use declarations::{CompiledOperator, OperatorInputDecl, ParamDecl, bind_params};
#[cfg(feature = "host")]
mod diagnostic;
#[cfg(feature = "host")]
mod fixed_params;
#[cfg(feature = "host")]
mod fusion;
#[cfg(feature = "host")]
mod loop_bounds;
#[cfg(feature = "host")]
mod optimize;
#[cfg(feature = "host")]
mod specialize;
#[cfg(feature = "host")]
pub use specialize::ProgramConstants;
#[cfg(feature = "host")]
mod parser;
#[cfg(feature = "host")]
mod typecheck;

#[cfg(feature = "host")]
use compiler::{compile_checked_effects, compile_checked_operators};
#[cfg(feature = "host")]
use core::hash::{Hash, Hasher};
#[cfg(feature = "host")]
pub use diagnostic::Diagnostic;
#[cfg(feature = "host")]
use parser::parse_module;
#[cfg(feature = "host")]
use typecheck::check_module;

#[cfg(feature = "host")]
pub(crate) mod lexer;
pub mod types;

pub use crate::values::{Color, Curve, CurvePoint, Gradient, GradientStop, Marks};
pub use types::{Identifier, Type, Value};

/// Each top-level declaration's kind, name, and byte range, in source order.
#[cfg(feature = "host")]
pub fn declaration_spans(source: &str) -> Result<Vec<DeclarationSpan>, Vec<Diagnostic>> {
    Ok(parse_module(source)?.declarations)
}

/// Compile sample effect declarations from a DSL source.
#[cfg(feature = "host")]
pub fn compile_effects(source: &str) -> Result<Vec<CompiledEffect>, Vec<Diagnostic>> {
    let module = parse_module(source)?;
    if !module.operators.is_empty() {
        return Err(vec![Diagnostic::new(
            lexer::TextSpan { start: 0, end: 0 },
            "operator declarations are not allowed in effect sources",
        )]);
    }
    compile_checked_effects(check_module(module)?).map_err(|error| vec![error])
}

#[cfg(feature = "host")]
pub fn compile_operators(source: &str) -> Result<Vec<CompiledOperator>, Vec<Diagnostic>> {
    let module = parse_module(source)?;
    if !module.effects.is_empty() {
        return Err(vec![Diagnostic::new(
            lexer::TextSpan { start: 0, end: 0 },
            "effect declarations are not allowed in operator sources",
        )]);
    }
    let module = check_module(module)?;
    compile_checked_operators(module).map_err(|error| vec![error])
}

#[cfg(feature = "host")]
pub fn hash_compiled_effect<H: Hasher>(effect: &CompiledEffect, state: &mut H) {
    effect.name.hash(state);
    hash_param_decls(&effect.params, state);
    hash_bytecode(effect.program.bytecode(), state);
}

#[cfg(feature = "host")]
fn hash_bytecode<H: Hasher, C: Hash, S: Hash, A: Hash>(
    bytecode: &BytecodeProgram<C, S, A>,
    state: &mut H,
) {
    bytecode.instructions.hash(state);
    bytecode.enums.hash(state);
    bytecode.enum_types.len().hash(state);
    for ty in &bytecode.enum_types {
        ty.ty().hash(state);
    }
    bytecode.array_constants.len().hash(state);
    for values in &bytecode.array_constants {
        hash_values(values, state);
    }
    bytecode.curves.len().hash(state);
    for curve in &bytecode.curves {
        hash_curve(curve, state);
    }
    bytecode.gradients.len().hash(state);
    for gradient in &bytecode.gradients {
        hash_gradient(gradient, state);
    }
    bytecode.value_operands.hash(state);
    bytecode.layout.hash(state);
    bytecode.array_types.hash(state);
    bytecode.loop_count.hash(state);
    bytecode.uses_pixel_context.hash(state);
    bytecode.pixel_entry.hash(state);
    bytecode.array_capacity.hash(state);
    bytecode.array_width.hash(state);
}

#[cfg(feature = "host")]
fn hash_param_decls<H: Hasher>(params: &[ParamDecl], state: &mut H) {
    params.len().hash(state);
    for param in params {
        param.fixed.hash(state);
        param.name.hash(state);
        param.ty.hash(state);
        hash_optional_value(&param.default, state);
    }
}

#[cfg(feature = "host")]
fn hash_optional_value<H: Hasher>(value: &Option<Value>, state: &mut H) {
    match value {
        Some(value) => {
            1u8.hash(state);
            hash_value(value, state);
        }
        None => 0u8.hash(state),
    }
}

#[cfg(feature = "host")]
fn hash_values<H: Hasher>(values: &[Value], state: &mut H) {
    values.len().hash(state);
    for value in values {
        hash_value(value, state);
    }
}

#[cfg(feature = "host")]
fn hash_value<H: Hasher>(value: &Value, state: &mut H) {
    match value {
        Value::Void => 0u8.hash(state),
        Value::Int(value) => {
            1u8.hash(state);
            value.hash(state);
        }
        Value::Float(value) => {
            2u8.hash(state);
            value.to_bits().hash(state);
        }
        Value::Bool(value) => {
            3u8.hash(state);
            value.hash(state);
        }
        Value::Color(value) => {
            4u8.hash(state);
            value.hash(state);
        }
        Value::Marks(value) => {
            5u8.hash(state);
            value.as_slice().len().hash(state);
            for mark in value.as_slice() {
                mark.as_ticks().hash(state);
            }
        }
        Value::Curve(value) => {
            9u8.hash(state);
            hash_curve(value, state);
        }
        Value::Gradient(value) => {
            10u8.hash(state);
            hash_gradient(value, state);
        }
        Value::Array(values) => {
            11u8.hash(state);
            hash_values(values, state);
        }
        Value::Enum(value) => {
            12u8.hash(state);
            value.hash(state);
        }
    }
}

#[cfg(feature = "host")]
fn hash_curve<H: Hasher>(curve: &Curve, state: &mut H) {
    curve.points.len().hash(state);
    for point in &curve.points {
        point.position.to_bits().hash(state);
        point.value.to_bits().hash(state);
    }
}

#[cfg(feature = "host")]
fn hash_gradient<H: Hasher>(gradient: &Gradient, state: &mut H) {
    gradient.stops.len().hash(state);
    for stop in &gradient.stops {
        stop.position.to_bits().hash(state);
        stop.color.hash(state);
    }
}

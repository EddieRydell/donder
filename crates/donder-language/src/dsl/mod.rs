//! The effect language. Source is parsed and checked into a dataflow IR
//! (`ir`); preparation instantiates, rewrites and lowers it (`lower`) to the
//! portable strip bytecode that playback runs (`bytecode`). The pipeline is
//! described in `docs/effect_compiler.md`.
mod bindings;
pub mod bytecode;
mod invocation;
mod operator;
mod sample;
pub use bindings::{BindingError, BoundParams};
pub use bytecode::BytecodeProgram;
pub use invocation::{OperatorDefinition, OperatorInvocation, SampleDefinition, SampleInvocation};
pub use operator::OperatorProgram;
pub use sample::SampleProgram;
pub const MAX_DSL_LOOP_ITERATIONS: usize = 10_000;

#[cfg(feature = "host")]
pub mod builtins;
#[cfg(feature = "host")]
pub(crate) mod check;
#[cfg(feature = "host")]
mod declarations;
#[cfg(feature = "host")]
mod definition;
#[cfg(feature = "host")]
mod diagnostic;
#[cfg(feature = "host")]
mod instance;
#[cfg(feature = "host")]
mod ir;
#[cfg(feature = "host")]
mod lower;
#[cfg(feature = "host")]
pub(crate) mod syntax;

#[cfg(feature = "host")]
pub use declarations::{OperatorInputDecl, ParamDecl, ParamRange, bind_params};
#[cfg(feature = "host")]
pub use definition::{
    CompiledEffect, CompiledOperator, CompiledScript, Instance, Invocation, SignalAddressing,
    compile_effects, compile_operators, compile_script,
};
#[cfg(feature = "host")]
pub use diagnostic::Diagnostic;
#[cfg(feature = "host")]
pub use instance::ProgramConstants;
#[cfg(feature = "host")]
pub use syntax::ast::{DeclarationKind, DeclarationSpan};
#[cfg(feature = "host")]
pub use syntax::lexer::TextSpan;

pub mod types;

pub use crate::values::{Color, Curve, CurvePoint, Gradient, GradientStop, Marks};
pub use types::{Identifier, Type, Value};

/// Each top-level declaration's kind, name, and byte range, in source order.
#[cfg(feature = "host")]
pub fn declaration_spans(source: &str) -> Result<Vec<DeclarationSpan>, Vec<Diagnostic>> {
    syntax::declaration_spans(source)
}

/// Each top-level `fn` declaration's byte range, in source order.
#[cfg(feature = "host")]
pub fn function_spans(source: &str) -> Result<Vec<TextSpan>, Vec<Diagnostic>> {
    syntax::function_spans(source)
}

/// Hash what a compiled effect renders: its name, parameters and behavior.
#[cfg(feature = "host")]
pub fn hash_compiled_effect<H: core::hash::Hasher>(effect: &CompiledEffect, state: &mut H) {
    use core::hash::Hash;
    effect.name().hash(state);
    effect.fingerprint().hash(state);
}

/// One instruction per line, nested code indented, for inspecting lowered
/// programs.
pub fn listing(program: &BytecodeProgram) -> alloc::string::String {
    use core::fmt::Write;
    let mut text = alloc::string::String::new();
    // Instructions left in each open construct.
    let mut open: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
    for (ip, instruction) in program.code.iter().enumerate() {
        if ip == usize::from(program.query_end) {
            let _ = writeln!(text, "target:");
        }
        if ip == usize::from(program.target_end) {
            let _ = writeln!(text, "body:");
        }
        let indent = "  ".repeat(open.len() + 1);
        let _ = writeln!(text, "{ip:>4}{indent}{instruction:?}");
        for remaining in &mut open {
            *remaining -= 1;
        }
        open.retain(|&remaining| remaining != 0);
        if instruction.nested() != 0 {
            open.push(instruction.nested());
        }
    }
    let _ = writeln!(text, "result {:?}", program.result);
    text
}

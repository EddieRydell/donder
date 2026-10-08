//! The effect language. Source is parsed and checked into a dataflow IR
//! (`ir`); preparation instantiates, rewrites and lowers it (`lower`) to the
//! portable strip bytecode that playback runs (`donder_runtime_types::bytecode`).
//! The pipeline is described in `docs/effect_compiler.md`.
pub(crate) mod builtins;
pub use builtins::{
    BUILTINS, Builtin, BuiltinFunction, BuiltinGroup, ContextValue, Signature, builtin_reference,
};
pub(crate) mod check;
mod declarations;
mod definition;
mod diagnostic;
mod instance;
mod ir;
mod lower;
pub(crate) mod syntax;

pub use declarations::{OperatorInputDecl, ParamDecl, ParamRange, bind_params};
pub use definition::{
    CompiledEffect, CompiledOperator, CompiledScript, Instance, Invocation, SignalAddressing,
    compile_effects, compile_operators, compile_script,
};
pub use diagnostic::Diagnostic;
pub use instance::ProgramConstants;
pub use syntax::ast::{DeclarationKind, DeclarationSpan};
pub use syntax::lexer::TextSpan;

/// Each top-level declaration's kind, name, and byte range, in source order.
pub fn declaration_spans(source: &str) -> Result<Vec<DeclarationSpan>, Vec<Diagnostic>> {
    syntax::declaration_spans(source)
}

/// The declarations that parse, ignoring syntax errors elsewhere.
pub fn partial_declaration_spans(source: &str) -> Vec<DeclarationSpan> {
    syntax::partial_declaration_spans(source)
}

/// Each top-level `fn` declaration's byte range, in source order.
pub fn function_spans(source: &str) -> Result<Vec<TextSpan>, Vec<Diagnostic>> {
    syntax::function_spans(source)
}

/// Hash what a compiled effect renders: its name, parameters and behavior.
pub fn hash_compiled_effect<H: core::hash::Hasher>(effect: &CompiledEffect, state: &mut H) {
    use core::hash::Hash;
    effect.name().hash(state);
    effect.fingerprint().hash(state);
}

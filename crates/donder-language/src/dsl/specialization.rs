//! Compile generator control and calculations together before preparation.
use super::Identifier;
use super::{Diagnostic, EmittedReference, ParamDecl, RuntimeError, Type, Value};
use donder_runtime::{
    BindingSlot, Block, Calculation, Expression, FixedBindingSlot, FixedCalculation, Statement,
};
pub use donder_runtime::{
    BoundGenerator, GeneratedEffectSlot, GeneratorBinding, GeneratorCalculation, GeneratorContext,
    GeneratorInput, GeneratorProgram, SpecializedChild, SpecializedGenerator,
};
mod compilation;
mod hashing;
pub(super) use hashing::hash_semantics;

pub(super) fn compile(
    params: Vec<ParamDecl>,
    body: super::checked::CheckedBlock,
    emissions: Vec<EmittedReference>,
    controls: &super::staging::PreparationControls,
) -> Result<GeneratorProgram, Diagnostic> {
    let (body, slots) = compilation::compile(&params, body, &emissions, controls)?;
    let emissions = emissions
        .iter()
        .map(|emission| {
            emission
                .parameters()
                .map(|argument| ParamDecl {
                    name: argument.name.clone(),
                    ty: argument.ty.clone(),
                    fixed: argument.live_dependency.is_none(),
                    default: None,
                })
                .collect()
        })
        .collect();
    GeneratorProgram::admit(params, body, slots, emissions).ok_or_else(|| {
        Diagnostic::new(
            super::lexer::TextSpan { start: 0, end: 0 },
            "invalid compiled generator binding plan",
        )
    })
}

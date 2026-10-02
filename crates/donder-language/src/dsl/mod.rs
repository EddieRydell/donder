mod array_lowering;
mod ast;
mod bytecode;
mod checked;
mod compiled_effect;
mod compiler;
pub use compiled_effect::{CompiledEffect, EffectKind, EffectProgram};
mod diagnostic;
mod emission;
mod loop_bounds;
mod optimize;
mod parser;
mod specialization;
mod staging;
pub use specialization::{
    BoundGenerator, GeneratedEffectSlot, GeneratorBinding, GeneratorCalculation, GeneratorContext,
    GeneratorInput, GeneratorProgram, SpecializedChild, SpecializedGenerator,
};
mod typecheck;
pub use emission::validate_emission;

use crate::imports::ImportDeclaration;
use compiler::{compile_checked_effects, compile_checked_operators};
pub use diagnostic::Diagnostic;
pub use donder_runtime::dsl::{
    BoundCalculation, BoundParams, CalculationOutput, CalculationProgram, CompiledOperator,
    DslBindCache, OperatorInputDecl, OperatorRunContext, ParamDecl, RunContext, RuntimeError,
    SignalPixel, SignalSampler, VmWorkspace, bytecode::BytecodeProgram,
};
use parser::parse_module;
use std::hash::{Hash, Hasher};
use typecheck::check_module;

pub(crate) mod lexer;
pub mod types;

pub use crate::values::{Color, Curve, CurvePoint, Gradient, GradientStop, Marks};
pub use types::{Identifier, TargetItemValue, TargetItemsValue, TargetValue, Type, Value};

#[derive(Clone, Debug)]
pub struct EffectImport {
    pub declaration: ImportDeclaration,
    pub span: lexer::TextSpan,
    pub source_spans: Vec<lexer::TextSpan>,
}

impl PartialEq for EffectImport {
    fn eq(&self, other: &Self) -> bool {
        self.declaration == other.declaration
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledEffectDocument {
    pub imports: Vec<EffectImport>,
    pub effects: Vec<EffectCompilation>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectCompilation {
    pub effect: CompiledEffect,
    pub emitted_references: Box<[EmittedReference]>,
}

#[derive(Clone, Debug)]
pub struct EmittedReference {
    pub arguments: Vec<EmittedArgument>,
    pub reference: crate::imports::SourceReference,
    pub span: lexer::TextSpan,
}

impl PartialEq for EmittedReference {
    fn eq(&self, other: &Self) -> bool {
        self.reference == other.reference && self.arguments == other.arguments
    }
}

#[derive(Clone, Debug)]
pub struct EmittedArgument {
    pub name: Identifier,
    pub ty: Type,
    pub live_dependency: Option<Identifier>,
    pub span: lexer::TextSpan,
}

impl PartialEq for EmittedArgument {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.ty == other.ty
            && self.live_dependency == other.live_dependency
    }
}

/// Compile a source document, retaining imports for the project linker.
pub fn compile_effect_document(source: &str) -> Result<CompiledEffectDocument, Vec<Diagnostic>> {
    let module = parse_module(source)?;
    if !module.operators.is_empty() {
        return Err(vec![Diagnostic::new(
            lexer::TextSpan { start: 0, end: 0 },
            "operator declarations are not allowed in effect sources",
        )]);
    }
    let imports = module.imports.clone();
    let module = check_module(module)?;
    let effects = compile_checked_effects(module).map_err(|error| vec![error])?;
    Ok(CompiledEffectDocument { imports, effects })
}

/// Standalone compilation has no project context to resolve imports.
pub fn compile_effects(source: &str) -> Result<Vec<EffectCompilation>, Vec<Diagnostic>> {
    let document = compile_effect_document(source)?;
    if let Some(import) = document.imports.first() {
        return Err(vec![Diagnostic::new(
            import.span,
            "effect imports require document compilation and project linking",
        )]);
    }
    Ok(document.effects)
}

/// Parse import spans for structural path edits without recompiling bytecode.
pub fn effect_source_imports(source: &str) -> Result<Vec<EffectImport>, Vec<Diagnostic>> {
    Ok(parse_module(source)?.imports)
}

pub fn compile_operators(source: &str) -> Result<Vec<CompiledOperator>, Vec<Diagnostic>> {
    let module = parse_module(source)?;
    if let Some(import) = module.imports.first() {
        return Err(vec![Diagnostic::new(
            import.span,
            "imports are only supported in effect documents",
        )]);
    }
    if !module.effects.is_empty() {
        return Err(vec![Diagnostic::new(
            lexer::TextSpan { start: 0, end: 0 },
            "effect declarations are not allowed in operator sources",
        )]);
    }
    let module = check_module(module)?;
    compile_checked_operators(module).map_err(|error| vec![error])
}

pub fn hash_compiled_effect<H: Hasher>(effect: &CompiledEffect, state: &mut H) {
    effect.name.hash(state);
    hash_param_decls(&effect.params, state);
    effect.kind().hash(state);
    match &effect.program {
        EffectProgram::Sample(program) => hash_bytecode(program, state),
        EffectProgram::Generator(program) => program.hash_semantics(state),
    }
}

fn hash_bytecode<H: Hasher, C: Hash, S: Hash>(bytecode: &BytecodeProgram<C, S>, state: &mut H) {
    bytecode.instructions.hash(state);
    bytecode.enums.hash(state);
    bytecode.enum_types.len().hash(state);
    for ty in &bytecode.enum_types {
        ty.ty().hash(state);
    }
    bytecode.targets.len().hash(state);
    for target in &bytecode.targets {
        hash_target_items(&target.groups, state);
    }
    bytecode.target_lists.len().hash(state);
    for target in &bytecode.target_lists {
        hash_target_items(&target.groups, state);
    }
    bytecode.target_items.len().hash(state);
    for target in &bytecode.target_items {
        hash_target_pixels(&target.pixels, state);
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

fn hash_param_decls<H: Hasher>(params: &[ParamDecl], state: &mut H) {
    params.len().hash(state);
    for param in params {
        param.fixed.hash(state);
        param.name.hash(state);
        param.ty.hash(state);
        hash_optional_value(&param.default, state);
    }
}

fn hash_optional_value<H: Hasher>(value: &Option<Value>, state: &mut H) {
    match value {
        Some(value) => {
            1u8.hash(state);
            hash_value(value, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_values<H: Hasher>(values: &[Value], state: &mut H) {
    values.len().hash(state);
    for value in values {
        hash_value(value, state);
    }
}

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
            value.marks.len().hash(state);
            for mark in &value.marks {
                mark.as_ticks().hash(state);
            }
        }
        Value::Target(value) => {
            6u8.hash(state);
            hash_target_items(&value.groups, state);
        }
        Value::TargetItems(value) => {
            7u8.hash(state);
            hash_target_items(&value.groups, state);
        }
        Value::TargetItem(value) => {
            8u8.hash(state);
            hash_target_pixels(&value.pixels, state);
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

fn hash_target_items<H: Hasher>(items: &[std::sync::Arc<TargetItemValue>], state: &mut H) {
    items.len().hash(state);
    for item in items {
        hash_target_pixels(&item.pixels, state);
    }
}

fn hash_target_pixels<H: Hasher>(pixels: &[donder_runtime::signal::PreparedPixel], state: &mut H) {
    pixels.len().hash(state);
    for pixel in pixels {
        pixel.fixture_index.hash(state);
        pixel.fixture_pixel_index.hash(state);
        pixel.pixel_index.hash(state);
        pixel.pixel_count.hash(state);
        pixel.pixel_fraction.to_bits().hash(state);
    }
}

fn hash_curve<H: Hasher>(curve: &Curve, state: &mut H) {
    curve.points.len().hash(state);
    for point in &curve.points {
        point.position.to_bits().hash(state);
        point.value.to_bits().hash(state);
    }
}

fn hash_gradient<H: Hasher>(gradient: &Gradient, state: &mut H) {
    gradient.stops.len().hash(state);
    for stop in &gradient.stops {
        stop.position.to_bits().hash(state);
        stop.color.hash(state);
    }
}

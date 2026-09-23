mod array_lowering;
mod ast;
mod bytecode;
mod checked;
mod compiler;
mod diagnostic;
mod emission;
mod optimize;
mod parser;
mod specialization;
mod staging;
pub use specialization::{
    GeneratorBinding, GeneratorCalculation, GeneratorInput, GeneratorProgram, SpecializedChild,
    SpecializedGenerator,
};
mod typecheck;
pub use emission::validate_emission;

use crate::imports::ImportDeclaration;
use compiler::{compile_checked_effects, compile_checked_operators};
pub use diagnostic::Diagnostic;
pub use donder_runtime::dsl::{
    BoundParams, CompiledEffect, CompiledOperator, DslBindCache, EffectKind, GeneratedEffect,
    GeneratedEffectSlot, GeneratorContext, OperatorInputDecl, OperatorRunContext, ParamDecl,
    RunContext, RuntimeError, SignalPixel, SignalSampler, VmWorkspace, bytecode::BytecodeProgram,
};
use parser::parse_module;
use std::hash::{Hash, Hasher};
use typecheck::check_module;

pub(crate) mod lexer;
pub mod types;

pub use crate::values::{Color, Curve, CurvePoint, Gradient, GradientStop, Marks};
pub use types::{
    Identifier, TargetItemValue, TargetItemsValue, TargetPixelValue, TargetValue, Type, Value,
};

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
    pub generator: Option<GeneratorProgram>,
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
    const RESERVED: &[&str] = &[
        "Max",
        "Add",
        "Multiply",
        "IntensityModulate",
        "Dim",
        "Invert",
        "Colorize",
        "Delay",
        "Echo",
        "max",
        "add",
        "multiply",
        "intensity_modulate",
        "dim",
        "invert",
        "colorize",
        "delay",
        "echo",
    ];
    if let Some(operator) = module
        .operators
        .iter()
        .find(|operator| RESERVED.contains(&operator.name.as_str()))
    {
        return Err(vec![Diagnostic::new(
            lexer::TextSpan { start: 0, end: 0 },
            format!("operator name `{}` is reserved", operator.name.as_str()),
        )]);
    }
    let module = check_module(module)?;
    compile_checked_operators(module).map_err(|error| vec![error])
}

pub fn hash_compiled_effect<H: Hasher>(effect: &CompiledEffect, state: &mut H) {
    effect.name.hash(state);
    hash_param_decls(&effect.params, state);
    effect.kind.hash(state);
    hash_bytecode(&effect.bytecode, state);
    effect.emit_fields.hash(state);
    effect.generated_effect_count.hash(state);
}

fn hash_bytecode<H: Hasher>(bytecode: &BytecodeProgram, state: &mut H) {
    bytecode.instructions.hash(state);
    hash_values(&bytecode.constants, state);
    bytecode.value_operands.hash(state);
    bytecode.layout.hash(state);
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

fn hash_target_pixels<H: Hasher>(pixels: &[TargetPixelValue], state: &mut H) {
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

use super::ast::{BinaryOp, UnaryOp};
use super::bytecode::{
    ArithmeticOp, ArraySlot, BoolSlot, BytecodeProgram, ColorBinary, ColorComponent, ColorSlot,
    CompareOp, ContextRead, CurveSlot, EnumSlot, EnumSlotType, FloatBinary, FloatSlot, FloatUnary,
    GradientSlot, Instruction, IntArithmeticOp, IntSlot, LocalId, MarkOp, MarksSlot, NumberSlot,
    ParamId, PoolSpan, SlotLayout, Target, TargetItemSlot, TargetItemsOp, TargetItemsSlot,
    TargetMember, TargetSlot, TargetSource, ValueSlot,
};
use super::checked::{
    CheckedBlock, CheckedEffectDecl, CheckedExpr, CheckedExprKind, CheckedModule,
    CheckedOperatorDecl, CheckedStmt,
};
use super::types::{Identifier, Type, Value};
use super::{CompiledEffect, CompiledOperator, EffectKind, EffectProgram};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

pub(crate) fn compile_checked_effects(
    module: CheckedModule,
) -> Result<Vec<super::EffectCompilation>, super::Diagnostic> {
    module.effects.into_iter().map(compile_effect).collect()
}

pub(crate) fn compile_checked_operators(
    module: CheckedModule,
) -> Result<Vec<CompiledOperator>, super::Diagnostic> {
    module.operators.into_iter().map(compile_operator).collect()
}

pub(super) fn compile_value(
    params: &[super::ParamDecl],
    statements: Vec<CheckedStmt>,
    outputs: Vec<CheckedExpr>,
) -> Result<super::CalculationProgram, super::Diagnostic> {
    check_parameter_count(params)?;
    if outputs.len() > usize::from(u16::MAX) + 1 {
        return Err(super::Diagnostic::new(
            super::lexer::TextSpan { start: 0, end: 0 },
            "generator calculation exceeds the parameter output slot space",
        ));
    }
    let output_types = outputs.iter().map(|output| output.ty.clone()).collect();
    let program = FunctionCompiler::new(params, EffectKind::Generator)
        .compile_with_outputs(CheckedBlock { statements }, Some(outputs))?;
    super::CalculationProgram::new(
        program,
        params.iter().map(|param| param.ty.clone()).collect(),
        output_types,
    )
    .ok_or_else(invalid_compiled_program)
}

fn compile_effect(
    effect: CheckedEffectDecl,
) -> Result<super::EffectCompilation, super::Diagnostic> {
    check_parameter_count(&effect.params)?;
    let kind = if effect.entrypoint.name.as_str() == "generate" {
        EffectKind::Generator
    } else {
        EffectKind::Sample
    };
    let mut emitted_references = Vec::new();
    let program = match kind {
        EffectKind::Generator => {
            collect_emissions(&effect.body, &mut emitted_references);
            EffectProgram::Generator(std::sync::Arc::new(super::specialization::compile(
                effect.params.clone(),
                effect.body,
                emitted_references.clone(),
                &effect.preparation_controls,
            )?))
        }
        EffectKind::Sample => {
            let bytecode = FunctionCompiler::new(&effect.params, kind).compile(effect.body)?;
            let program = super::SampleProgram::admit(
                bytecode,
                effect.params.iter().map(|param| param.ty.clone()).collect(),
            )
            .ok_or_else(invalid_compiled_program)?;
            EffectProgram::Sample(std::sync::Arc::new(program))
        }
    };
    Ok(super::EffectCompilation {
        effect: CompiledEffect {
            name: effect.name,
            params: effect.params,
            program,
        },
        emitted_references: emitted_references.into_boxed_slice(),
    })
}

/// Linker slots follow authored statement order, independent of specialization.
fn collect_emissions(block: &CheckedBlock, emissions: &mut Vec<super::EmittedReference>) {
    for statement in &block.statements {
        match statement {
            CheckedStmt::Emit { effect, .. } => emissions.push(effect.clone()),
            CheckedStmt::If {
                then_block,
                else_block,
                ..
            } => {
                collect_emissions(then_block, emissions);
                if let Some(block) = else_block {
                    collect_emissions(block, emissions);
                }
            }
            CheckedStmt::For { body, .. }
            | CheckedStmt::ForMarks { body, .. }
            | CheckedStmt::ForRange { body, .. } => collect_emissions(body, emissions),
            _ => {}
        }
    }
}

fn compile_operator(operator: CheckedOperatorDecl) -> Result<CompiledOperator, super::Diagnostic> {
    check_parameter_count(&operator.params)?;
    let bytecode = FunctionCompiler::new_operator(&operator.params, &operator.inputs)
        .compile(operator.body)?;
    CompiledOperator::admit(operator.name, operator.inputs, operator.params, bytecode)
        .ok_or_else(invalid_compiled_program)
}

fn invalid_compiled_program() -> super::Diagnostic {
    super::Diagnostic::new(
        super::lexer::TextSpan { start: 0, end: 0 },
        "compiler produced invalid bytecode",
    )
}

/// Prepared automation and retained bindings address declaration slots with u16.
/// The declaration fixes this count; parameter values never change it.
fn check_parameter_count(params: &[super::ParamDecl]) -> Result<(), super::Diagnostic> {
    if params.len() > usize::from(u16::MAX) + 1 {
        return Err(super::Diagnostic::new(
            super::lexer::TextSpan { start: 0, end: 0 },
            "declaration exceeds the parameter slot space",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod parameter_slot_tests {
    #[test]
    fn parameter_slot_bound_is_checked_during_compilation() {
        let declaration = super::super::ParamDecl {
            name: super::Identifier::new("parameter".into()).unwrap(),
            ty: super::Type::Float,
            default: None,
            fixed: false,
        };
        let mut params = vec![declaration.clone(); usize::from(u16::MAX) + 1];
        assert!(super::check_parameter_count(&params).is_ok());
        params.push(declaration);
        assert!(super::check_parameter_count(&params).is_err());
    }
}

struct FunctionCompiler {
    instructions: Vec<Instruction>,
    array_constants: Vec<std::sync::Arc<[Value]>>,
    enums: Vec<Identifier>,
    enum_types: Vec<Type>,
    target_items: Vec<std::sync::Arc<super::types::TargetItemValue>>,
    target_lists: Vec<std::sync::Arc<super::types::TargetItemsValue>>,
    targets: Vec<std::sync::Arc<super::types::TargetValue>>,
    curves: Vec<std::sync::Arc<crate::values::Curve>>,
    gradients: Vec<std::sync::Arc<crate::values::Gradient>>,
    value_operands: Vec<ValueSlot>,
    scopes: Vec<IndexMap<Identifier, Binding>>,
    param_types: Vec<Type>,
    layout: SlotLayout,
    array_types: Vec<Type>,
    kind: EffectKind,
    signal_inputs: IndexMap<Identifier, usize>,
    assigned_names: HashSet<Identifier>,
    context_reads: HashMap<ContextRead, ValueSlot>,
    param_reads: HashMap<ParamId, ValueSlot>,
    loop_count: u32,
    invalid_loop: bool,
    invalid_emission: Option<super::lexer::TextSpan>,
}

fn constant_array_item(expr: &CheckedExpr) -> Option<Value> {
    match &expr.kind {
        CheckedExprKind::Literal(value) => Some(value.clone()),
        CheckedExprKind::Array(items) => items
            .iter()
            .map(constant_array_item)
            .collect::<Option<Vec<_>>>()
            .map(|values| Value::Array(values.into())),
        _ => None,
    }
}

#[derive(Clone)]
enum Binding {
    Param(ParamId),
    Local(LocalId),
}

fn collect_assigned_names(block: &CheckedBlock, assigned: &mut HashSet<Identifier>) {
    for statement in &block.statements {
        collect_statement_assigned_names(statement, assigned);
    }
}

fn collect_statement_assigned_names(statement: &CheckedStmt, assigned: &mut HashSet<Identifier>) {
    match statement {
        CheckedStmt::Assign { name, .. } => {
            assigned.insert(name.clone());
        }
        CheckedStmt::If {
            then_block,
            else_block,
            ..
        } => {
            collect_assigned_names(then_block, assigned);
            if let Some(else_block) = else_block {
                collect_assigned_names(else_block, assigned);
            }
        }
        CheckedStmt::For {
            initializer,
            update,
            body,
            ..
        } => {
            collect_statement_assigned_names(initializer, assigned);
            collect_statement_assigned_names(update, assigned);
            collect_assigned_names(body, assigned);
        }
        CheckedStmt::ForMarks { body, .. } | CheckedStmt::ForRange { body, .. } => {
            collect_assigned_names(body, assigned)
        }
        CheckedStmt::Local { .. }
        | CheckedStmt::Expr(_)
        | CheckedStmt::Emit { .. }
        | CheckedStmt::Return(_) => {}
    }
}

fn context_read(name: &Identifier) -> Option<ContextRead> {
    match name.as_str() {
        "progress" => Some(ContextRead::Progress),
        "seconds" => Some(ContextRead::Seconds),
        "duration" => Some(ContextRead::Duration),
        "pixel_index" => Some(ContextRead::PixelIndex),
        "pixel_count" => Some(ContextRead::PixelCount),
        "pixel_fraction" => Some(ContextRead::PixelFraction),
        "pixel_x" => Some(ContextRead::PixelX),
        "pixel_y" => Some(ContextRead::PixelY),
        "target_min_x" => Some(ContextRead::TargetMinX),
        "target_min_y" => Some(ContextRead::TargetMinY),
        "target_max_x" => Some(ContextRead::TargetMaxX),
        "target_max_y" => Some(ContextRead::TargetMaxY),

        _ => None,
    }
}

fn float_const_operand(
    op: BinaryOp,
    result_ty: &Type,
    left: &CheckedExpr,
    right: &CheckedExpr,
) -> Option<(bool, f32)> {
    let supported = match op {
        BinaryOp::Add
        | BinaryOp::Subtract
        | BinaryOp::Multiply
        | BinaryOp::Divide
        | BinaryOp::Remainder => matches!(result_ty, Type::Float),
        BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
            !matches!((&left.ty, &right.ty), (Type::Int, Type::Int))
        }
        _ => false,
    };
    if !supported {
        return None;
    }
    if let Some(constant) = numeric_literal(left) {
        return Some((true, constant));
    }
    numeric_literal(right).map(|constant| (false, constant))
}

fn numeric_literal(expr: &CheckedExpr) -> Option<f32> {
    match &expr.kind {
        CheckedExprKind::Literal(Value::Int(value)) => Some(*value as f32),
        CheckedExprKind::Literal(Value::Float(value)) => Some(*value),
        _ => None,
    }
}

fn numeric_literal_argument(args: &[CheckedExpr]) -> Option<(usize, f32)> {
    if args.len() != 2 {
        return None;
    }
    numeric_literal(&args[0])
        .map(|constant| (0, constant))
        .or_else(|| numeric_literal(&args[1]).map(|constant| (1, constant)))
}

impl FunctionCompiler {
    fn new(params: &[super::ast::ParamDecl], kind: EffectKind) -> Self {
        let mut param_scope = IndexMap::new();
        for (index, param) in params.iter().enumerate() {
            param_scope.insert(param.name.clone(), Binding::Param(index));
        }
        Self {
            instructions: Vec::new(),
            array_constants: Vec::new(),
            enums: Vec::new(),
            enum_types: Vec::new(),
            target_items: Vec::new(),
            target_lists: Vec::new(),
            targets: Vec::new(),
            curves: Vec::new(),
            gradients: Vec::new(),
            value_operands: Vec::new(),
            scopes: vec![param_scope],
            param_types: params.iter().map(|param| param.ty.clone()).collect(),
            layout: SlotLayout::default(),
            array_types: Vec::new(),
            kind,
            signal_inputs: IndexMap::new(),
            assigned_names: HashSet::new(),
            context_reads: HashMap::new(),
            param_reads: HashMap::new(),
            loop_count: 0,
            invalid_loop: false,
            invalid_emission: None,
        }
    }

    fn new_operator(
        params: &[super::ast::ParamDecl],
        inputs: &[super::ast::OperatorInputDecl],
    ) -> Self {
        let mut compiler = Self::new(params, EffectKind::Sample);
        compiler.signal_inputs = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| (input.name.clone(), index))
            .collect();
        compiler
    }

    fn compile(&mut self, block: CheckedBlock) -> Result<BytecodeProgram, super::Diagnostic> {
        self.compile_with_outputs(block, None)
    }

    fn compile_with_outputs(
        &mut self,
        block: CheckedBlock,
        outputs: Option<Vec<CheckedExpr>>,
    ) -> Result<BytecodeProgram, super::Diagnostic> {
        collect_assigned_names(&block, &mut self.assigned_names);
        // Parameters are immutable inputs. Assignment uses an ordinary local,
        // initialized once on entry, including assignments inside branches/loops.
        let assigned_params = self.scopes[0]
            .iter()
            .filter_map(|(name, binding)| match binding {
                Binding::Param(index) if self.assigned_names.contains(name) => {
                    Some((name.clone(), *index))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for (name, index) in assigned_params {
            let slot = self.allocate_slot(&self.param_types[index].clone());
            self.emit_load_param(slot, index);
            self.scopes[0].insert(name, Binding::Local(slot));
        }
        self.scopes.push(IndexMap::new());
        for statement in block.statements {
            self.compile_statement(statement);
        }
        if let Some(outputs) = outputs {
            // Calculations return a declared list of registers, not an erased
            // array<void> whose shape must be reconstructed during execution.
            let values = outputs
                .into_iter()
                .map(|output| self.compile_expr(output))
                .collect();
            let outputs = self.add_value_operands(values);
            self.emit(Instruction::ReturnValues(outputs));
        }
        let _ = self.scopes.pop();
        if let Some(span) = self.invalid_emission {
            return Err(super::Diagnostic::new(
                span,
                "generator emissions belong to specialization, not calculation bytecode",
            ));
        }
        if self.invalid_loop {
            return Err(super::Diagnostic::new(
                super::lexer::TextSpan { start: 0, end: 0 },
                "loop bound could not be compiled",
            ));
        }
        if self.array_constants.len() > u32::MAX as usize {
            return Err(super::Diagnostic::new(
                super::lexer::TextSpan { start: 0, end: 0 },
                "constant pool exceeds 32-bit addressable capacity",
            ));
        }
        super::array_lowering::lower_arrays(&mut self.instructions, &mut self.value_operands);
        super::optimize::cleanup(
            &mut self.instructions,
            &mut self.array_constants,
            &mut self.value_operands,
            &mut self.layout,
            &mut self.array_types,
            &mut self.enum_types,
        );
        let pixel_entry = if self.kind == EffectKind::Sample {
            super::optimize::hoist_uniform(&mut self.instructions, &mut self.value_operands)
        } else {
            0
        };
        let mut program = BytecodeProgram {
            pixel_entry,
            array_capacity: 0,
            array_width: 0,
            loop_count: self.loop_count,
            uses_pixel_context: self.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::ContextRead {
                        read: ContextRead::PixelIndex
                            | ContextRead::PixelCount
                            | ContextRead::PixelFraction
                            | ContextRead::PixelX
                            | ContextRead::PixelY
                            | ContextRead::TargetMinX
                            | ContextRead::TargetMinY
                            | ContextRead::TargetMaxX
                            | ContextRead::TargetMaxY,
                        ..
                    } | Instruction::SectionPosition { .. }
                        | Instruction::SignalSample { .. }
                )
            }),
            instructions: std::mem::take(&mut self.instructions).into_boxed_slice(),
            targets: std::mem::take(&mut self.targets).into_boxed_slice(),
            target_lists: std::mem::take(&mut self.target_lists).into_boxed_slice(),
            target_items: std::mem::take(&mut self.target_items).into_boxed_slice(),
            array_constants: std::mem::take(&mut self.array_constants).into_boxed_slice(),
            enums: std::mem::take(&mut self.enums).into_boxed_slice(),
            enum_types: std::mem::take(&mut self.enum_types)
                .into_iter()
                .map(EnumSlotType::new)
                .collect::<Option<Box<[_]>>>()
                .ok_or_else(invalid_compiled_program)?,
            curves: std::mem::take(&mut self.curves).into_boxed_slice(),
            gradients: std::mem::take(&mut self.gradients).into_boxed_slice(),
            value_operands: std::mem::take(&mut self.value_operands).into_boxed_slice(),
            array_types: std::mem::take(&mut self.array_types).into_boxed_slice(),
            layout: self.layout,
        };
        let (array_capacity, array_width) = program.required_array_storage().ok_or_else(|| {
            super::Diagnostic::new(
                super::lexer::TextSpan { start: 0, end: 0 },
                "calculated array storage exceeds 32-bit addressable capacity",
            )
        })?;
        program.array_capacity = array_capacity;
        program.array_width = array_width;
        if !program.has_valid_structure()
            || !program.has_valid_parameter_reads(|index| {
                self.param_types
                    .get(index)
                    .map(super::bytecode::ParameterKind::for_type)
            })
            || !program.has_valid_reference_parameter_reads(|index, expected| {
                self.param_types
                    .get(index)
                    .is_some_and(|actual| expected.accepts(actual))
            })
        {
            return Err(invalid_compiled_program());
        }
        Ok(program)
    }

    fn compile_block(&mut self, block: CheckedBlock) {
        self.scopes.push(IndexMap::new());
        for statement in block.statements {
            self.compile_statement(statement);
        }
        let _ = self.scopes.pop();
    }

    fn compile_statement(&mut self, statement: CheckedStmt) {
        match statement {
            CheckedStmt::Local {
                ty,
                name,
                initializer,
            } => {
                let bind_directly = !self.assigned_names.contains(&name)
                    && initializer
                        .as_ref()
                        .is_some_and(|initializer| self.initializer_can_bind_directly(initializer));
                match initializer {
                    Some(initializer) if bind_directly => {
                        let value = self.compile_expr(initializer);
                        let value = self.coerce_slot(value, &ty);
                        self.bind_local(name, value);
                    }
                    Some(initializer) => {
                        // The initializer sees the outer scope, just as it did
                        // during type checking. Bind the new name only afterward.
                        let value = self.compile_expr(initializer);
                        let value = self.coerce_slot(value, &ty);
                        let slot = self.allocate_local(name, &ty);
                        self.emit(Instruction::Move {
                            dst: slot,
                            src: value.index(),
                        });
                    }
                    None => {
                        let slot = self.allocate_local(name, &ty);
                        self.emit_default(slot, &ty);
                    }
                }
            }
            CheckedStmt::Assign { name, value } => {
                let value = self.compile_expr(value);
                match self.lookup(&name) {
                    Some(Binding::Local(slot)) => {
                        let value = self.coerce_to_slot(value, slot);
                        self.emit(Instruction::Move {
                            dst: slot,
                            src: value.index(),
                        });
                    }
                    Some(Binding::Param(_)) => unreachable!("assigned parameters are locals"),
                    None => {}
                }
            }
            CheckedStmt::Expr(expr) => {
                let _ = self.compile_expr(expr);
            }
            CheckedStmt::If {
                condition,
                then_block,
                else_block,
            } => {
                let condition = self.compile_expr(condition);
                let condition = self.bool_slot(condition);
                let dominating_context_reads = self.context_reads.clone();
                let dominating_param_reads = self.param_reads.clone();
                let false_jump = self.emit_jump(Instruction::JumpIfFalse {
                    condition,
                    target: usize::MAX,
                });
                self.context_reads = dominating_context_reads.clone();
                self.param_reads = dominating_param_reads.clone();
                self.compile_block(then_block);
                if let Some(else_block) = else_block {
                    let end_jump = self.emit_jump(Instruction::Jump(usize::MAX));
                    self.patch_jump(false_jump, self.current_target());
                    self.context_reads = dominating_context_reads.clone();
                    self.param_reads = dominating_param_reads.clone();
                    self.compile_block(else_block);
                    self.patch_jump(end_jump, self.current_target());
                } else {
                    self.patch_jump(false_jump, self.current_target());
                }
                self.context_reads = dominating_context_reads;
                self.param_reads = dominating_param_reads;
            }
            CheckedStmt::For {
                initializer,
                condition,
                update,
                body,
            } => {
                let Some(iterations) = super::loop_bounds::fixed_for_iterations(
                    &initializer,
                    &condition,
                    &update,
                    &body,
                ) else {
                    self.invalid_loop = true;
                    return;
                };
                self.scopes.push(IndexMap::new());
                self.compile_statement(*initializer);
                let count = self.allocate_slot(&Type::Int);
                self.emit_constant(count, Value::Int(iterations as i32));
                let Some((id, loop_start)) =
                    self.emit_range_start(count, donder_runtime::MAX_DSL_LOOP_ITERATIONS as i32)
                else {
                    return;
                };
                let dominating_context_reads = self.context_reads.clone();
                let dominating_param_reads = self.param_reads.clone();
                self.compile_block(body);
                self.compile_statement(*update);
                self.finish_loop(id, loop_start);
                self.context_reads = dominating_context_reads;
                self.param_reads = dominating_param_reads;
                let _ = self.scopes.pop();
            }
            CheckedStmt::ForMarks { index, marks, body } => {
                self.scopes.push(IndexMap::new());
                let source = self.compile_expr(marks);
                let snapshot = self.allocate_slot(&Type::Marks);
                self.emit(Instruction::Move {
                    dst: snapshot,
                    src: source.index(),
                });
                let index_slot = self.allocate_local(index, &Type::Int);
                self.emit_constant(index_slot, Value::Int(0));
                let one_slot = self.allocate_slot(&Type::Int);
                self.emit_constant(one_slot, Value::Int(1));
                let Some((id, loop_start)) = self.emit_marks_start(self.marks_slot(snapshot))
                else {
                    return;
                };
                let dominating_context_reads = self.context_reads.clone();
                let dominating_param_reads = self.param_reads.clone();
                self.compile_block(body);
                self.emit(Instruction::IntArithmetic {
                    dst: self.int_slot(index_slot),
                    op: IntArithmeticOp::Add,
                    left: self.int_slot(index_slot),
                    right: self.int_slot(one_slot),
                });
                self.finish_loop(id, loop_start);
                self.context_reads = dominating_context_reads;
                self.param_reads = dominating_param_reads;
                let _ = self.scopes.pop();
            }
            CheckedStmt::ForRange {
                index,
                count,
                cap,
                body,
            } => {
                self.scopes.push(IndexMap::new());
                let source = self.compile_expr(count);
                let count = self.allocate_slot(&Type::Int);
                self.emit(Instruction::Move {
                    dst: count,
                    src: source.index(),
                });
                let CheckedExprKind::Literal(Value::Int(cap)) = cap.kind else {
                    self.invalid_loop = true;
                    return;
                };
                let index_slot = self.allocate_local(index, &Type::Int);
                self.emit_constant(index_slot, Value::Int(0));
                let one_slot = self.allocate_slot(&Type::Int);
                self.emit_constant(one_slot, Value::Int(1));
                let Some((id, loop_start)) = self.emit_range_start(count, cap) else {
                    return;
                };
                let dominating_context_reads = self.context_reads.clone();
                let dominating_param_reads = self.param_reads.clone();
                self.compile_block(body);
                self.emit(Instruction::IntArithmetic {
                    dst: self.int_slot(index_slot),
                    op: IntArithmeticOp::Add,
                    left: self.int_slot(index_slot),
                    right: self.int_slot(one_slot),
                });
                self.finish_loop(id, loop_start);
                self.context_reads = dominating_context_reads;
                self.param_reads = dominating_param_reads;
                let _ = self.scopes.pop();
            }
            CheckedStmt::Emit { effect, .. } => {
                self.invalid_emission = Some(effect.span);
            }
            CheckedStmt::Return(expr) => {
                let value = self.compile_expr(expr);
                self.emit(Instruction::ReturnColor(self.color_slot(value)));
            }
        }
    }

    fn compile_expr(&mut self, expr: CheckedExpr) -> ValueSlot {
        let result_ty = expr.ty.clone();
        match expr.kind {
            CheckedExprKind::Literal(value) => {
                let dst = self.allocate_slot(&result_ty);
                self.emit_constant(dst, value);
                dst
            }
            CheckedExprKind::Variable(name) => match self.lookup(&name) {
                Some(Binding::Param(slot)) => {
                    if !self.assigned_names.contains(&name)
                        && let Some(cached) = self.param_reads.get(&slot)
                    {
                        return *cached;
                    }
                    let dst = self.allocate_slot(&result_ty);
                    self.emit_load_param(dst, slot);
                    if !self.assigned_names.contains(&name) {
                        self.param_reads.insert(slot, dst);
                    }
                    dst
                }
                Some(Binding::Local(slot)) => slot,
                None => {
                    let value = match name.as_str() {
                        "PI" => Value::Float(std::f32::consts::PI),
                        "TAU" => Value::Float(std::f32::consts::TAU),
                        _ => Value::Enum(name),
                    };
                    let dst = self.allocate_slot(&result_ty);
                    self.emit_constant(dst, value);
                    dst
                }
            },
            CheckedExprKind::Array(items) => {
                if let Some(values) = items
                    .iter()
                    .map(constant_array_item)
                    .collect::<Option<Vec<_>>>()
                {
                    let dst = self.allocate_slot(&result_ty);
                    self.emit_constant(dst, Value::Array(values.into()));
                    return dst;
                }
                let item_slots = items
                    .into_iter()
                    .map(|item| self.compile_expr(item))
                    .collect::<Vec<_>>();
                let item_slots = self.add_value_operands(item_slots);
                let dst = self.allocate_slot(&result_ty);
                let dst = self.array_slot(dst);
                self.emit(Instruction::MakeArray {
                    dst,
                    items: item_slots,
                });
                ValueSlot::Array(dst)
            }
            CheckedExprKind::Index { target, index } => {
                if let Some(param) = self.param_binding(&target, &Type::Curve) {
                    let position = self.float_slot_from_expr(*index);
                    let slot = self.allocate_slot(&result_ty);
                    let dst = self.float_slot(slot);
                    self.emit(Instruction::CurveParamSample {
                        dst,
                        param,
                        source: CurveSlot(self.parameter_bank_index(param)),
                        position,
                    });
                    ValueSlot::Float(dst)
                } else if let Some(param) = self.param_binding(&target, &Type::Gradient) {
                    let position = self.float_slot_from_expr(*index);
                    let slot = self.allocate_slot(&result_ty);
                    let dst = self.color_slot(slot);
                    self.emit(Instruction::GradientParamSample {
                        dst,
                        param,
                        source: GradientSlot(self.parameter_bank_index(param)),
                        position,
                    });
                    ValueSlot::Color(dst)
                } else {
                    let target = self.compile_expr(*target);
                    let index = self.compile_expr(*index);
                    let dst = self.allocate_slot(&result_ty);
                    match target {
                        ValueSlot::TargetItems(source) => {
                            self.emit(Instruction::TargetPick {
                                dst: self.target_item_slot(dst),
                                source,
                                index: Self::number_slot(index),
                            });
                        }
                        ValueSlot::Curve(curve) => {
                            let position = self.float_slot(index);
                            let dst = self.float_slot(dst);
                            self.emit(Instruction::CurveSample {
                                dst,
                                curve,
                                position,
                            });
                        }
                        ValueSlot::Gradient(gradient) => {
                            let position = self.float_slot(index);
                            self.emit(Instruction::GradientSample {
                                dst: self.color_slot(dst),
                                gradient,
                                position,
                            });
                        }
                        target => {
                            let target = self.array_slot(target);
                            let default = self.allocate_slot(&result_ty);
                            self.emit_default(default, &result_ty);
                            self.emit(Instruction::Index {
                                dst,
                                target,
                                index: Self::number_slot(index),
                                default: default.index(),
                            });
                        }
                    }
                    dst
                }
            }
            CheckedExprKind::Member { target, member } => {
                let target = self.compile_expr(*target);
                let target = self.target_item_slot(target);
                let dst = self.allocate_slot(&result_ty);
                if member.as_str() == "pixel_fraction" {
                    let dst = self.float_slot(dst);
                    self.emit(Instruction::MemberFraction { dst, target });
                } else {
                    self.emit(Instruction::MemberInt {
                        dst: self.int_slot(dst),
                        target,
                        member: target_member(&member),
                    });
                }
                dst
            }
            CheckedExprKind::Call { callee, args } => {
                let CheckedExprKind::Variable(name) = callee.kind else {
                    let dst = self.allocate_slot(&result_ty);
                    self.emit_default(dst, &Type::Void);
                    return dst;
                };
                self.compile_builtin_call(name, args, result_ty)
            }
            CheckedExprKind::SignalSample {
                input,
                seconds,
                pixel,
            } => {
                let seconds = self.float_slot_from_expr(*seconds);
                let pixel = pixel.map(|expr| {
                    let slot = self.compile_expr(*expr);
                    self.int_slot(slot)
                });
                let dst = self.allocate_slot(&Type::Color);
                let input = self
                    .signal_inputs
                    .get(&input)
                    .copied()
                    .unwrap_or_else(|| unreachable!("checked Signal input exists"));
                self.emit(Instruction::SignalSample {
                    capability: (),
                    dst: self.color_slot(dst),
                    input,
                    seconds,
                    pixel,
                    frame_cache: u32::MAX,
                });
                dst
            }
            CheckedExprKind::Unary { op, expr } => {
                let src = self.compile_expr(*expr);
                let dst = self.allocate_slot(&result_ty);
                match op {
                    UnaryOp::Not => self.emit(Instruction::Not {
                        dst: self.bool_slot(dst),
                        src: self.bool_slot(src),
                    }),
                    UnaryOp::Negate => match (dst, src) {
                        (ValueSlot::Int(dst), ValueSlot::Int(src)) => {
                            self.emit(Instruction::NegInt { dst, src })
                        }
                        (ValueSlot::Float(dst), src) => {
                            let src = self.float_slot(src);
                            self.emit(Instruction::NegFloat { dst, src });
                        }
                        _ => {}
                    },
                }
                dst
            }
            CheckedExprKind::Binary { op, left, right } => {
                if let Some((constant_left, constant)) =
                    float_const_operand(op, &result_ty, &left, &right)
                {
                    return self.compile_float_const_binary(
                        op,
                        *left,
                        *right,
                        result_ty,
                        constant_left,
                        constant,
                    );
                }
                match op {
                    BinaryOp::And => self.compile_short_circuit(false, *left, *right, result_ty),
                    BinaryOp::Or => self.compile_short_circuit(true, *left, *right, result_ty),
                    BinaryOp::Equal | BinaryOp::NotEqual => {
                        if let Some(dst) = self.compile_enum_param_const_equal(op, &left, &right) {
                            dst
                        } else {
                            let left = self.compile_expr(*left);
                            let right = self.compile_expr(*right);
                            let dst = self.allocate_slot(&result_ty);
                            self.emit_binary(dst, op, left, right);
                            dst
                        }
                    }
                    _ => {
                        let left = self.compile_expr(*left);
                        let right = self.compile_expr(*right);
                        let dst = self.allocate_slot(&result_ty);
                        self.emit_binary(dst, op, left, right);
                        dst
                    }
                }
            }
        }
    }

    fn compile_float_const_binary(
        &mut self,
        op: BinaryOp,
        left: CheckedExpr,
        right: CheckedExpr,
        result_ty: Type,
        constant_left: bool,
        constant: f32,
    ) -> ValueSlot {
        let value = if constant_left { right } else { left };
        let value = self.float_slot_from_expr(value);
        let dst = self.allocate_slot(&result_ty);
        match op {
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Remainder => {
                let dst = self.float_slot(dst);
                self.emit(Instruction::FloatArithmeticConst {
                    dst,
                    op: arithmetic_op(op),
                    value,
                    constant_bits: constant.to_bits(),
                    constant_left,
                });
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                self.emit(Instruction::FloatCompareConst {
                    dst: self.bool_slot(dst),
                    op: compare_op(op),
                    value,
                    constant_bits: constant.to_bits(),
                    constant_left,
                })
            }
            _ => unreachable!("float constant binary operation is arithmetic or comparison"),
        }
        dst
    }

    fn compile_short_circuit(
        &mut self,
        jump_when_true: bool,
        left: CheckedExpr,
        right: CheckedExpr,
        result_ty: Type,
    ) -> ValueSlot {
        let dst = self.allocate_slot(&result_ty);
        let left = self.compile_expr(left);
        self.emit(Instruction::Move {
            dst,
            src: left.index(),
        });
        let dominating_context_reads = self.context_reads.clone();
        let dominating_param_reads = self.param_reads.clone();
        let condition = self.bool_slot(dst);
        let jump = if jump_when_true {
            self.emit_jump(Instruction::JumpIfTrue {
                condition,
                target: usize::MAX,
            })
        } else {
            self.emit_jump(Instruction::JumpIfFalse {
                condition,
                target: usize::MAX,
            })
        };
        let right = self.compile_expr(right);
        self.emit(Instruction::Move {
            dst,
            src: right.index(),
        });
        self.patch_jump(jump, self.current_target());
        self.context_reads = dominating_context_reads;
        self.param_reads = dominating_param_reads;
        dst
    }

    fn compile_builtin_call(
        &mut self,
        name: Identifier,
        args: Vec<CheckedExpr>,
        result_ty: Type,
    ) -> ValueSlot {
        if let Some(read) = context_read(&name) {
            if let Some(slot) = self.context_reads.get(&read) {
                return *slot;
            }
            let dst = self.allocate_slot(&result_ty);
            self.emit_context_read(dst, read);
            self.context_reads.insert(read, dst);
            return dst;
        }
        let dst = self.allocate_slot(&result_ty);
        match name.as_str() {
            "progress" => self.emit_context_read(dst, ContextRead::Progress),
            "seconds" => self.emit_context_read(dst, ContextRead::Seconds),
            "duration" => self.emit_context_read(dst, ContextRead::Duration),
            "pixel_index" => self.emit_context_read(dst, ContextRead::PixelIndex),
            "pixel_count" => self.emit_context_read(dst, ContextRead::PixelCount),
            "pixel_fraction" => self.emit_context_read(dst, ContextRead::PixelFraction),
            "section_position" => {
                let args = self.compile_float_args(args);
                let dst = self.float_slot(dst);
                self.emit(Instruction::SectionPosition {
                    dst,
                    width: args[0],
                });
            }
            "sin" | "cos" | "abs" | "floor" => {
                let args = self.compile_float_args(args);
                let dst = self.float_slot(dst);
                self.emit(Instruction::FloatUnary {
                    dst,
                    op: match name.as_str() {
                        "sin" => FloatUnary::Sin,
                        "cos" => FloatUnary::Cos,
                        "abs" => FloatUnary::Abs,
                        "floor" => FloatUnary::Floor,
                        _ => unreachable!("matched float unary builtin"),
                    },
                    value: args[0],
                });
            }
            "min" => {
                let dst = self.float_slot(dst);
                if let Some((constant_index, constant)) = numeric_literal_argument(&args) {
                    let mut args = args;
                    let value = args.remove(if constant_index == 0 { 1 } else { 0 });
                    let value = self.float_slot_from_expr(value);
                    self.emit(Instruction::FloatBinaryConst {
                        dst,
                        op: FloatBinary::Min,
                        value,
                        constant_bits: constant.to_bits(),
                    });
                } else {
                    let args = self.compile_float_args(args);
                    self.emit(Instruction::FloatBinary {
                        dst,
                        op: FloatBinary::Min,
                        left: args[0],
                        right: args[1],
                    });
                }
            }
            "max" if result_ty == Type::Color => {
                let args = self.compile_args(args);
                self.emit(Instruction::ColorBinary {
                    dst: self.color_slot(dst),
                    op: ColorBinary::Max,
                    left: self.color_slot(args[0]),
                    right: self.color_slot(args[1]),
                });
            }
            "max" => {
                let dst = self.float_slot(dst);
                if let Some((constant_index, constant)) = numeric_literal_argument(&args) {
                    let mut args = args;
                    let value = args.remove(if constant_index == 0 { 1 } else { 0 });
                    let value = self.float_slot_from_expr(value);
                    self.emit(Instruction::FloatBinaryConst {
                        dst,
                        op: FloatBinary::Max,
                        value,
                        constant_bits: constant.to_bits(),
                    });
                } else {
                    let args = self.compile_float_args(args);
                    self.emit(Instruction::FloatBinary {
                        dst,
                        op: FloatBinary::Max,
                        left: args[0],
                        right: args[1],
                    });
                }
            }
            "hue" | "saturation" | "intensity" => {
                let args = self.compile_args(args);
                let dst = self.float_slot(dst);
                let op = match name.as_str() {
                    "hue" => ColorComponent::Hue,
                    "saturation" => ColorComponent::Saturation,
                    "intensity" => ColorComponent::Intensity,
                    _ => unreachable!(),
                };
                self.emit(Instruction::ColorComponent {
                    dst,
                    op,
                    color: self.color_slot(args[0]),
                });
            }
            "invert" => {
                let args = self.compile_args(args);
                self.emit(Instruction::ColorInvert {
                    dst: self.color_slot(dst),
                    color: self.color_slot(args[0]),
                });
            }
            "clamp" => {
                let dst = self.float_slot(dst);
                let bounds = args
                    .get(1)
                    .and_then(numeric_literal)
                    .zip(args.get(2).and_then(numeric_literal));
                if let Some((min, max)) = bounds {
                    let mut args = args;
                    let value = self.float_slot_from_expr(args.remove(0));
                    self.emit(Instruction::ClampConst {
                        dst,
                        value,
                        min_bits: min.to_bits(),
                        max_bits: max.to_bits(),
                    });
                } else {
                    let args = self.compile_float_args(args);
                    self.emit(Instruction::Clamp {
                        dst,
                        value: args[0],
                        min: args[1],
                        max: args[2],
                    });
                }
            }
            "smoothstep" => {
                let args = self.compile_float_args(args);
                let dst = self.float_slot(dst);
                self.emit(Instruction::Smoothstep {
                    dst,
                    edge0: args[0],
                    edge1: args[1],
                    value: args[2],
                });
            }
            "mix" => {
                let mut args = self.compile_args(args);
                let left = args.remove(0);
                let right = args.remove(0);
                let amount = self.float_slot(args.remove(0));
                match dst {
                    ValueSlot::Float(dst) => {
                        let left = self.float_slot(left);
                        let right = self.float_slot(right);
                        self.emit(Instruction::MixFloat {
                            dst,
                            left,
                            right,
                            amount,
                        });
                    }
                    ValueSlot::Color(dst) => self.emit(Instruction::MixColor {
                        dst,
                        left: self.color_slot(left),
                        right: self.color_slot(right),
                        amount,
                    }),
                    _ => self.emit_default(dst, &Type::Void),
                }
            }
            "rgb" => {
                let args = self.compile_float_args(args);
                self.emit(Instruction::Rgb {
                    dst: self.color_slot(dst),
                    red: args[0],
                    green: args[1],
                    blue: args[2],
                });
            }
            "hsv" => {
                let args = self.compile_float_args(args);
                self.emit(Instruction::Hsv {
                    dst: self.color_slot(dst),
                    hue: args[0],
                    saturation: args[1],
                    value: args[2],
                });
            }
            "srand" | "rand" => {
                let args = self.compile_float_args(args);
                let dst = self.float_slot(dst);
                // Argument count and types are known here. Lower the seed fold
                // to ordinary float arithmetic, leaving one total scalar hash
                // in the VM. Preserve the original evaluation order, including
                // the initial +0.0 operation for signed zero and NaN inputs.
                if args.is_empty() {
                    self.emit_default(ValueSlot::Float(dst), &Type::Float);
                }
                for (index, arg) in args.into_iter().enumerate() {
                    if index == 0 {
                        self.emit(Instruction::FloatArithmeticConst {
                            dst,
                            op: ArithmeticOp::Add,
                            value: arg,
                            constant_bits: 0.0_f32.to_bits(),
                            constant_left: true,
                        });
                    } else {
                        self.emit(Instruction::FloatArithmeticConst {
                            dst,
                            op: ArithmeticOp::Multiply,
                            value: dst,
                            constant_bits: 31.0_f32.to_bits(),
                            constant_left: false,
                        });
                        self.emit(Instruction::FloatArithmetic {
                            dst,
                            op: ArithmeticOp::Add,
                            left: dst,
                            right: arg,
                        });
                    }
                }
                self.emit(Instruction::Rand { dst, seed: dst });
            }
            "curve_clamped" if args.len() == 4 => {
                if let Some(param) = self.param_binding(&args[0], &Type::Curve) {
                    let registers = self.compile_float_args(args.into_iter().skip(1).collect());
                    let dst = self.float_slot(dst);
                    self.emit(Instruction::CurveParamFloatClamped {
                        dst,
                        param,
                        source: CurveSlot(self.parameter_bank_index(param)),
                        position: registers[0],
                        min: registers[1],
                        max: registers[2],
                    });
                } else {
                    let mut args = args;
                    let curve_expr = args.remove(0);
                    let curve = self.compile_expr(curve_expr);
                    let curve = self.curve_slot(curve);
                    let registers = self.compile_float_args(args);
                    let dst = self.float_slot(dst);
                    self.emit(Instruction::CurveFloatClamped {
                        dst,
                        curve,
                        position: registers[0],
                        min: registers[1],
                        max: registers[2],
                    });
                }
            }
            "gradient_color_scaled" if args.len() == 3 => {
                if let Some(param) = self.param_binding(&args[0], &Type::Gradient) {
                    let registers = self.compile_float_args(args.into_iter().skip(1).collect());
                    let dst = self.color_slot(dst);
                    self.emit(Instruction::GradientParamColorScaled {
                        dst,
                        param,
                        source: GradientSlot(self.parameter_bank_index(param)),
                        position: registers[0],
                        scale: registers[1],
                    });
                } else {
                    let mut args = args;
                    let gradient_expr = args.remove(0);
                    let gradient = self.compile_expr(gradient_expr);
                    let gradient = self.gradient_slot(gradient);
                    let registers = self.compile_float_args(args);
                    let dst = self.color_slot(dst);
                    self.emit(Instruction::GradientColorScaled {
                        dst,
                        gradient,
                        position: registers[0],
                        scale: registers[1],
                    });
                }
            }
            "curve_crossing" if args.len() == 2 || args.len() == 3 => {
                if let Some(param) = self.param_binding(&args[0], &Type::Curve) {
                    let registers = self.compile_float_args(args.into_iter().skip(1).collect());
                    let dst = self.float_slot(dst);
                    self.emit(Instruction::CurveParamCrossing {
                        dst,
                        param,
                        source: CurveSlot(self.parameter_bank_index(param)),
                        value: registers[0],
                        fallback: registers.get(1).copied(),
                    });
                } else {
                    let mut args = args;
                    let curve_expr = args.remove(0);
                    let curve = self.compile_expr(curve_expr);
                    let curve = self.curve_slot(curve);
                    let registers = self.compile_float_args(args);
                    let dst = self.float_slot(dst);
                    self.emit(Instruction::CurveCrossing {
                        dst,
                        curve,
                        value: registers[0],
                        fallback: registers.get(1).copied(),
                    });
                }
            }
            "len" => {
                let args = self.compile_args(args);
                match args[0] {
                    ValueSlot::Marks(marks) => self.emit(Instruction::Mark {
                        marks,
                        op: MarkOp::Count {
                            dst: self.int_slot(dst),
                        },
                    }),
                    slot => self.emit(Instruction::Len {
                        dst: self.int_slot(dst),
                        value: self.array_slot(slot),
                    }),
                }
            }
            "mark_count" | "mark_at" | "mark_prev" | "mark_prev_index" | "mark_next_index"
            | "mark_elapsed" | "mark_phase" => {
                let args = self.compile_args(args);
                let marks = self.marks_slot(args[0]);
                let mut time = args.get(1).copied().map(Self::number_slot);
                if time.is_none()
                    && self.kind == EffectKind::Generator
                    && name.as_str() != "mark_count"
                {
                    // A generator's omitted query time is its preparation-time
                    // origin, not the playback clock of a retained calculation.
                    // Keep that source context explicit when code is staged.
                    let zero = self.allocate_slot(&Type::Float);
                    self.emit_default(zero, &Type::Float);
                    time = Some(Self::number_slot(zero));
                }
                let fallback = || args.get(2).copied().map(Self::number_slot);
                let op = match name.as_str() {
                    "mark_count" => MarkOp::Count {
                        dst: self.int_slot(dst),
                    },
                    "mark_at" => MarkOp::At {
                        dst: self.float_slot(dst),
                        index: Self::number_slot(args[1]),
                        fallback: fallback(),
                    },
                    "mark_prev" => MarkOp::Prev {
                        dst: self.float_slot(dst),
                        seconds: time,
                        fallback: fallback(),
                    },
                    "mark_prev_index" => MarkOp::PrevIndex {
                        dst: self.int_slot(dst),
                        seconds: time,
                    },
                    "mark_next_index" => MarkOp::NextIndex {
                        dst: self.int_slot(dst),
                        seconds: time,
                    },
                    "mark_elapsed" => MarkOp::Elapsed {
                        dst: self.float_slot(dst),
                        seconds: time,
                    },
                    "mark_phase" => MarkOp::Phase {
                        dst: self.float_slot(dst),
                        seconds: time,
                    },
                    _ => unreachable!("matched mark builtin"),
                };
                self.emit(Instruction::Mark { marks, op });
            }
            "count" => {
                let args = self.compile_args(args);
                self.emit(Instruction::TargetCount {
                    dst: self.int_slot(dst),
                    source: self.target_items_slot(args[0]),
                });
            }
            "pick" => {
                let args = self.compile_args(args);
                self.emit(Instruction::TargetPick {
                    dst: self.target_item_slot(dst),
                    source: self.target_items_slot(args[0]),
                    index: Self::number_slot(args[1]),
                });
            }
            "fixtures" | "pixels" | "sections" => {
                let args = self.compile_args(args);
                let source = Self::target_source(args[0]);
                let dst = self.target_items_slot(dst);
                let op = match name.as_str() {
                    "fixtures" => TargetItemsOp::Fixtures { dst },
                    "pixels" => TargetItemsOp::Pixels { dst },
                    "sections" => TargetItemsOp::Sections {
                        dst,
                        width: Self::number_slot(args[1]),
                    },
                    _ => unreachable!("matched target builtin"),
                };
                self.emit(Instruction::TargetItems { source, op });
            }
            _ => self.emit_default(dst, &Type::Void),
        }
        dst
    }

    fn compile_args(&mut self, args: Vec<CheckedExpr>) -> Vec<ValueSlot> {
        args.into_iter().map(|arg| self.compile_expr(arg)).collect()
    }

    fn compile_float_args(&mut self, args: Vec<CheckedExpr>) -> Vec<FloatSlot> {
        args.into_iter()
            .map(|arg| self.float_slot_from_expr(arg))
            .collect()
    }

    fn float_slot_from_expr(&mut self, expr: CheckedExpr) -> FloatSlot {
        let slot = self.compile_expr(expr);
        self.float_slot(slot)
    }

    fn emit_load_param(&mut self, dst: ValueSlot, param: ParamId) {
        let source = self.parameter_bank_index(param);
        match dst {
            ValueSlot::Target(dst) => self.emit(Instruction::LoadTargetParam {
                dst,
                param,
                source: TargetSlot(source),
            }),
            ValueSlot::TargetItems(dst) => self.emit(Instruction::LoadTargetItemsParam {
                dst,
                param,
                source: TargetItemsSlot(source),
            }),
            ValueSlot::TargetItem(dst) => self.emit(Instruction::LoadTargetItemParam {
                dst,
                param,
                source: TargetItemSlot(source),
            }),
            ValueSlot::Curve(dst) => self.emit(Instruction::LoadCurveParam {
                dst,
                param,
                source: CurveSlot(source),
            }),
            ValueSlot::Gradient(dst) => self.emit(Instruction::LoadGradientParam {
                dst,
                param,
                source: GradientSlot(source),
            }),
            ValueSlot::Marks(dst) => self.emit(Instruction::LoadMarksParam {
                dst,
                param,
                source: MarksSlot(source),
            }),
            ValueSlot::Int(dst) => self.emit(Instruction::LoadIntParam {
                dst,
                param,
                source: IntSlot(source),
            }),
            ValueSlot::Float(dst) => self.emit(Instruction::LoadFloatParam {
                dst,
                param,
                source: FloatSlot(source),
            }),
            ValueSlot::Bool(dst) => self.emit(Instruction::LoadBoolParam {
                dst,
                param,
                source: BoolSlot(source),
            }),
            ValueSlot::Color(dst) => self.emit(Instruction::LoadColorParam {
                dst,
                param,
                source: ColorSlot(source),
            }),
            ValueSlot::Enum(dst) => self.emit(Instruction::LoadEnumParam {
                dst,
                param,
                source: EnumSlot(self.parameter_bank_index(param)),
            }),
            ValueSlot::Array(dst) => self.emit(Instruction::LoadArrayParam {
                dst,
                param,
                source: ArraySlot(self.parameter_bank_index(param)),
            }),
            ValueSlot::Void => {}
        }
    }

    fn parameter_bank_index(&self, param: ParamId) -> u32 {
        self.param_types[..param]
            .iter()
            .filter(|ty| {
                super::bytecode::ParameterKind::for_type(ty)
                    == super::bytecode::ParameterKind::for_type(&self.param_types[param])
            })
            .count() as u32
    }

    fn emit_context_read(&mut self, dst: ValueSlot, read: ContextRead) {
        self.emit(Instruction::ContextRead {
            dst: Self::number_slot(dst),
            read,
        });
    }

    fn emit_binary(&mut self, dst: ValueSlot, op: BinaryOp, left: ValueSlot, right: ValueSlot) {
        match op {
            BinaryOp::Add if matches!(dst, ValueSlot::Color(_)) => {
                self.emit(Instruction::ColorBinary {
                    dst: self.color_slot(dst),
                    op: ColorBinary::Add,
                    left: self.color_slot(left),
                    right: self.color_slot(right),
                });
            }
            BinaryOp::Multiply if matches!(dst, ValueSlot::Color(_)) => match (left, right) {
                (ValueSlot::Color(left), ValueSlot::Color(right)) => {
                    self.emit(Instruction::ColorBinary {
                        dst: self.color_slot(dst),
                        op: ColorBinary::Multiply,
                        left,
                        right,
                    });
                }
                (ValueSlot::Color(color), scale) | (scale, ValueSlot::Color(color)) => {
                    let scale = self.float_slot(scale);
                    self.emit(Instruction::ColorScale {
                        dst: self.color_slot(dst),
                        color,
                        scale,
                    });
                }
                _ => unreachable!("checked color multiplication"),
            },
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Remainder => match dst {
                ValueSlot::Float(dst) => {
                    let left = self.float_slot(left);
                    let right = self.float_slot(right);
                    self.emit(Instruction::FloatArithmetic {
                        dst,
                        op: arithmetic_op(op),
                        left,
                        right,
                    });
                }
                ValueSlot::Int(dst) => self.emit(Instruction::IntArithmetic {
                    dst,
                    op: int_arithmetic_op(op),
                    left: self.int_slot(left),
                    right: self.int_slot(right),
                }),
                _ => unreachable!("checked arithmetic result is numeric"),
            },
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                match (left, right) {
                    (ValueSlot::Int(left), ValueSlot::Int(right)) => {
                        self.emit(Instruction::IntCompare {
                            dst: self.bool_slot(dst),
                            op: compare_op(op),
                            left,
                            right,
                        });
                    }
                    (left, right) => {
                        let left = self.float_slot(left);
                        let right = self.float_slot(right);
                        self.emit(Instruction::FloatCompare {
                            dst: self.bool_slot(dst),
                            op: compare_op(op),
                            left,
                            right,
                        });
                    }
                }
            }
            BinaryOp::Equal | BinaryOp::NotEqual => self.emit(Instruction::ValueEqual {
                dst: self.bool_slot(dst),
                negate: op == BinaryOp::NotEqual,
                left,
                right,
            }),
            BinaryOp::And | BinaryOp::Or => unreachable!("short-circuit binary operator"),
        }
    }

    fn compile_enum_param_const_equal(
        &mut self,
        op: BinaryOp,
        left: &CheckedExpr,
        right: &CheckedExpr,
    ) -> Option<ValueSlot> {
        let (param, constant) = self.enum_param_const_pair(left, right)?;
        let dst = self.allocate_slot(&Type::Bool);
        let bool_dst = self.bool_slot(dst);
        self.emit(Instruction::EnumParamEqualConst {
            dst: bool_dst,
            param,
            source: EnumSlot(self.parameter_bank_index(param)),
            constant,
            negate: op == BinaryOp::NotEqual,
        });
        Some(dst)
    }

    fn enum_param_const_pair(
        &mut self,
        left: &CheckedExpr,
        right: &CheckedExpr,
    ) -> Option<(ParamId, usize)> {
        if let Some(param) = self.enum_param_binding(left) {
            return self.enum_constant(right).map(|constant| (param, constant));
        }
        if let Some(param) = self.enum_param_binding(right) {
            return self.enum_constant(left).map(|constant| (param, constant));
        }
        None
    }

    fn enum_param_binding(&self, expr: &CheckedExpr) -> Option<ParamId> {
        let CheckedExprKind::Variable(name) = &expr.kind else {
            return None;
        };
        let Some(Binding::Param(param)) = self.lookup(name) else {
            return None;
        };
        match self.param_types.get(param) {
            Some(Type::Enum(_)) => Some(param),
            _ => None,
        }
    }

    fn enum_constant(&mut self, expr: &CheckedExpr) -> Option<usize> {
        let CheckedExprKind::Variable(name) = &expr.kind else {
            return None;
        };
        if self.lookup(name).is_some() || !matches!(expr.ty, Type::Enum(_)) {
            return None;
        }
        let index = self.enums.len();
        self.enums.push(name.clone());
        Some(index)
    }

    fn param_binding(&self, expr: &CheckedExpr, expected: &Type) -> Option<ParamId> {
        let CheckedExprKind::Variable(name) = &expr.kind else {
            return None;
        };
        let Some(Binding::Param(param)) = self.lookup(name) else {
            return None;
        };
        (self.param_types.get(param) == Some(expected)).then_some(param)
    }

    fn coerce_slot(&mut self, slot: ValueSlot, target: &Type) -> ValueSlot {
        if target == &Type::Float {
            return ValueSlot::Float(self.float_slot(slot));
        }
        slot
    }

    fn coerce_to_slot(&mut self, src: ValueSlot, dst: ValueSlot) -> ValueSlot {
        match (dst, src) {
            (ValueSlot::Float(_), src) => ValueSlot::Float(self.float_slot(src)),
            _ => src,
        }
    }

    fn allocate_local(&mut self, name: Identifier, ty: &Type) -> LocalId {
        let slot = self.allocate_slot(ty);
        self.bind_local(name, slot);
        slot
    }

    fn bind_local(&mut self, name: Identifier, slot: LocalId) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, Binding::Local(slot));
        }
    }

    fn initializer_can_bind_directly(&self, initializer: &CheckedExpr) -> bool {
        let CheckedExprKind::Variable(name) = &initializer.kind else {
            return true;
        };
        match self.lookup(name) {
            Some(Binding::Local(_)) => !self.assigned_names.contains(name),
            Some(Binding::Param(_)) | None => true,
        }
    }

    fn allocate_slot(&mut self, ty: &Type) -> ValueSlot {
        let slot = ValueSlot::for_type(ty, &mut self.layout);
        if let ValueSlot::Enum(_) = slot {
            self.enum_types.push(ty.clone());
        }
        if let ValueSlot::Array(index) = slot {
            self.array_types.push(ty.clone());
            debug_assert_eq!(index.0 as usize, self.array_types.len() - 1);
        }
        slot
    }

    fn number_slot(slot: ValueSlot) -> NumberSlot {
        match slot {
            ValueSlot::Int(slot) => NumberSlot::Int(slot),
            ValueSlot::Float(slot) => NumberSlot::Float(slot),
            _ => unreachable!("typechecked numeric operand"),
        }
    }

    fn int_slot(&self, slot: ValueSlot) -> IntSlot {
        match slot {
            ValueSlot::Int(slot) => slot,
            _ => unreachable!("checked expression is int"),
        }
    }

    fn float_slot(&mut self, slot: ValueSlot) -> FloatSlot {
        match slot {
            ValueSlot::Float(slot) => slot,
            ValueSlot::Int(src) => {
                let dst = match self.allocate_slot(&Type::Float) {
                    ValueSlot::Float(slot) => slot,
                    _ => unreachable!("float allocation"),
                };
                self.emit(Instruction::IntToFloat { dst, src });
                dst
            }
            _ => unreachable!("checked expression is numeric"),
        }
    }

    fn bool_slot(&self, slot: ValueSlot) -> BoolSlot {
        match slot {
            ValueSlot::Bool(slot) => slot,
            _ => unreachable!("checked expression is bool"),
        }
    }

    fn color_slot(&self, slot: ValueSlot) -> ColorSlot {
        match slot {
            ValueSlot::Color(slot) => slot,
            _ => unreachable!("checked expression is color"),
        }
    }

    fn array_slot(&self, slot: ValueSlot) -> ArraySlot {
        match slot {
            ValueSlot::Array(slot) => slot,
            _ => unreachable!("checked expression is reference-like"),
        }
    }

    fn target_items_slot(&self, slot: ValueSlot) -> TargetItemsSlot {
        match slot {
            ValueSlot::TargetItems(slot) => slot,
            _ => unreachable!("checked target collection"),
        }
    }

    fn target_item_slot(&self, slot: ValueSlot) -> TargetItemSlot {
        match slot {
            ValueSlot::TargetItem(slot) => slot,
            _ => unreachable!("checked target item"),
        }
    }

    fn target_source(slot: ValueSlot) -> TargetSource {
        match slot {
            ValueSlot::Target(slot) => TargetSource::Target(slot),
            ValueSlot::TargetItems(slot) => TargetSource::Items(slot),
            ValueSlot::TargetItem(slot) => TargetSource::Item(slot),
            _ => unreachable!("checked target operand"),
        }
    }

    fn marks_slot(&self, slot: ValueSlot) -> MarksSlot {
        match slot {
            ValueSlot::Marks(slot) => slot,
            _ => unreachable!("checked marks operand"),
        }
    }

    fn curve_slot(&self, slot: ValueSlot) -> CurveSlot {
        match slot {
            ValueSlot::Curve(slot) => slot,
            _ => unreachable!("checked curve operand"),
        }
    }

    fn gradient_slot(&self, slot: ValueSlot) -> GradientSlot {
        match slot {
            ValueSlot::Gradient(slot) => slot,
            _ => unreachable!("checked gradient operand"),
        }
    }

    fn lookup(&self, name: &Identifier) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn add_array_constant(&mut self, value: std::sync::Arc<[Value]>) -> usize {
        self.array_constants.push(value);
        self.array_constants.len() - 1
    }

    fn emit_default(&mut self, dst: ValueSlot, ty: &Type) {
        self.emit_constant(dst, ty.default_value());
    }

    fn emit_constant(&mut self, dst: ValueSlot, value: Value) {
        if matches!((dst, &value), (ValueSlot::Void, Value::Void)) {
            return;
        }
        let instruction = match (dst, value) {
            (ValueSlot::TargetItem(dst), Value::TargetItem(value)) => {
                let constant = self.target_items.len();
                self.target_items.push(value);
                Instruction::LoadTargetItemConst { dst, constant }
            }
            (ValueSlot::TargetItems(dst), Value::TargetItems(value)) => {
                let constant = self.target_lists.len();
                self.target_lists.push(value);
                Instruction::LoadTargetItemsConst { dst, constant }
            }
            (ValueSlot::Target(dst), Value::Target(value)) => {
                let constant = self.targets.len();
                self.targets.push(value);
                Instruction::LoadTargetConst { dst, constant }
            }
            (ValueSlot::Curve(dst), Value::Curve(value)) => {
                let constant = self.curves.len();
                self.curves.push(value);
                Instruction::LoadCurveConst { dst, constant }
            }
            (ValueSlot::Gradient(dst), Value::Gradient(value)) => {
                let constant = self.gradients.len();
                self.gradients.push(value);
                Instruction::LoadGradientConst { dst, constant }
            }
            (ValueSlot::Marks(dst), Value::Marks(value)) => {
                Instruction::LoadMarksConst { dst, value }
            }
            (ValueSlot::Int(dst), Value::Int(value)) => Instruction::LoadIntConst { dst, value },
            (ValueSlot::Float(dst), Value::Float(value)) => Instruction::LoadFloatConst {
                dst,
                bits: value.to_bits(),
            },
            (ValueSlot::Float(dst), Value::Int(value)) => Instruction::LoadFloatConst {
                dst,
                bits: (value as f32).to_bits(),
            },
            (ValueSlot::Bool(dst), Value::Bool(value)) => Instruction::LoadBoolConst { dst, value },
            (ValueSlot::Color(dst), Value::Color(value)) => {
                Instruction::LoadColorConst { dst, value }
            }
            (ValueSlot::Enum(dst), Value::Enum(value)) => {
                let constant = self.enums.len();
                self.enums.push(value);
                Instruction::LoadEnumConst { dst, constant }
            }
            (ValueSlot::Array(dst), Value::Array(value)) => Instruction::LoadArrayConst {
                dst,
                constant: self.add_array_constant(value),
            },
            _ => unreachable!("checked constant matches its destination"),
        };
        self.emit(instruction);
    }

    fn add_value_operands(&mut self, values: Vec<ValueSlot>) -> PoolSpan {
        let span = pool_span(self.value_operands.len(), values.len());
        self.value_operands.extend(values);
        span
    }

    fn current_target(&self) -> Target {
        self.instructions.len()
    }

    fn emit(&mut self, instruction: Instruction) {
        self.instructions.push(instruction);
    }

    fn allocate_loop_id(&mut self) -> Option<u32> {
        let Some(next) = self.loop_count.checked_add(1) else {
            self.invalid_loop = true;
            return None;
        };
        let id = self.loop_count;
        self.loop_count = next;
        Some(id)
    }

    fn emit_range_start(&mut self, count: ValueSlot, cap: i32) -> Option<(u32, usize)> {
        if cap <= 0 || cap as usize > donder_runtime::MAX_DSL_LOOP_ITERATIONS {
            self.invalid_loop = true;
            return None;
        }
        let id = self.allocate_loop_id()?;
        self.emit(Instruction::LoopRangeStart {
            id,
            count: self.int_slot(count),
            cap,
            end: usize::MAX,
        });
        Some((id, self.current_target()))
    }

    fn emit_marks_start(&mut self, marks: MarksSlot) -> Option<(u32, usize)> {
        let id = self.allocate_loop_id()?;
        self.emit(Instruction::LoopMarksStart {
            id,
            marks,
            end: usize::MAX,
        });
        Some((id, self.current_target()))
    }

    fn finish_loop(&mut self, id: u32, start: usize) {
        let end = self.current_target();
        self.emit(Instruction::LoopEnd { id, start });
        if let Some(
            Instruction::LoopRangeStart { end: target, .. }
            | Instruction::LoopMarksStart { end: target, .. },
        ) = start
            .checked_sub(1)
            .and_then(|index| self.instructions.get_mut(index))
        {
            *target = end;
        } else {
            self.invalid_loop = true;
        }
    }

    fn emit_jump(&mut self, instruction: Instruction) -> usize {
        let offset = self.instructions.len();
        self.instructions.push(instruction);
        offset
    }

    fn patch_jump(&mut self, offset: usize, target: Target) {
        match &mut self.instructions[offset] {
            Instruction::Jump(existing) => *existing = target,
            Instruction::JumpIfFalse {
                target: existing, ..
            }
            | Instruction::JumpIfTrue {
                target: existing, ..
            } => *existing = target,
            _ => {}
        }
    }
}

fn pool_span(start: usize, len: usize) -> PoolSpan {
    debug_assert!(u32::try_from(start).is_ok() && u32::try_from(len).is_ok());
    PoolSpan {
        start: start as u32,
        len: len as u32,
    }
}

fn target_member(member: &Identifier) -> TargetMember {
    match member.as_str() {
        "fixture_index" => TargetMember::FixtureIndex,
        "fixture_pixel_index" => TargetMember::FixturePixelIndex,
        "pixel_index" => TargetMember::PixelIndex,
        "pixel_count" => TargetMember::PixelCount,
        _ => unreachable!("checked TargetItem member is known"),
    }
}

fn arithmetic_op(op: BinaryOp) -> ArithmeticOp {
    match op {
        BinaryOp::Add => ArithmeticOp::Add,
        BinaryOp::Subtract => ArithmeticOp::Subtract,
        BinaryOp::Multiply => ArithmeticOp::Multiply,
        BinaryOp::Divide => ArithmeticOp::Divide,
        BinaryOp::Remainder => ArithmeticOp::Remainder,
        _ => unreachable!("arithmetic operator"),
    }
}

fn int_arithmetic_op(op: BinaryOp) -> IntArithmeticOp {
    match op {
        BinaryOp::Add => IntArithmeticOp::Add,
        BinaryOp::Subtract => IntArithmeticOp::Subtract,
        BinaryOp::Multiply => IntArithmeticOp::Multiply,
        BinaryOp::Remainder => IntArithmeticOp::Remainder,
        BinaryOp::Divide => unreachable!("int division compiles to float arithmetic"),
        _ => unreachable!("int arithmetic operator"),
    }
}

fn compare_op(op: BinaryOp) -> CompareOp {
    match op {
        BinaryOp::Less => CompareOp::Less,
        BinaryOp::LessEqual => CompareOp::LessEqual,
        BinaryOp::Greater => CompareOp::Greater,
        BinaryOp::GreaterEqual => CompareOp::GreaterEqual,
        _ => unreachable!("compare operator"),
    }
}

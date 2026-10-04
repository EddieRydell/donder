use super::ast::{BinaryOp, UnaryOp};
use super::bytecode::{
    ArraySlot, BoolSlot, BytecodeProgram, ColorBinary, ColorComponent, ColorSlot, CompareOp,
    ContextRead, CurveSlot, EnumSlot, EnumSlotType, FloatBinary, FloatSlot, FloatUnary,
    GradientSlot, Instruction, IntSlot, LocalId, MarkOp, MarksSlot, NumberSlot, ParamId, PoolSpan,
    SlotLayout, Target, ValueSlot,
};
use super::checked::{
    CheckedBlock, CheckedEffectDecl, CheckedExpr, CheckedExprKind, CheckedModule,
    CheckedOperatorDecl, CheckedStmt,
};
use super::types::{Identifier, Type, Value};
use super::{CompiledEffect, CompiledOperator};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

pub(crate) fn compile_checked_effects(
    module: CheckedModule,
) -> Result<Vec<CompiledEffect>, super::Diagnostic> {
    module.effects.into_iter().map(compile_effect).collect()
}

pub(crate) fn compile_checked_operators(
    module: CheckedModule,
) -> Result<Vec<CompiledOperator>, super::Diagnostic> {
    module.operators.into_iter().map(compile_operator).collect()
}

fn compile_effect(effect: CheckedEffectDecl) -> Result<CompiledEffect, super::Diagnostic> {
    check_parameter_count(&effect.params)?;
    let bytecode = FunctionCompiler::new(&effect.params).compile(effect.body)?;
    check_register_capacity(&bytecode)?;
    let program = super::SampleProgram::admit(
        bytecode,
        effect.params.iter().map(|param| param.ty.clone()).collect(),
    )
    .ok_or_else(invalid_compiled_program)?;
    Ok(CompiledEffect {
        name: effect.name,
        params: effect.params,
        program: std::sync::Arc::new(program),
    })
}

fn compile_operator(operator: CheckedOperatorDecl) -> Result<CompiledOperator, super::Diagnostic> {
    check_parameter_count(&operator.params)?;
    let bytecode = FunctionCompiler::new_operator(&operator.params, &operator.inputs)
        .compile(operator.body)?;
    check_register_capacity(&bytecode)?;
    CompiledOperator::admit(operator.name, operator.inputs, operator.params, bytecode)
        .ok_or_else(invalid_compiled_program)
}

fn invalid_compiled_program() -> super::Diagnostic {
    super::Diagnostic::new(
        super::lexer::TextSpan { start: 0, end: 0 },
        "compiler produced invalid bytecode",
    )
}

fn check_register_capacity(
    bytecode: &super::bytecode::BytecodeProgram,
) -> Result<(), super::Diagnostic> {
    let Some((bank, used, limit)) = bytecode.layout.exceeded_bank() else {
        return Ok(());
    };
    let bank = match bank {
        super::bytecode::PrimitiveBank::Float => "float",
        super::bytecode::PrimitiveBank::Int => "int",
        super::bytecode::PrimitiveBank::Bool => "bool",
    };
    Err(super::Diagnostic::new(
        super::lexer::TextSpan { start: 0, end: 0 },
        format!("program needs {used} {bank} registers; the limit is {limit}"),
    ))
}

/// Prepared automation addresses declaration slots with u16.
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
    curves: Vec<std::sync::Arc<crate::values::Curve>>,
    gradients: Vec<std::sync::Arc<crate::values::Gradient>>,
    value_operands: Vec<ValueSlot>,
    scopes: Vec<IndexMap<Identifier, Binding>>,
    param_types: Vec<Type>,
    layout: SlotLayout,
    array_types: Vec<Type>,
    signal_inputs: IndexMap<Identifier, usize>,
    assigned_names: HashSet<Identifier>,
    context_reads: HashMap<ContextRead, ValueSlot>,
    param_reads: HashMap<ParamId, ValueSlot>,
    loop_count: u32,
    invalid_loop: bool,
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
        CheckedStmt::Local { .. } | CheckedStmt::Expr(_) | CheckedStmt::Return(_) => {}
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
    fn new(params: &[super::ast::ParamDecl]) -> Self {
        let mut param_scope = IndexMap::new();
        for (index, param) in params.iter().enumerate() {
            param_scope.insert(param.name.clone(), Binding::Param(index));
        }
        Self {
            instructions: Vec::new(),
            array_constants: Vec::new(),
            enums: Vec::new(),
            enum_types: Vec::new(),
            curves: Vec::new(),
            gradients: Vec::new(),
            value_operands: Vec::new(),
            scopes: vec![param_scope],
            param_types: params.iter().map(|param| param.ty.clone()).collect(),
            layout: SlotLayout::default(),
            array_types: Vec::new(),
            signal_inputs: IndexMap::new(),
            assigned_names: HashSet::new(),
            context_reads: HashMap::new(),
            param_reads: HashMap::new(),
            loop_count: 0,
            invalid_loop: false,
        }
    }

    fn new_operator(
        params: &[super::ast::ParamDecl],
        inputs: &[super::ast::OperatorInputDecl],
    ) -> Self {
        let mut compiler = Self::new(params);
        compiler.signal_inputs = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| (input.name.clone(), index))
            .collect();
        compiler
    }

    fn compile(&mut self, block: CheckedBlock) -> Result<BytecodeProgram, super::Diagnostic> {
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
        let _ = self.scopes.pop();
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
        let pixel_entry = super::optimize::prepare_pixels(
            &mut self.instructions,
            &mut self.value_operands,
            &mut self.layout,
            &mut self.array_types,
        );
        let mut program = BytecodeProgram {
            pixel_entry,
            array_capacity: 0,
            array_width: 0,
            loop_count: super::optimize::compact_loops(&mut self.instructions),
            uses_pixel_context: false,
            instructions: std::mem::take(&mut self.instructions).into_boxed_slice(),
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
        program.uses_pixel_context = program.reads_pixel_context();
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
                let dominating_context_reads = self.context_reads.clone();
                let dominating_param_reads = self.param_reads.clone();
                let false_jumps = self.compile_condition(condition, false);
                self.context_reads = dominating_context_reads.clone();
                self.param_reads = dominating_param_reads.clone();
                self.compile_block(then_block);
                if let Some(else_block) = else_block {
                    let end_jump = self.emit_jump(Instruction::Jump(usize::MAX));
                    for jump in false_jumps {
                        self.patch_jump(jump, self.current_target());
                    }
                    self.context_reads = dominating_context_reads.clone();
                    self.param_reads = dominating_param_reads.clone();
                    self.compile_block(else_block);
                    self.patch_jump(end_jump, self.current_target());
                } else {
                    for jump in false_jumps {
                        self.patch_jump(jump, self.current_target());
                    }
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
                    self.emit_range_start(count, super::MAX_DSL_LOOP_ITERATIONS as i32)
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
                self.emit(Instruction::IntAdd {
                    dst: self.int_slot(index_slot),
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
                self.emit(Instruction::IntAdd {
                    dst: self.int_slot(index_slot),
                    left: self.int_slot(index_slot),
                    right: self.int_slot(one_slot),
                });
                self.finish_loop(id, loop_start);
                self.context_reads = dominating_context_reads;
                self.param_reads = dominating_param_reads;
                let _ = self.scopes.pop();
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
                self.emit(float_const_instruction(
                    op,
                    dst,
                    value,
                    constant.to_bits(),
                    constant_left,
                ));
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

    /// Conditions need control flow, not a temporary boolean for each logical
    /// operator. Return the branches whose destination belongs to the caller.
    fn compile_condition(&mut self, expr: CheckedExpr, when: bool) -> Vec<usize> {
        let context_reads = self.context_reads.clone();
        let param_reads = self.param_reads.clone();
        let jumps = match expr.kind {
            CheckedExprKind::Unary {
                op: UnaryOp::Not,
                expr,
            } => self.compile_condition(*expr, !when),
            CheckedExprKind::Binary {
                op: op @ (BinaryOp::And | BinaryOp::Or),
                left,
                right,
            } => {
                let short_circuit = op == BinaryOp::Or;
                if when == short_circuit {
                    let mut jumps = self.compile_condition(*left, when);
                    jumps.extend(self.compile_condition(*right, when));
                    jumps
                } else {
                    let skip = self.compile_condition(*left, short_circuit);
                    let jumps = self.compile_condition(*right, when);
                    for jump in skip {
                        self.patch_jump(jump, self.current_target());
                    }
                    jumps
                }
            }
            _ => {
                let start = self.instructions.len();
                let condition = self.compile_expr(expr);
                let branch = self.instructions.last().and_then(|last| {
                    (self.instructions.len() > start && last.written_slot() == Some(condition))
                        .then(|| super::optimize::comparison_branch(last, when, usize::MAX))
                        .flatten()
                });
                let branch = if let Some(branch) = branch {
                    self.instructions.pop();
                    branch
                } else if when {
                    Instruction::JumpIfTrue {
                        condition: self.bool_slot(condition),
                        target: usize::MAX,
                    }
                } else {
                    Instruction::JumpIfFalse {
                        condition: self.bool_slot(condition),
                        target: usize::MAX,
                    }
                };
                vec![self.emit_jump(branch)]
            }
        };
        // Reads introduced inside a short-circuited operand do not dominate the
        // continuation. Keep only values available before this condition.
        self.context_reads = context_reads;
        self.param_reads = param_reads;
        jumps
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
                let width = self.allocate_slot(&Type::Float);
                let width = self.float_slot(width);
                let inverse = self.allocate_slot(&Type::Float);
                let inverse = self.float_slot(inverse);
                self.emit(Instruction::FloatBinaryConst {
                    dst: width,
                    op: FloatBinary::Max,
                    value: args[0],
                    constant_bits: 1.0f32.to_bits(),
                });
                self.emit(Instruction::FloatDivideIntoConst {
                    dst: inverse,
                    value: width,
                    constant_bits: 1.0f32.to_bits(),
                });
                self.emit(Instruction::SectionPosition {
                    dst,
                    width,
                    inverse,
                });
            }
            "section_count" | "section_index" => {
                let args = self.compile_args(args);
                self.emit(Instruction::SectionQuery {
                    dst: self.int_slot(dst),
                    width: self.int_slot(args[0]),
                    index: name.as_str() == "section_index",
                });
            }
            "int" => {
                let src = self.compile_float_args(args)[0];
                self.emit(Instruction::FloatToInt {
                    dst: self.int_slot(dst),
                    src,
                });
            }
            "sin" | "cos" | "abs" | "floor" | "sqrt" => {
                let args = self.compile_float_args(args);
                let dst = self.float_slot(dst);
                self.emit(Instruction::FloatUnary {
                    dst,
                    op: match name.as_str() {
                        "sin" => FloatUnary::Sin,
                        "cos" => FloatUnary::Cos,
                        "abs" => FloatUnary::Abs,
                        "floor" => FloatUnary::Floor,
                        "sqrt" => FloatUnary::Sqrt,
                        _ => unreachable!("matched float unary builtin"),
                    },
                    value: args[0],
                });
            }
            "is_nan" => {
                let args = self.compile_float_args(args);
                self.emit(Instruction::ValueEqual {
                    dst: self.bool_slot(dst),
                    negate: true,
                    left: ValueSlot::Float(args[0]),
                    right: ValueSlot::Float(args[0]),
                });
            }
            "value_or" | "atan2" => {
                let args = self.compile_float_args(args);
                let dst = self.float_slot(dst);
                self.emit(Instruction::FloatBinary {
                    dst,
                    op: match name.as_str() {
                        "value_or" => FloatBinary::ValueOr,
                        "atan2" => FloatBinary::Atan2,
                        _ => unreachable!("matched float binary builtin"),
                    },
                    left: args[0],
                    right: args[1],
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
                let width = self.allocate_slot(&Type::Float);
                let width = self.float_slot(width);
                let position = self.allocate_slot(&Type::Float);
                let position = self.float_slot(position);
                let normalized = self.allocate_slot(&Type::Float);
                let normalized = self.float_slot(normalized);
                self.emit(Instruction::FloatSubtract {
                    dst: width,
                    left: args[1],
                    right: args[0],
                });
                self.emit(Instruction::FloatSubtract {
                    dst: position,
                    left: args[2],
                    right: args[0],
                });
                self.emit(Instruction::FloatDivide {
                    dst: normalized,
                    left: position,
                    right: width,
                });
                self.emit(Instruction::Smoothstep {
                    dst,
                    value: normalized,
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
            "rand" => {
                let seed = self.compile_float_args(args)[0];
                let dst = self.float_slot(dst);
                // Adding zero maps a negative-zero seed to the same hash as zero.
                self.emit(Instruction::FloatAddConst {
                    dst,
                    value: seed,
                    constant_bits: 0.0_f32.to_bits(),
                });
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
            "curve_first_crossing" | "curve_last_crossing" => {
                if let Some(param) = self.param_binding(&args[0], &Type::Curve) {
                    let registers = self.compile_float_args(args.into_iter().skip(1).collect());
                    let dst = self.float_slot(dst);
                    self.emit(Instruction::CurveParamCrossing {
                        dst,
                        param,
                        source: CurveSlot(self.parameter_bank_index(param)),
                        value: registers[0],
                        before: registers.get(1).copied(),
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
                        before: registers.get(1).copied(),
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
            "mark_count" | "mark_at" | "mark_last" | "mark_last_index" => {
                let args = self.compile_args(args);
                let marks = self.marks_slot(args[0]);
                let op = match name.as_str() {
                    "mark_count" => MarkOp::Count {
                        dst: self.int_slot(dst),
                    },
                    "mark_at" => MarkOp::At {
                        dst: self.float_slot(dst),
                        index: self.int_slot(args[1]),
                    },
                    "mark_last" => MarkOp::Last {
                        dst: self.float_slot(dst),
                        seconds: self.float_slot(args[1]),
                    },
                    "mark_last_index" => MarkOp::LastIndex {
                        dst: self.int_slot(dst),
                        seconds: self.float_slot(args[1]),
                    },
                    _ => unreachable!("matched mark builtin"),
                };
                self.emit(Instruction::Mark { marks, op });
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
                    self.emit(float_instruction(op, dst, left, right));
                }
                ValueSlot::Int(dst) => self.emit(int_instruction(
                    op,
                    dst,
                    self.int_slot(left),
                    self.int_slot(right),
                )),
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

    fn marks_slot(&self, slot: ValueSlot) -> MarksSlot {
        match slot {
            ValueSlot::Marks(slot) => slot,
            _ => unreachable!("checked expression has marks type"),
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
        if cap <= 0 || cap as usize > super::MAX_DSL_LOOP_ITERATIONS {
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
        let Some(existing) = self.instructions[offset].jump_target_mut() else {
            unreachable!("only compiler branches are patched")
        };
        *existing = target;
    }
}

fn pool_span(start: usize, len: usize) -> PoolSpan {
    debug_assert!(u32::try_from(start).is_ok() && u32::try_from(len).is_ok());
    PoolSpan {
        start: start as u32,
        len: len as u32,
    }
}

fn float_instruction(
    op: BinaryOp,
    dst: FloatSlot,
    left: FloatSlot,
    right: FloatSlot,
) -> Instruction {
    match op {
        BinaryOp::Add => Instruction::FloatAdd { dst, left, right },
        BinaryOp::Subtract => Instruction::FloatSubtract { dst, left, right },
        BinaryOp::Multiply => Instruction::FloatMultiply { dst, left, right },
        BinaryOp::Divide => Instruction::FloatDivide { dst, left, right },
        BinaryOp::Remainder => Instruction::FloatRemainder { dst, left, right },
        _ => unreachable!("checked arithmetic operator"),
    }
}

fn int_instruction(op: BinaryOp, dst: IntSlot, left: IntSlot, right: IntSlot) -> Instruction {
    match op {
        BinaryOp::Add => Instruction::IntAdd { dst, left, right },
        BinaryOp::Subtract => Instruction::IntSubtract { dst, left, right },
        BinaryOp::Multiply => Instruction::IntMultiply { dst, left, right },
        BinaryOp::Remainder => Instruction::IntRemainder { dst, left, right },
        _ => unreachable!("checked arithmetic operator"),
    }
}

fn float_const_instruction(
    op: BinaryOp,
    dst: FloatSlot,
    value: FloatSlot,
    constant_bits: u32,
    constant_left: bool,
) -> Instruction {
    match (op, constant_left) {
        (BinaryOp::Add, _) => Instruction::FloatAddConst {
            dst,
            value,
            constant_bits,
        },
        (BinaryOp::Subtract, true) => Instruction::FloatSubtractFromConst {
            dst,
            value,
            constant_bits,
        },
        (BinaryOp::Subtract, _) => Instruction::FloatSubtractConst {
            dst,
            value,
            constant_bits,
        },
        (BinaryOp::Multiply, _) => Instruction::FloatMultiplyConst {
            dst,
            value,
            constant_bits,
        },
        (BinaryOp::Divide, true) => Instruction::FloatDivideIntoConst {
            dst,
            value,
            constant_bits,
        },
        (BinaryOp::Divide, _) => Instruction::FloatDivideConst {
            dst,
            value,
            constant_bits,
        },
        (BinaryOp::Remainder, true) => Instruction::FloatRemainderFromConst {
            dst,
            value,
            constant_bits,
        },
        (BinaryOp::Remainder, _) => Instruction::FloatRemainderConst {
            dst,
            value,
            constant_bits,
        },
        _ => unreachable!("checked arithmetic operator"),
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

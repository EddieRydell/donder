use super::ast::{BinaryOp, UnaryOp};
use super::bytecode::{
    ArithmeticOp, BoolSlot, BytecodeProgram, ColorBinary, ColorSlot, CompareOp, ContextRead,
    FloatBinary, FloatSlot, FloatUnary, GeneratorContextId, Instruction, IntArithmeticOp, IntSlot,
    LocalId, MarkOp, ParamId, PoolSpan, RefSlot, SlotLayout, Target, TargetItemsOp, TargetMember,
    ValueSlot,
};
use super::checked::{
    CheckedBlock, CheckedEffectDecl, CheckedExpr, CheckedExprKind, CheckedModule,
    CheckedOperatorDecl, CheckedStmt,
};
use super::types::{Identifier, Type, Value};
use super::{CompiledEffect, CompiledOperator, EffectKind};
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

fn compile_effect(
    effect: CheckedEffectDecl,
) -> Result<super::EffectCompilation, super::Diagnostic> {
    let kind = if effect.entrypoint.name.as_str() == "generate" {
        EffectKind::Generator
    } else {
        EffectKind::Sample
    };
    let mut compiler = FunctionCompiler::new(&effect.params, kind);
    let bytecode = compiler.compile(effect.body)?;
    Ok(super::EffectCompilation {
        effect: CompiledEffect {
            name: effect.name,
            params: effect.params,
            kind,
            bytecode,
            emit_fields: compiler.emit_fields.into_boxed_slice(),
            generated_effect_count: compiler.generated_effects.len() as u32,
        },
        emitted_references: compiler.generated_effects.into_boxed_slice(),
    })
}

fn compile_operator(operator: CheckedOperatorDecl) -> Result<CompiledOperator, super::Diagnostic> {
    let bytecode = FunctionCompiler::new_operator(&operator.params, &operator.inputs)
        .compile(operator.body)?;
    Ok(CompiledOperator {
        name: operator.name,
        inputs: operator.inputs,
        params: operator.params,
        bytecode,
    })
}

struct FunctionCompiler {
    instructions: Vec<Instruction>,
    constants: Vec<Value>,
    value_operands: Vec<ValueSlot>,
    emit_fields: Vec<(Identifier, ValueSlot)>,
    generated_effects: Vec<super::EmittedReference>,
    scopes: Vec<IndexMap<Identifier, Binding>>,
    param_types: Vec<Type>,
    layout: SlotLayout,
    kind: EffectKind,
    signal_inputs: IndexMap<Identifier, usize>,
    assigned_names: HashSet<Identifier>,
    context_reads: HashMap<ContextRead, ValueSlot>,
    param_reads: HashMap<ParamId, ValueSlot>,
    array_roots: Vec<u32>,
    array_widths: Vec<u32>,
}

fn array_depth(mut ty: &Type) -> usize {
    let mut depth = 0;
    while let Type::Array(item) = ty {
        depth += 1;
        ty = item;
    }
    depth
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
    GeneratorContext(GeneratorContextId),
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
        BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => true,
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
        if kind == EffectKind::Generator {
            param_scope.insert(
                static_identifier("timeline"),
                Binding::GeneratorContext(GeneratorContextId::Timeline),
            );
            param_scope.insert(
                static_identifier("target"),
                Binding::GeneratorContext(GeneratorContextId::Target),
            );
            param_scope.insert(
                static_identifier("duration"),
                Binding::GeneratorContext(GeneratorContextId::Duration),
            );
        }
        Self {
            instructions: Vec::new(),
            constants: Vec::new(),
            value_operands: Vec::new(),
            emit_fields: Vec::new(),
            generated_effects: Vec::new(),
            scopes: vec![param_scope],
            param_types: params.iter().map(|param| param.ty.clone()).collect(),
            layout: SlotLayout::default(),
            kind,
            signal_inputs: IndexMap::new(),
            assigned_names: HashSet::new(),
            context_reads: HashMap::new(),
            param_reads: HashMap::new(),
            array_roots: Vec::new(),
            array_widths: Vec::new(),
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
        self.compile_block(block);
        if self.kind == EffectKind::Generator {
            let void = self.allocate_slot(&Type::Void);
            let constant = self.add_constant(Value::Void);
            self.emit(Instruction::LoadConst {
                dst: void,
                constant,
            });
            self.emit(Instruction::Return(void));
        }
        super::array_lowering::lower_arrays(
            &mut self.instructions,
            &mut self.constants,
            &mut self.value_operands,
        );
        super::optimize::cleanup(
            &mut self.instructions,
            &mut self.constants,
            &mut self.value_operands,
            &mut self.emit_fields,
            &mut self.layout,
        );
        let (array_capacity, array_width) = self.array_storage_bound().ok_or_else(|| {
            super::Diagnostic::new(
                super::lexer::TextSpan { start: 0, end: 0 },
                "calculated array storage exceeds 32-bit addressable capacity",
            )
        })?;
        let pixel_entry = if self.kind == EffectKind::Sample {
            super::optimize::hoist_uniform(
                &mut self.instructions,
                &mut self.value_operands,
                &mut self.emit_fields,
            )
        } else {
            0
        };
        Ok(BytecodeProgram {
            pixel_entry,
            array_capacity,
            array_width,
            uses_pixel_context: self.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::ContextRead {
                        read: ContextRead::PixelIndex
                            | ContextRead::PixelCount
                            | ContextRead::PixelFraction,
                        ..
                    } | Instruction::SectionPosition { .. }
                        | Instruction::SignalSample { .. }
                )
            }),
            instructions: std::mem::take(&mut self.instructions).into_boxed_slice(),
            constants: std::mem::take(&mut self.constants).into_boxed_slice(),
            value_operands: std::mem::take(&mut self.value_operands).into_boxed_slice(),
            layout: self.layout,
        })
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
                        let slot = self.allocate_local(name, &ty);
                        let value = self.compile_expr(initializer);
                        let value = self.coerce_slot(value, &ty);
                        self.emit(Instruction::Move {
                            dst: slot,
                            src: value,
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
                            src: value,
                        });
                    }
                    Some(Binding::Param(_)) => unreachable!("assigned parameters are locals"),
                    Some(Binding::GeneratorContext(_)) | None => {}
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
                self.scopes.push(IndexMap::new());
                self.compile_statement(*initializer);
                let loop_start = self.current_target();
                let condition = self.compile_expr(condition);
                let condition = self.bool_slot(condition);
                let dominating_context_reads = self.context_reads.clone();
                let dominating_param_reads = self.param_reads.clone();
                let end_jump = self.emit_jump(Instruction::JumpIfFalse {
                    condition,
                    target: usize::MAX,
                });
                self.emit(Instruction::CheckLoopLimit);
                self.compile_block(body);
                self.compile_statement(*update);
                self.emit(Instruction::Jump(loop_start));
                self.patch_jump(end_jump, self.current_target());
                self.context_reads = dominating_context_reads;
                self.param_reads = dominating_param_reads;
                let _ = self.scopes.pop();
            }
            CheckedStmt::Emit { effect, fields } => {
                let fields = fields
                    .into_iter()
                    .map(|(name, value)| (name, self.compile_expr(value)))
                    .collect::<Vec<_>>();
                let fields = self.add_emit_fields(fields);
                let effect = self.add_generated_effect(effect);
                self.emit(Instruction::Emit { effect, fields });
            }
            CheckedStmt::Return(expr) => {
                let value = self.compile_expr(expr);
                if self.kind == EffectKind::Sample {
                    self.emit(Instruction::ReturnColor(self.color_slot(value)));
                } else {
                    self.emit(Instruction::Return(value));
                }
            }
        }
    }

    fn compile_expr(&mut self, expr: CheckedExpr) -> ValueSlot {
        let result_ty = expr.ty.clone();
        match expr.kind {
            CheckedExprKind::Literal(value) => {
                let dst = self.allocate_slot(&result_ty);
                let constant = self.add_constant(value);
                self.emit(Instruction::LoadConst { dst, constant });
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
                Some(Binding::GeneratorContext(slot)) => {
                    let dst = self.allocate_slot(&result_ty);
                    self.emit(Instruction::LoadGeneratorContext { dst, slot });
                    dst
                }
                None => {
                    let value = match name.as_str() {
                        "PI" => Value::Float(std::f32::consts::PI),
                        "TAU" => Value::Float(std::f32::consts::TAU),
                        _ => Value::Enum(name),
                    };
                    let dst = self.allocate_slot(&result_ty);
                    let constant = self.add_constant(value);
                    self.emit(Instruction::LoadConst { dst, constant });
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
                    let constant = self.add_constant(Value::Array(values.into()));
                    self.emit(Instruction::LoadConst { dst, constant });
                    return dst;
                }
                let item_slots = items
                    .into_iter()
                    .map(|item| self.compile_expr(item))
                    .collect::<Vec<_>>();
                let depth = array_depth(&result_ty);
                self.array_widths
                    .resize(self.array_widths.len().max(depth + 1), 0);
                self.array_widths[depth] = self.array_widths[depth].max(item_slots.len() as u32);
                let item_slots = self.add_value_operands(item_slots);
                let dst = self.allocate_slot(&result_ty);
                let dst = self.ref_slot(dst);
                self.emit(Instruction::MakeArray {
                    dst,
                    items: item_slots,
                });
                ValueSlot::Ref(dst)
            }
            CheckedExprKind::Index { target, index } => {
                if let Some(param) = self.param_binding(&target, &Type::Curve) {
                    let position = self.float_slot_from_expr(*index);
                    let slot = self.allocate_slot(&result_ty);
                    let dst = self.float_slot(slot);
                    self.emit(Instruction::CurveParamSample {
                        dst,
                        param,
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
                        position,
                    });
                    ValueSlot::Color(dst)
                } else {
                    let target = self.compile_expr(*target);
                    let target = self.ref_slot(target);
                    let index = self.compile_expr(*index);
                    let dst = self.allocate_slot(&result_ty);
                    self.emit(Instruction::Index { dst, target, index });
                    dst
                }
            }
            CheckedExprKind::Member { target, member } => {
                let target = self.compile_expr(*target);
                let target = self.ref_slot(target);
                let dst = self.allocate_slot(&result_ty);
                self.emit(Instruction::Member {
                    dst,
                    target,
                    member: target_member(&member),
                });
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
        self.emit(Instruction::Move { dst, src: left });
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
        self.emit(Instruction::Move { dst, src: right });
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
            "intensity" => {
                let args = self.compile_args(args);
                let dst = self.float_slot(dst);
                self.emit(Instruction::ColorIntensity {
                    dst,
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
                let args = self.add_float_operands(args);
                let dst = self.float_slot(dst);
                self.emit(Instruction::Rand { dst, args });
            }
            "curve_clamped" if args.len() == 4 => {
                if let Some(param) = self.param_binding(&args[0], &Type::Curve) {
                    let registers = self.compile_float_args(args.into_iter().skip(1).collect());
                    let dst = self.float_slot(dst);
                    self.emit(Instruction::CurveParamFloatClamped {
                        dst,
                        param,
                        position: registers[0],
                        min: registers[1],
                        max: registers[2],
                    });
                } else {
                    let mut args = args;
                    let curve_expr = args.remove(0);
                    let curve = self.compile_expr(curve_expr);
                    let curve = self.ref_slot(curve);
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
                        position: registers[0],
                        scale: registers[1],
                    });
                } else {
                    let mut args = args;
                    let gradient_expr = args.remove(0);
                    let gradient = self.compile_expr(gradient_expr);
                    let gradient = self.ref_slot(gradient);
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
                        value: registers[0],
                        fallback: registers.get(1).copied(),
                    });
                } else {
                    let mut args = args;
                    let curve_expr = args.remove(0);
                    let curve = self.compile_expr(curve_expr);
                    let curve = self.ref_slot(curve);
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
                self.emit(Instruction::Len {
                    dst: self.int_slot(dst),
                    value: self.ref_slot(args[0]),
                });
            }
            "mark_count" | "mark_at" | "mark_prev" | "mark_prev_index" | "mark_next_index"
            | "mark_elapsed" | "mark_phase" => {
                let op = match name.as_str() {
                    "mark_count" => MarkOp::Count,
                    "mark_at" => MarkOp::At,
                    "mark_prev" => MarkOp::Prev,
                    "mark_prev_index" => MarkOp::PrevIndex,
                    "mark_next_index" => MarkOp::NextIndex,
                    "mark_elapsed" => MarkOp::Elapsed,
                    "mark_phase" => MarkOp::Phase,
                    _ => unreachable!("matched mark builtin"),
                };
                let args = self.compile_args(args);
                let args = self.add_value_operands(args);
                self.emit(Instruction::Mark { dst, op, args });
            }
            "fixtures" | "pixels" | "sections" | "count" | "pick" => {
                let op = match name.as_str() {
                    "fixtures" => TargetItemsOp::Fixtures,
                    "pixels" => TargetItemsOp::Pixels,
                    "sections" => TargetItemsOp::Sections,
                    "count" => TargetItemsOp::Count,
                    "pick" => TargetItemsOp::Pick,
                    _ => unreachable!("matched target builtin"),
                };
                let args = self.compile_args(args);
                let args = self.add_value_operands(args);
                self.emit(Instruction::TargetItems { dst, op, args });
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
        match dst {
            ValueSlot::Int(dst) => self.emit(Instruction::LoadIntParam { dst, param }),
            ValueSlot::Float(dst) => self.emit(Instruction::LoadFloatParam { dst, param }),
            ValueSlot::Bool(dst) => self.emit(Instruction::LoadBoolParam { dst, param }),
            ValueSlot::Color(dst) => self.emit(Instruction::LoadColorParam { dst, param }),
            ValueSlot::Ref(dst) => self.emit(Instruction::LoadRefParam { dst, param }),
        }
    }

    fn emit_context_read(&mut self, dst: ValueSlot, read: ContextRead) {
        self.emit(Instruction::ContextRead { dst, read });
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
                let left = self.float_slot(left);
                let right = self.float_slot(right);
                self.emit(Instruction::FloatCompare {
                    dst: self.bool_slot(dst),
                    op: compare_op(op),
                    left,
                    right,
                });
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
        Some(self.add_constant(Value::Enum(name.clone())))
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
            Some(Binding::Param(_) | Binding::GeneratorContext(_)) | None => true,
        }
    }

    fn allocate_slot(&mut self, ty: &Type) -> ValueSlot {
        let depth = array_depth(ty);
        if depth != 0 {
            self.array_roots
                .resize(self.array_roots.len().max(depth + 1), 0);
            self.array_roots[depth] += 1;
        }
        ValueSlot::for_type(ty, &mut self.layout)
    }

    fn array_storage_bound(&self) -> Option<(u32, u32)> {
        if !self
            .instructions
            .iter()
            .any(|op| matches!(op, Instruction::MakeArray { .. }))
        {
            return Some((0, 0));
        }
        let width = self.array_widths.iter().copied().max().unwrap_or(0);
        if width == 0 {
            return Some((0, 0));
        }
        let mut live = 0_u32;
        let mut capacity = 1_u32; // New array before its destination is overwritten.
        for depth in (1..self.array_roots.len()).rev() {
            let parent_width = self.array_widths.get(depth + 1).copied().unwrap_or(0);
            live = live
                .checked_mul(parent_width)?
                .checked_add(self.array_roots[depth])?;
            capacity = capacity.checked_add(live)?;
        }
        capacity
            .checked_mul(width)?
            .checked_mul(size_of::<Value>() as u32)?;
        capacity.checked_mul(2)?.checked_add(3)?;
        Some((capacity, width))
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

    fn ref_slot(&self, slot: ValueSlot) -> RefSlot {
        match slot {
            ValueSlot::Ref(slot) => slot,
            _ => unreachable!("checked expression is reference-like"),
        }
    }

    fn lookup(&self, name: &Identifier) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn add_constant(&mut self, value: Value) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }

    fn emit_default(&mut self, dst: ValueSlot, ty: &Type) {
        let constant = self.add_constant(ty.default_value());
        self.emit(Instruction::LoadConst { dst, constant });
    }

    fn add_value_operands(&mut self, values: Vec<ValueSlot>) -> PoolSpan {
        let span = pool_span(self.value_operands.len(), values.len());
        self.value_operands.extend(values);
        span
    }

    fn add_float_operands(&mut self, values: Vec<FloatSlot>) -> PoolSpan {
        self.add_value_operands(values.into_iter().map(ValueSlot::Float).collect())
    }

    fn add_emit_fields(&mut self, values: Vec<(Identifier, ValueSlot)>) -> PoolSpan {
        let span = pool_span(self.emit_fields.len(), values.len());
        self.emit_fields.extend(values);
        span
    }

    fn add_generated_effect(
        &mut self,
        effect: super::EmittedReference,
    ) -> super::GeneratedEffectSlot {
        debug_assert!(u32::try_from(self.generated_effects.len()).is_ok());
        let index = self.generated_effects.len() as u32;
        self.generated_effects.push(effect);
        super::GeneratedEffectSlot(index)
    }

    fn current_target(&self) -> Target {
        self.instructions.len()
    }

    fn emit(&mut self, instruction: Instruction) {
        self.instructions.push(instruction);
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
        "element_index" => TargetMember::ElementIndex,
        "element_cell_index" => TargetMember::ElementCellIndex,
        "pixel_index" => TargetMember::PixelIndex,
        "pixel_count" => TargetMember::PixelCount,
        "pixel_fraction" => TargetMember::PixelFraction,
        _ => unreachable!("checked TargetItem member is known"),
    }
}

fn static_identifier(value: &str) -> Identifier {
    match Identifier::new(value.to_string()) {
        Ok(identifier) => identifier,
        Err(_) => unreachable!("static identifier is valid"),
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

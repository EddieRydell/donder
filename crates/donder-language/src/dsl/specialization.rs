//! Host specialization of checked generators. Expressions and retained control
//! flow are always compiled to the ordinary typed VM; this is not an expression
//! interpreter. Only fixed expansion control is traversed here.
use super::checked::{CheckedBlock, CheckedExpr, CheckedExprKind, CheckedStmt};
use super::lexer::TextSpan;
use super::{
    BoundParams, BytecodeProgram, EmittedReference, GeneratedEffectSlot, GeneratorContext,
    Identifier, ParamDecl, RunContext, RuntimeError, TargetItemValue, Type, Value, VmWorkspace,
};
use crate::values::{SampleDuration, SampleTime};
use indexmap::{IndexMap, IndexSet};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratorProgram {
    params: Vec<ParamDecl>,
    body: CheckedBlock,
    emissions: Vec<EmittedReference>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GeneratorInput {
    Fixed(Value),
    Live,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GeneratorBinding {
    Constant(Value),
    Parameter(u16),
    Calculation { index: u32, output: u16 },
}

/// A tuple-valued VM program. Input/output names disappear before playback.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratorCalculation {
    pub program: BytecodeProgram,
    pub inputs: Box<[(Type, GeneratorBinding)]>,
    pub output_types: Box<[Type]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpecializedChild {
    pub definition: GeneratedEffectSlot,
    pub start_time: SampleTime,
    pub duration: SampleDuration,
    pub target: Arc<TargetItemValue>,
    pub params: Vec<(Identifier, GeneratorBinding)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpecializedGenerator {
    pub calculations: Vec<GeneratorCalculation>,
    pub children: Vec<SpecializedChild>,
}

#[derive(Clone)]
struct Symbol {
    ty: Type,
    binding: GeneratorBinding,
}
type Environment = IndexMap<Identifier, Symbol>;

impl GeneratorProgram {
    pub(super) fn new(
        params: Vec<ParamDecl>,
        body: CheckedBlock,
        emissions: Vec<EmittedReference>,
    ) -> Self {
        Self {
            params,
            body,
            emissions,
        }
    }

    pub fn specialize(
        &self,
        inputs: &[GeneratorInput],
        context: &GeneratorContext,
        max_children: usize,
    ) -> Result<SpecializedGenerator, RuntimeError> {
        if inputs.len() != self.params.len() {
            return Err(error(
                "generator input count does not match its declaration",
            ));
        }
        let mut env = Environment::new();
        for (index, (param, input)) in self.params.iter().zip(inputs).enumerate() {
            let binding = match input {
                GeneratorInput::Fixed(value) => GeneratorBinding::Constant(value.clone()),
                GeneratorInput::Live if param.fixed => {
                    return Err(error(format!(
                        "fixed parameter `{}` cannot receive a live binding",
                        param.name.as_str()
                    )));
                }
                GeneratorInput::Live => GeneratorBinding::Parameter(
                    u16::try_from(index).map_err(|_| error("too many generator parameters"))?,
                ),
            };
            env.insert(
                param.name.clone(),
                Symbol {
                    ty: param.ty.clone(),
                    binding,
                },
            );
        }
        env.insert(
            identifier("target"),
            Symbol {
                ty: Type::Target,
                binding: GeneratorBinding::Constant(Value::Target(Arc::clone(&context.target))),
            },
        );
        env.insert(
            identifier("duration"),
            Symbol {
                ty: Type::Float,
                binding: GeneratorBinding::Constant(Value::Float(
                    crate::values::sample_duration_seconds_f32(context.duration),
                )),
            },
        );
        let mut specializer = Specializer {
            source: self,
            context,
            result: SpecializedGenerator::default(),
            workspace: VmWorkspace::default(),
            max_children,
            remaining_iterations: donder_runtime::dsl::MAX_VM_INSTRUCTIONS_PER_INVOCATION,
        };
        specializer.block(&self.body, &mut env)?;
        Ok(specializer.result)
    }
}

struct Specializer<'a> {
    source: &'a GeneratorProgram,
    context: &'a GeneratorContext,
    result: SpecializedGenerator,
    workspace: VmWorkspace,
    remaining_iterations: usize,
    max_children: usize,
}

impl Specializer<'_> {
    fn evaluate(&mut self, program: &BytecodeProgram) -> Result<Value, RuntimeError> {
        program.evaluate_value(
            &BoundParams::default(),
            &RunContext {
                progress: 0.0,
                time: SampleDuration::from_ticks(0),
                duration: self.context.duration,
                pixel_index: 0,
                pixel_count: 0,
                pixel_fraction: 0.0,
            },
            &mut self.workspace,
            &mut self.remaining_iterations,
        )
    }

    fn calculate(
        &mut self,
        lowering: Lowering,
        statements: Vec<CheckedStmt>,
        outputs: Vec<CheckedExpr>,
    ) -> Result<Vec<GeneratorBinding>, RuntimeError> {
        let output_types = outputs
            .iter()
            .map(|output| output.ty.clone())
            .collect::<Vec<_>>();
        let result = CheckedExpr {
            kind: CheckedExprKind::Array(outputs),
            span: span(),
            // Tuple slots are typed individually, not exposed as an authored array.
            ty: Type::Array(Box::new(Type::Void)),
        };
        let params = lowering
            .inputs
            .iter()
            .enumerate()
            .map(|(index, (ty, _))| ParamDecl {
                name: input_name(index),
                ty: ty.clone(),
                default: None,
                fixed: false,
            })
            .collect::<Vec<_>>();
        let program = super::compiler::compile_value(&params, statements, result)
            .map_err(|diagnostic| error(diagnostic.message))?;
        let uses_time = program.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                donder_runtime::dsl::bytecode::Instruction::ContextRead {
                    read: donder_runtime::dsl::bytecode::ContextRead::Seconds
                        | donder_runtime::dsl::bytecode::ContextRead::Progress,
                    ..
                }
            )
        });
        if lowering.inputs.is_empty() && !uses_time {
            let Value::Array(values) = self.evaluate(&program)? else {
                return Err(error(
                    "generator calculation did not return its typed outputs",
                ));
            };
            return Ok(values
                .iter()
                .cloned()
                .map(GeneratorBinding::Constant)
                .collect());
        }
        let index = u32::try_from(self.result.calculations.len())
            .map_err(|_| error("too many retained generator calculations"))?;
        let bindings = output_types
            .iter()
            .enumerate()
            .map(|(output, _)| {
                Ok(GeneratorBinding::Calculation {
                    index,
                    output: u16::try_from(output)
                        .map_err(|_| error("too many generator calculation outputs"))?,
                })
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        self.result.calculations.push(GeneratorCalculation {
            program,
            inputs: lowering.inputs.into_boxed_slice(),
            output_types: output_types.into_boxed_slice(),
        });
        Ok(bindings)
    }

    fn expression(
        &mut self,
        expr: &CheckedExpr,
        env: &Environment,
    ) -> Result<GeneratorBinding, RuntimeError> {
        if let CheckedExprKind::Variable(name) = &expr.kind
            && let Some(symbol) = env.get(name)
        {
            return Ok(symbol.binding.clone());
        }
        if let CheckedExprKind::Literal(value) = &expr.kind {
            return Ok(GeneratorBinding::Constant(value.clone()));
        }
        let mut lowering = Lowering::default();
        let lowered = lowering.expr(expr, &lexical_environment(env))?;
        self.calculate(lowering, Vec::new(), vec![lowered])?
            .pop()
            .ok_or_else(|| error("generator expression has no result"))
    }

    fn fixed_bool(&mut self, expr: &CheckedExpr, env: &Environment) -> Result<bool, RuntimeError> {
        match self.expression(expr, env)? {
            GeneratorBinding::Constant(Value::Bool(value)) => Ok(value),
            _ => Err(error(
                "generator expansion control requires a fixed boolean",
            )),
        }
    }

    fn block(&mut self, block: &CheckedBlock, env: &mut Environment) -> Result<(), RuntimeError> {
        let mut shadowed = IndexMap::new();
        for statement in &block.statements {
            if let CheckedStmt::Local { name, .. } = statement {
                shadowed
                    .entry(name.clone())
                    .or_insert_with(|| env.get(name).cloned());
            }
            self.statement(statement, env)?;
        }
        for (name, previous) in shadowed {
            if let Some(previous) = previous {
                env.insert(name, previous);
            } else {
                env.shift_remove(&name);
            }
        }
        Ok(())
    }

    fn statement(
        &mut self,
        statement: &CheckedStmt,
        env: &mut Environment,
    ) -> Result<(), RuntimeError> {
        match statement {
            CheckedStmt::Local {
                ty,
                name,
                initializer,
            } => {
                let binding = if let Some(expr) = initializer {
                    self.expression(expr, env)?
                } else {
                    GeneratorBinding::Constant(ty.default_value())
                };
                env.insert(
                    name.clone(),
                    Symbol {
                        ty: ty.clone(),
                        binding,
                    },
                );
            }
            CheckedStmt::Assign { name, value } => {
                let binding = self.expression(value, env)?;
                env.get_mut(name)
                    .ok_or_else(|| error("unknown generator assignment"))?
                    .binding = binding;
            }
            CheckedStmt::Expr(expr) => {
                self.expression(expr, env)?;
            }
            CheckedStmt::If {
                condition,
                then_block,
                else_block,
            } => {
                let before_condition = self.result.calculations.len();
                let condition_value = self.expression(condition, env)?;
                if let GeneratorBinding::Constant(Value::Bool(value)) = condition_value {
                    if value {
                        self.block(then_block, env)?;
                    } else if let Some(block) = else_block {
                        self.block(block, env)?;
                    }
                } else if super::staging::contains_emit(then_block)
                    || else_block
                        .as_ref()
                        .is_some_and(super::staging::contains_emit)
                {
                    return Err(error("live control flow cannot determine child emission"));
                } else {
                    // The retained block contains its condition; do not retain the
                    // probe as a second calculation.
                    self.result.calculations.truncate(before_condition);
                    self.pure_control(statement, env)?;
                }
            }
            CheckedStmt::For {
                initializer,
                condition,
                update,
                body,
            } => {
                if !super::staging::contains_emit(body) {
                    return self.pure_control(statement, env);
                }
                let mut loop_env = env.clone();
                self.statement(initializer, &mut loop_env)?;
                while self.fixed_bool(condition, &loop_env)? {
                    self.remaining_iterations = self
                        .remaining_iterations
                        .checked_sub(1)
                        .ok_or_else(|| error("loop iteration limit exceeded"))?;
                    self.block(body, &mut loop_env)?;
                    self.statement(update, &mut loop_env)?;
                }
                for (name, value) in env.iter_mut() {
                    if matches!(initializer.as_ref(), CheckedStmt::Local { name: local, .. } if local == name)
                    {
                        continue;
                    }
                    *value = loop_env
                        .get(name)
                        .cloned()
                        .ok_or_else(|| error("generator loop lost an outer binding"))?;
                }
            }
            CheckedStmt::Emit { effect, fields } => {
                if self.result.children.len() >= self.max_children {
                    return Err(error("generated child limit exceeded"));
                }
                let mut structural = Vec::new();
                let mut params = Vec::new();
                for (name, expr) in fields {
                    let binding = self.expression(expr, env)?;
                    if matches!(name.as_str(), "start" | "duration" | "target") {
                        let GeneratorBinding::Constant(value) = binding else {
                            return Err(error("emitted structure requires fixed values"));
                        };
                        structural.push((name.clone(), literal(value, expr.ty.clone(), expr.span)));
                    } else {
                        params.push((name.clone(), binding));
                    }
                }
                let compiled = super::compiler::compile_emission(effect.clone(), structural)
                    .map_err(|diagnostic| error(diagnostic.message))?;
                let mut generated = compiled.generate_bound(
                    &BoundParams::default(),
                    self.context,
                    &mut self.workspace,
                )?;
                let child = generated
                    .pop()
                    .ok_or_else(|| error("fixed emission produced no child"))?;
                let slot = self
                    .source
                    .emissions
                    .iter()
                    .position(|candidate| candidate.span == effect.span)
                    .ok_or_else(|| error("emission is missing its numeric child slot"))?;
                self.result.children.push(SpecializedChild {
                    definition: GeneratedEffectSlot(
                        u32::try_from(slot).map_err(|_| error("too many emitted definitions"))?,
                    ),
                    start_time: child.start_time,
                    duration: child.duration,
                    target: child.target,
                    params,
                });
            }
            CheckedStmt::Return(_) => return Err(error("generator return cannot produce a value")),
        }
        Ok(())
    }

    fn pure_control(
        &mut self,
        statement: &CheckedStmt,
        env: &mut Environment,
    ) -> Result<(), RuntimeError> {
        let mut assigned = IndexSet::new();
        collect_assignments(statement, &mut HashSet::new(), env, &mut assigned);
        let mut lowering = Lowering::default();
        let mut lexical = lexical_environment(env);
        let mut statements = Vec::new();
        for name in &assigned {
            let symbol = env
                .get(name)
                .ok_or_else(|| error("unknown generator local"))?;
            let initializer = lowering.source(symbol, span());
            let local = lowering.local();
            statements.push(CheckedStmt::Local {
                ty: symbol.ty.clone(),
                name: local.clone(),
                initializer: Some(initializer),
            });
            lexical.insert(name.clone(), LexicalValue::Local(local));
        }
        statements.push(lowering.statement(statement, &mut lexical)?);
        let outputs = assigned
            .iter()
            .map(|name| {
                let ty = &env
                    .get(name)
                    .ok_or_else(|| error("unknown generator output"))?
                    .ty;
                lowering.expr(
                    &CheckedExpr {
                        kind: CheckedExprKind::Variable(name.clone()),
                        ty: ty.clone(),
                        span: span(),
                    },
                    &lexical,
                )
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let values = self.calculate(lowering, statements, outputs)?;
        for (name, binding) in assigned.into_iter().zip(values) {
            env.get_mut(&name)
                .ok_or_else(|| error("unknown generator output"))?
                .binding = binding;
        }
        Ok(())
    }
}

#[derive(Clone)]
enum LexicalValue {
    Source(Symbol),
    Local(Identifier),
}
type LexicalEnvironment = IndexMap<Identifier, LexicalValue>;
fn lexical_environment(env: &Environment) -> LexicalEnvironment {
    env.iter()
        .map(|(name, symbol)| (name.clone(), LexicalValue::Source(symbol.clone())))
        .collect()
}

#[derive(Default)]
struct Lowering {
    inputs: Vec<(Type, GeneratorBinding)>,
    next_local: usize,
}
impl Lowering {
    fn local(&mut self) -> Identifier {
        let name = identifier(&format!("local_{}", self.next_local));
        self.next_local += 1;
        name
    }
    fn source(&mut self, symbol: &Symbol, span: TextSpan) -> CheckedExpr {
        if let GeneratorBinding::Constant(value) = &symbol.binding {
            return literal(value.clone(), symbol.ty.clone(), span);
        }
        let index = self
            .inputs
            .iter()
            .position(|input| input == &(symbol.ty.clone(), symbol.binding.clone()))
            .unwrap_or_else(|| {
                self.inputs
                    .push((symbol.ty.clone(), symbol.binding.clone()));
                self.inputs.len() - 1
            });
        CheckedExpr {
            kind: CheckedExprKind::Variable(input_name(index)),
            ty: symbol.ty.clone(),
            span,
        }
    }
    fn expr(
        &mut self,
        expr: &CheckedExpr,
        env: &LexicalEnvironment,
    ) -> Result<CheckedExpr, RuntimeError> {
        let kind = match &expr.kind {
            CheckedExprKind::Literal(_) => return Ok(expr.clone()),
            CheckedExprKind::Variable(name) => match env.get(name) {
                Some(LexicalValue::Source(symbol)) => return Ok(self.source(symbol, expr.span)),
                Some(LexicalValue::Local(name)) => CheckedExprKind::Variable(name.clone()),
                None => return Ok(expr.clone()),
            },
            CheckedExprKind::Array(items) => CheckedExprKind::Array(
                items
                    .iter()
                    .map(|item| self.expr(item, env))
                    .collect::<Result<_, _>>()?,
            ),
            CheckedExprKind::Index { target, index } => CheckedExprKind::Index {
                target: Box::new(self.expr(target, env)?),
                index: Box::new(self.expr(index, env)?),
            },
            CheckedExprKind::Member { target, member } => CheckedExprKind::Member {
                target: Box::new(self.expr(target, env)?),
                member: member.clone(),
            },
            CheckedExprKind::Call { callee, args } => CheckedExprKind::Call {
                // A bare callee is a builtin, even when a parameter has the same name.
                callee: if matches!(callee.kind, CheckedExprKind::Variable(_)) {
                    callee.clone()
                } else {
                    Box::new(self.expr(callee, env)?)
                },
                args: args
                    .iter()
                    .map(|arg| self.expr(arg, env))
                    .collect::<Result<_, _>>()?,
            },
            CheckedExprKind::Unary { op, expr } => CheckedExprKind::Unary {
                op: *op,
                expr: Box::new(self.expr(expr, env)?),
            },
            CheckedExprKind::Binary { op, left, right } => CheckedExprKind::Binary {
                op: *op,
                left: Box::new(self.expr(left, env)?),
                right: Box::new(self.expr(right, env)?),
            },
            CheckedExprKind::SignalSample { .. } => {
                return Err(error("generators cannot sample signals"));
            }
        };
        Ok(CheckedExpr {
            kind,
            span: expr.span,
            ty: expr.ty.clone(),
        })
    }
    fn block(
        &mut self,
        block: &CheckedBlock,
        env: &LexicalEnvironment,
    ) -> Result<CheckedBlock, RuntimeError> {
        let mut local = env.clone();
        Ok(CheckedBlock {
            statements: block
                .statements
                .iter()
                .map(|statement| self.statement(statement, &mut local))
                .collect::<Result<_, _>>()?,
        })
    }
    fn statement(
        &mut self,
        statement: &CheckedStmt,
        env: &mut LexicalEnvironment,
    ) -> Result<CheckedStmt, RuntimeError> {
        Ok(match statement {
            CheckedStmt::Local {
                ty,
                name,
                initializer,
            } => {
                let initializer = initializer
                    .as_ref()
                    .map(|expr| self.expr(expr, env))
                    .transpose()?;
                let local = self.local();
                env.insert(name.clone(), LexicalValue::Local(local.clone()));
                CheckedStmt::Local {
                    ty: ty.clone(),
                    name: local,
                    initializer,
                }
            }
            CheckedStmt::Assign { name, value } => {
                let Some(LexicalValue::Local(local)) = env.get(name) else {
                    return Err(error("retained assignment has no local storage"));
                };
                CheckedStmt::Assign {
                    name: local.clone(),
                    value: self.expr(value, env)?,
                }
            }
            CheckedStmt::Expr(expr) => CheckedStmt::Expr(self.expr(expr, env)?),
            CheckedStmt::If {
                condition,
                then_block,
                else_block,
            } => CheckedStmt::If {
                condition: self.expr(condition, env)?,
                then_block: self.block(then_block, env)?,
                else_block: else_block
                    .as_ref()
                    .map(|block| self.block(block, env))
                    .transpose()?,
            },
            CheckedStmt::For {
                initializer,
                condition,
                update,
                body,
            } => {
                let mut loop_env = env.clone();
                let initializer = Box::new(self.statement(initializer, &mut loop_env)?);
                CheckedStmt::For {
                    initializer,
                    condition: self.expr(condition, &loop_env)?,
                    update: Box::new(self.statement(update, &mut loop_env)?),
                    body: self.block(body, &loop_env)?,
                }
            }
            CheckedStmt::Emit { .. } => {
                return Err(error("retained parameter code cannot emit children"));
            }
            CheckedStmt::Return(_) => {
                return Err(error(
                    "retained parameter code cannot return from a generator",
                ));
            }
        })
    }
}

fn collect_assignments(
    statement: &CheckedStmt,
    locals: &mut HashSet<Identifier>,
    env: &Environment,
    assigned: &mut IndexSet<Identifier>,
) {
    match statement {
        CheckedStmt::Local { name, .. } => {
            locals.insert(name.clone());
        }
        CheckedStmt::Assign { name, .. } if !locals.contains(name) && env.contains_key(name) => {
            assigned.insert(name.clone());
        }
        CheckedStmt::If {
            then_block,
            else_block,
            ..
        } => {
            for block in std::iter::once(then_block).chain(else_block.iter()) {
                let mut scoped = locals.clone();
                for statement in &block.statements {
                    collect_assignments(statement, &mut scoped, env, assigned);
                }
            }
        }
        CheckedStmt::For {
            initializer,
            update,
            body,
            ..
        } => {
            let mut scoped = locals.clone();
            collect_assignments(initializer, &mut scoped, env, assigned);
            let mut body_scope = scoped.clone();
            for statement in &body.statements {
                collect_assignments(statement, &mut body_scope, env, assigned);
            }
            collect_assignments(update, &mut scoped, env, assigned);
        }
        _ => {}
    }
}

fn literal(value: Value, ty: Type, span: TextSpan) -> CheckedExpr {
    CheckedExpr {
        kind: CheckedExprKind::Literal(value),
        ty,
        span,
    }
}
fn span() -> TextSpan {
    TextSpan { start: 0, end: 0 }
}
fn input_name(index: usize) -> Identifier {
    identifier(&format!("input_{index}"))
}
fn identifier(name: &str) -> Identifier {
    Identifier::new(name.to_owned())
        .unwrap_or_else(|_| unreachable!("compiler-generated identifier is valid"))
}
fn error(message: impl Into<String>) -> RuntimeError {
    RuntimeError {
        message: message.into(),
    }
}

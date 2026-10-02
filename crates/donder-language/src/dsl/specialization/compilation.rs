//! Lower generator expressions once, while compiling the effect declaration.
//! Preparation supplies captured values to these programs; it never invokes the compiler.
use super::*;
use crate::dsl::checked::{CheckedBlock, CheckedExpr, CheckedExprKind, CheckedStmt};
use crate::dsl::lexer::TextSpan;
use crate::dsl::{Diagnostic, EmittedReference};
use indexmap::{IndexMap, IndexSet};

#[derive(Clone)]
struct Symbol {
    ty: Type,
    slot: BindingSlot,
}
type Environment = IndexMap<Identifier, Symbol>;

struct CompileContext<'a> {
    slots: Vec<Type>,
    preparation_controls: &'a super::super::staging::PreparationControls,
}

impl CompileContext<'_> {
    fn declare(&mut self, env: &mut Environment, name: Identifier, ty: Type) -> BindingSlot {
        let slot = BindingSlot(self.slots.len());
        self.slots.push(ty.clone());
        env.insert(name, Symbol { ty, slot });
        slot
    }
}

pub(super) fn compile(
    params: &[ParamDecl],
    body: CheckedBlock,
    emissions: &[EmittedReference],
    preparation_controls: &super::super::staging::PreparationControls,
) -> Result<(Block, Box<[Type]>), Diagnostic> {
    let mut env = Environment::new();
    let mut context = CompileContext {
        slots: Vec::new(),
        preparation_controls,
    };
    for param in params {
        context.declare(&mut env, param.name.clone(), param.ty.clone());
    }
    context.declare(&mut env, identifier("target"), Type::Target);
    context.declare(&mut env, identifier("duration"), Type::Float);
    let body = block(&body, &env, emissions, &mut context)
        .map_err(|error| Diagnostic::new(span(), error.message))?;
    Ok((body, context.slots.into()))
}

fn block(
    body: &CheckedBlock,
    env: &Environment,
    emissions: &[EmittedReference],
    context: &mut CompileContext<'_>,
) -> Result<Block, RuntimeError> {
    let mut env = env.clone();
    body.statements
        .iter()
        .map(|item| statement(item, &mut env, emissions, context))
        .collect()
}

fn statement(
    item: &CheckedStmt,
    env: &mut Environment,
    emissions: &[EmittedReference],
    context: &mut CompileContext<'_>,
) -> Result<Statement, RuntimeError> {
    Ok(match item {
        CheckedStmt::Local {
            name,
            ty,
            initializer,
        } => {
            let value = match initializer {
                Some(value) => expression(value, env)?,
                None => Expression::Constant(ty.default_value()),
            };
            let slot = context.declare(env, name.clone(), ty.clone());
            Statement::Assign { slot, value }
        }
        CheckedStmt::Assign { name, value } => Statement::Assign {
            slot: env[name].slot,
            value: expression(value, env)?,
        },
        CheckedStmt::Expr(value) => Statement::Expression(expression(value, env)?),
        CheckedStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            if !super::super::staging::contains_emit(then_block)
                && !else_block
                    .as_ref()
                    .is_some_and(super::super::staging::contains_emit)
                && !context.preparation_controls.contains(condition)
            {
                return pure_control(item, env);
            }
            Statement::Branch {
                condition: fixed_expression(condition, env)?,
                then_block: block(then_block, env, emissions, context)?,
                else_block: else_block
                    .as_ref()
                    .map(|body| block(body, env, emissions, context))
                    .transpose()?
                    .unwrap_or_default(),
            }
        }
        CheckedStmt::For {
            initializer,
            condition,
            update,
            body,
        } => {
            if !super::super::staging::contains_emit(body)
                && !context.preparation_controls.contains(condition)
            {
                return pure_control(item, env);
            }
            let iterations = super::super::loop_bounds::fixed_for_iterations(
                initializer,
                condition,
                update,
                body,
            )
            .ok_or_else(|| error("generator for loop requires a compile-time iteration count"))?;
            let mut local = env.clone();
            let initializer = Box::new(statement(initializer, &mut local, emissions, context)?);
            Statement::For {
                initializer,
                iterations,
                update: Box::new(statement(update, &mut local, emissions, context)?),
                body: block(body, &local, emissions, context)?,
            }
        }
        CheckedStmt::ForMarks { index, marks, body } => {
            if !super::super::staging::contains_emit(body)
                && !context.preparation_controls.contains(marks)
            {
                return pure_control(item, env);
            }
            let marks = fixed_expression(marks, env)?;
            let mut local = env.clone();
            let index = context.declare(&mut local, index.clone(), Type::Int);
            Statement::Marks {
                index,
                marks,
                body: block(body, &local, emissions, context)?,
            }
        }
        CheckedStmt::ForRange {
            index,
            count,
            cap,
            body,
        } => {
            if !super::super::staging::contains_emit(body)
                && !context.preparation_controls.contains(count)
            {
                return pure_control(item, env);
            }
            let count = fixed_expression(count, env)?;
            let CheckedExprKind::Literal(Value::Int(cap)) = cap.kind else {
                return Err(error("range cap requires an integer literal"));
            };
            let mut local = env.clone();
            let index = context.declare(&mut local, index.clone(), Type::Int);
            Statement::Range {
                index,
                count,
                cap,
                body: block(body, &local, emissions, context)?,
            }
        }
        CheckedStmt::Emit { effect, fields } => {
            let mut start = None;
            let mut duration = None;
            let mut target = None;
            let mut params = Vec::new();
            for (name, expr) in fields {
                match name.as_str() {
                    "start" => start = Some(fixed_expression(expr, env)?),
                    "duration" => duration = Some(fixed_expression(expr, env)?),
                    "target" => target = Some(fixed_expression(expr, env)?),
                    _ => params.push((name.clone(), expression(expr, env)?)),
                }
            }
            let slot = emissions
                .iter()
                .position(|candidate| candidate.span == effect.span)
                .ok_or_else(|| error("emission is missing its numeric child slot"))?;
            Statement::Emit {
                slot: GeneratedEffectSlot(
                    u32::try_from(slot).map_err(|_| error("too many emitted definitions"))?,
                ),
                start: start.ok_or_else(|| error("emission is missing its start"))?,
                duration: duration.ok_or_else(|| error("emission is missing its duration"))?,
                target: target.ok_or_else(|| error("emission is missing its target"))?,
                params,
            }
        }
        CheckedStmt::Return(_) => return Err(error("generator return cannot produce a value")),
    })
}

fn expression(expr: &CheckedExpr, env: &Environment) -> Result<Expression, RuntimeError> {
    match &expr.kind {
        CheckedExprKind::Variable(name) if env.contains_key(name) => {
            return Ok(Expression::Read(env[name].slot));
        }
        CheckedExprKind::Literal(value) => return Ok(Expression::Constant(value.clone())),
        _ => {}
    }
    let mut lowering = Lowering::default();
    let lowered = lowering.expr(expr, &lexical_environment(env))?;
    let Calculation { program, inputs } = calculation(lowering, Vec::new(), vec![lowered])?;
    let program = program
        .into_output::<Value>()
        .ok_or_else(|| error("generator expression must produce exactly one value"))?;
    Ok(Expression::Calculate(Box::new(Calculation {
        program,
        inputs,
    })))
}

fn fixed_expression<O: super::super::CalculationOutput>(
    expr: &CheckedExpr,
    env: &Environment,
) -> Result<Box<FixedCalculation<O>>, RuntimeError> {
    let mut lowering = Lowering::default();
    let lowered = lowering.expr(expr, &lexical_environment(env))?;
    let Calculation { program, inputs } = calculation(lowering, Vec::new(), vec![lowered])?;
    if program.uses_time() {
        return Err(error(
            "structural generator expression cannot read playback time",
        ));
    }
    let program = program
        .into_output::<O>()
        .ok_or_else(|| error("structural generator expression has an invalid result type"))?;
    // This helper is used only for structural fields/control expressions after
    // staging has proved them fixed, including all loop-carried dependencies.
    let inputs = inputs.into_iter().map(FixedBindingSlot).collect();
    Ok(Box::new(Calculation { program, inputs }))
}

fn calculation(
    lowering: Lowering,
    statements: Vec<CheckedStmt>,
    outputs: Vec<CheckedExpr>,
) -> Result<Calculation, RuntimeError> {
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
    let program = super::super::compiler::compile_value(&params, statements, outputs)
        .map_err(|diagnostic| error(diagnostic.message))?;
    let inputs = lowering.inputs.into_iter().map(|(_, slot)| slot).collect();
    Ok(Calculation { program, inputs })
}

fn pure_control(statement: &CheckedStmt, env: &Environment) -> Result<Statement, RuntimeError> {
    let assigned = statement
        .assigned_names()
        .into_iter()
        .filter(|name| env.contains_key(name))
        .collect::<IndexSet<_>>();
    let mut lowering = Lowering::default();
    let mut lexical = lexical_environment(env);
    let mut statements = Vec::new();
    for name in &assigned {
        let symbol = &env[name];
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
            lowering.expr(
                &CheckedExpr {
                    kind: CheckedExprKind::Variable(name.clone()),
                    ty: env[name].ty.clone(),
                    span: span(),
                },
                &lexical,
            )
        })
        .collect::<Result<_, _>>()?;
    Ok(Statement::Calculate {
        assigned: assigned.iter().map(|name| env[name].slot).collect(),
        calculation: Box::new(calculation(lowering, statements, outputs)?),
    })
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
    inputs: Vec<(Type, BindingSlot)>,
    next_local: usize,
}
impl Lowering {
    fn local(&mut self) -> Identifier {
        let name = identifier(&format!("local_{}", self.next_local));
        self.next_local += 1;
        name
    }
    fn source(&mut self, symbol: &Symbol, span: TextSpan) -> CheckedExpr {
        let index = self
            .inputs
            .iter()
            .position(|(_, slot)| *slot == symbol.slot)
            .unwrap_or_else(|| {
                self.inputs.push((symbol.ty.clone(), symbol.slot));
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
            CheckedStmt::ForMarks { index, marks, body } => {
                let marks = self.expr(marks, env)?;
                let mut loop_env = env.clone();
                let local = self.local();
                loop_env.insert(index.clone(), LexicalValue::Local(local.clone()));
                CheckedStmt::ForMarks {
                    index: local,
                    marks,
                    body: self.block(body, &loop_env)?,
                }
            }
            CheckedStmt::ForRange {
                index,
                count,
                cap,
                body,
            } => {
                let count = self.expr(count, env)?;
                let cap = self.expr(cap, env)?;
                let mut loop_env = env.clone();
                let local = self.local();
                loop_env.insert(index.clone(), LexicalValue::Local(local.clone()));
                CheckedStmt::ForRange {
                    index: local,
                    count,
                    cap,
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

fn span() -> TextSpan {
    TextSpan { start: 0, end: 0 }
}
fn input_name(index: usize) -> Identifier {
    identifier(&format!("input_{index}"))
}
pub(super) fn identifier(name: &str) -> Identifier {
    Identifier::new(name.to_owned())
        .unwrap_or_else(|_| unreachable!("compiler-generated identifier is valid"))
}
fn error(message: impl Into<String>) -> RuntimeError {
    RuntimeError {
        message: message.into(),
    }
}

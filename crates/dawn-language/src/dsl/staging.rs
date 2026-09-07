//! Declaration-based preparation dependencies over the checked program.
use super::checked::{CheckedBlock, CheckedExpr, CheckedExprKind, CheckedStmt};
use super::{Diagnostic, Identifier, ParamDecl};
use indexmap::IndexMap;

type Dependency = Option<Identifier>;
#[derive(Clone, Debug, PartialEq)]
struct Binding {
    dependency: Dependency,
    fixed: bool,
}
type Environment = IndexMap<Identifier, Binding>;

pub(super) fn check(
    params: &[ParamDecl],
    body: &mut CheckedBlock,
    generator: bool,
) -> Result<(), Vec<Diagnostic>> {
    let mut checker = Checker {
        generator,
        diagnostics: Vec::new(),
        emitted: IndexMap::new(),
    };
    let mut env = params
        .iter()
        .map(|param| {
            (
                param.name.clone(),
                Binding {
                    dependency: (!param.fixed).then(|| param.name.clone()),
                    fixed: param.fixed,
                },
            )
        })
        .collect();
    checker.block(body, &mut env, &None);
    annotate(body, &checker.emitted);
    if checker.diagnostics.is_empty() {
        Ok(())
    } else {
        Err(checker.diagnostics)
    }
}

struct Checker {
    generator: bool,
    diagnostics: Vec<Diagnostic>,
    emitted: IndexMap<(usize, usize), Dependency>,
}

impl Checker {
    fn require_fixed(&mut self, expr: &CheckedExpr, dependency: &Dependency, destination: &str) {
        if let Some(origin) = dependency {
            let diagnostic = Diagnostic::new(
                expr.span,
                format!(
                    "live dependency `{}` reaches {destination}, which requires a fixed value",
                    origin.as_str()
                ),
            );
            if !self.diagnostics.contains(&diagnostic) {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    fn expr(&mut self, expr: &CheckedExpr, env: &Environment) -> Dependency {
        match &expr.kind {
            CheckedExprKind::Literal(_) => None,
            CheckedExprKind::Variable(name) => {
                env.get(name).and_then(|value| value.dependency.clone())
            }
            CheckedExprKind::Array(items) => items.iter().fold(None, |dependency, item| {
                let next = self.expr(item, env);
                dependency.or(next)
            }),
            CheckedExprKind::Index { target, index }
            | CheckedExprKind::Binary {
                left: target,
                right: index,
                ..
            } => {
                let target = self.expr(target, env);
                let index = self.expr(index, env);
                target.or(index)
            }
            CheckedExprKind::Member { target, .. }
            | CheckedExprKind::Unary { expr: target, .. } => self.expr(target, env),
            CheckedExprKind::Call { callee, args } => {
                let mut dependency = match &callee.kind {
                    CheckedExprKind::Variable(name) => match name.as_str() {
                        "seconds" | "progress" => Some(name.clone()),
                        "pixel_index" | "pixel_count" | "pixel_fraction" | "section_position" => {
                            if self.generator {
                                let diagnostic = Diagnostic::new(
                                    expr.span,
                                    "generator calculations cannot read pixel context; move this calculation into a child sample effect",
                                );
                                if !self.diagnostics.contains(&diagnostic) {
                                    self.diagnostics.push(diagnostic);
                                }
                            }
                            Some(name.clone())
                        }
                        _ => None,
                    },
                    _ => self.expr(callee, env),
                };
                for arg in args {
                    let next = self.expr(arg, env);
                    dependency = dependency.or(next);
                }
                dependency
            }
            CheckedExprKind::SignalSample { input, seconds, .. } => {
                self.expr(seconds, env);
                Some(input.clone())
            }
        }
    }

    fn block(&mut self, block: &CheckedBlock, env: &mut Environment, control: &Dependency) {
        // Locals shadow outer bindings and disappear when their block exits.
        let mut shadowed = IndexMap::new();
        for statement in &block.statements {
            if let CheckedStmt::Local { name, .. } = statement {
                shadowed
                    .entry(name.clone())
                    .or_insert_with(|| env.get(name).cloned());
            }
            self.statement(statement, env, control);
        }
        for (name, previous) in shadowed {
            if let Some(previous) = previous {
                env.insert(name, previous);
            } else {
                env.shift_remove(&name);
            }
        }
    }

    fn statement(&mut self, statement: &CheckedStmt, env: &mut Environment, control: &Dependency) {
        match statement {
            CheckedStmt::Local {
                name, initializer, ..
            } => {
                let dependency = initializer.as_ref().and_then(|expr| self.expr(expr, env));
                env.insert(
                    name.clone(),
                    Binding {
                        dependency: dependency.or_else(|| control.clone()),
                        fixed: false,
                    },
                );
            }
            CheckedStmt::Assign { name, value } => {
                let dependency = self.expr(value, env).or_else(|| control.clone());
                if env.get(name).is_some_and(|value| value.fixed) {
                    self.require_fixed(
                        value,
                        &dependency,
                        &format!("fixed parameter `{}`", name.as_str()),
                    );
                }
                if let Some(binding) = env.get_mut(name) {
                    binding.dependency = dependency;
                }
            }
            CheckedStmt::Expr(expr) | CheckedStmt::Return(expr) => {
                self.expr(expr, env);
            }
            CheckedStmt::If {
                condition,
                then_block,
                else_block,
            } => {
                let dependency = self.expr(condition, env).or_else(|| control.clone());
                if self.generator
                    && (contains_emit(then_block) || else_block.as_ref().is_some_and(contains_emit))
                {
                    self.require_fixed(
                        condition,
                        &dependency,
                        "control flow determining child emission",
                    );
                }
                let mut left = env.clone();
                let mut right = env.clone();
                self.block(then_block, &mut left, &dependency);
                if let Some(block) = else_block {
                    self.block(block, &mut right, &dependency);
                }
                for (name, value) in env.iter_mut() {
                    value.dependency = left
                        .get(name)
                        .and_then(|value| value.dependency.clone())
                        .or_else(|| right.get(name).and_then(|value| value.dependency.clone()));
                }
            }
            CheckedStmt::For {
                initializer,
                condition,
                update,
                body,
            } => {
                let mut loop_env = env.clone();
                self.statement(initializer, &mut loop_env, control);
                let entry = loop_env.clone();
                // Compute a monotone fixed point to include every loop-carried dependency.
                loop {
                    let before = loop_env.clone();
                    let dependency = self.expr(condition, &loop_env).or_else(|| control.clone());
                    if self.generator && contains_emit(body) {
                        self.require_fixed(
                            condition,
                            &dependency,
                            "loop controlling child emission",
                        );
                    }
                    self.block(body, &mut loop_env, &dependency);
                    self.statement(update, &mut loop_env, &dependency);
                    for (name, value) in loop_env.iter_mut() {
                        value.dependency = before
                            .get(name)
                            .and_then(|value| value.dependency.clone())
                            .or_else(|| value.dependency.clone());
                    }
                    if loop_env == before {
                        break;
                    }
                }
                for (name, value) in env.iter_mut() {
                    if matches!(initializer.as_ref(), CheckedStmt::Local { name: local, .. } if local == name)
                    {
                        continue;
                    }
                    value.dependency = entry
                        .get(name)
                        .and_then(|value| value.dependency.clone())
                        .or_else(|| {
                            loop_env
                                .get(name)
                                .and_then(|value| value.dependency.clone())
                        });
                }
            }
            CheckedStmt::Emit { fields, .. } => {
                for (name, expr) in fields {
                    let dependency = self.expr(expr, env).or_else(|| control.clone());
                    let stored = self
                        .emitted
                        .entry((expr.span.start, expr.span.end))
                        .or_default();
                    *stored = stored.clone().or_else(|| dependency.clone());
                    if matches!(name.as_str(), "start" | "duration" | "target") {
                        self.require_fixed(expr, &dependency, &format!("emit `{}`", name.as_str()));
                    }
                }
            }
        }
    }
}

pub(super) fn contains_emit(block: &CheckedBlock) -> bool {
    block.statements.iter().any(|statement| match statement {
        CheckedStmt::Emit { .. } => true,
        CheckedStmt::If {
            then_block,
            else_block,
            ..
        } => contains_emit(then_block) || else_block.as_ref().is_some_and(contains_emit),
        CheckedStmt::For { body, .. } => contains_emit(body),
        _ => false,
    })
}

fn annotate(block: &mut CheckedBlock, emitted: &IndexMap<(usize, usize), Dependency>) {
    for statement in &mut block.statements {
        match statement {
            CheckedStmt::Emit { effect, fields } => {
                effect.arguments = fields
                    .iter()
                    .map(|(name, expr)| super::EmittedArgument {
                        name: name.clone(),
                        ty: expr.ty.clone(),
                        span: expr.span,
                        live_dependency: emitted
                            .get(&(expr.span.start, expr.span.end))
                            .cloned()
                            .flatten(),
                    })
                    .collect();
            }
            CheckedStmt::If {
                then_block,
                else_block,
                ..
            } => {
                annotate(then_block, emitted);
                if let Some(block) = else_block {
                    annotate(block, emitted);
                }
            }
            CheckedStmt::For { body, .. } => annotate(body, emitted),
            _ => {}
        }
    }
}

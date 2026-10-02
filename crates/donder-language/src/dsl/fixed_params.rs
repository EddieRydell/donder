//! Prevent live values from being assigned back into fixed parameters.
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

pub(super) fn check(params: &[ParamDecl], body: &CheckedBlock) -> Result<(), Vec<Diagnostic>> {
    let mut checker = Checker {
        diagnostics: Vec::new(),
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
    if checker.diagnostics.is_empty() {
        Ok(())
    } else {
        Err(checker.diagnostics)
    }
}

struct Checker {
    diagnostics: Vec<Diagnostic>,
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
            CheckedExprKind::Unary { expr: target, .. } => self.expr(target, env),
            CheckedExprKind::Call { callee, args } => {
                let mut dependency = match &callee.kind {
                    CheckedExprKind::Variable(name) => match name.as_str() {
                        "seconds" | "progress" => Some(name.clone()),
                        "pixel_index" | "pixel_count" | "pixel_fraction" | "pixel_x"
                        | "pixel_y" | "target_min_x" | "target_min_y" | "target_max_x"
                        | "target_max_y" | "section_position" | "section_count"
                        | "section_index" => Some(name.clone()),
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
            CheckedStmt::ForMarks {
                index,
                marks: collection,
                body,
            }
            | CheckedStmt::ForRange {
                index,
                count: collection,
                body,
                ..
            } => {
                let dependency = self.expr(collection, env).or_else(|| control.clone());
                let mut loop_env = env.clone();
                loop_env.insert(
                    index.clone(),
                    Binding {
                        dependency: dependency.clone(),
                        fixed: false,
                    },
                );
                loop {
                    let before = loop_env.clone();
                    self.block(body, &mut loop_env, &dependency);
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
                    value.dependency = value.dependency.clone().or_else(|| {
                        loop_env
                            .get(name)
                            .and_then(|value| value.dependency.clone())
                    });
                }
            }
        }
    }
}

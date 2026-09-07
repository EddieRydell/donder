use super::ast::{
    BinaryOp, Block, EffectDecl, Expr, ExprKind, Module, OperatorDecl, ParamDecl, Stmt, UnaryOp,
};
use super::checked::{
    CheckedBlock, CheckedEffectDecl, CheckedExpr, CheckedExprKind, CheckedModule,
    CheckedOperatorDecl, CheckedStmt,
};
use super::diagnostic::Diagnostic;
use super::lexer::TextSpan;
use super::types::{Identifier, Type, Value};
use indexmap::IndexMap;

pub(crate) fn check_module(module: Module) -> Result<CheckedModule, Vec<Diagnostic>> {
    let mut checker = Checker {
        diagnostics: Vec::new(),
    };
    let effects = module
        .effects
        .into_iter()
        .map(|effect| checker.check_effect(effect))
        .collect();
    let operators = module
        .operators
        .into_iter()
        .map(|operator| checker.check_operator(operator))
        .collect();
    if checker.diagnostics.is_empty() {
        Ok(CheckedModule { effects, operators })
    } else {
        Err(checker.diagnostics)
    }
}

struct Checker {
    diagnostics: Vec<Diagnostic>,
}

impl Checker {
    fn check_operator(&mut self, operator: OperatorDecl) -> CheckedOperatorDecl {
        if operator.inputs.is_empty() {
            self.error(
                TextSpan { start: 0, end: 0 },
                "operator must declare at least one Signal input",
            );
        }
        if operator.entrypoint.name.as_str() != "sample" {
            self.error(
                TextSpan { start: 0, end: 0 },
                "operator entrypoint must be named `sample`",
            );
        }
        if operator.entrypoint.return_type != Type::Color {
            self.error(
                TextSpan { start: 0, end: 0 },
                "operator entrypoint must return `Color`",
            );
        }
        if !operator.entrypoint.params.is_empty() {
            self.error(
                TextSpan { start: 0, end: 0 },
                "operator entrypoint does not accept arguments",
            );
        }

        let mut env = IndexMap::new();
        for input in &operator.inputs {
            if env.insert(input.name.clone(), Type::Signal).is_some() {
                self.error(
                    TextSpan { start: 0, end: 0 },
                    format!("duplicate operator input `{}`", input.name.as_str()),
                );
            }
        }
        for param in &operator.params {
            self.check_param(param);
            if env.insert(param.name.clone(), param.ty.clone()).is_some() {
                self.error(
                    TextSpan { start: 0, end: 0 },
                    format!("duplicate operator member `{}`", param.name.as_str()),
                );
            }
        }
        let (body, returns) =
            self.check_block(operator.entrypoint.body.clone(), &mut env, &Type::Color);
        if !returns {
            self.error(
                TextSpan { start: 0, end: 0 },
                "`sample` must return a color on all paths",
            );
        }
        CheckedOperatorDecl {
            name: operator.name,
            inputs: operator.inputs,
            params: operator.params,
            entrypoint: operator.entrypoint,
            body,
        }
    }

    fn check_effect(&mut self, effect: EffectDecl) -> CheckedEffectDecl {
        let is_sample = effect.entrypoint.name.as_str() == "sample";
        let is_generator = effect.entrypoint.name.as_str() == "generate";
        if !is_sample && !is_generator {
            self.error(
                TextSpan { start: 0, end: 0 },
                "effect entrypoint must be named `sample` or `generate`",
            );
        }
        let expected_return = if is_generator {
            Type::Void
        } else {
            Type::Color
        };
        if effect.entrypoint.return_type != expected_return {
            self.error(
                TextSpan { start: 0, end: 0 },
                format!("effect entrypoint must return `{expected_return:?}`"),
            );
        }
        if !effect.entrypoint.params.is_empty() {
            for param in &effect.entrypoint.params {
                self.error(
                    TextSpan { start: 0, end: 0 },
                    format!(
                        "entrypoint does not accept arguments; remove `{:?} {}`",
                        param.ty,
                        param.name.as_str()
                    ),
                );
            }
        }

        let mut env = IndexMap::new();
        for param in &effect.params {
            self.check_param(param);
            env.insert(param.name.clone(), param.ty.clone());
        }
        if is_generator {
            env.insert(static_identifier("timeline"), Type::Timeline);
            env.insert(static_identifier("target"), Type::Target);
            env.insert(static_identifier("duration"), Type::Float);
        }

        let (body, returns) =
            self.check_block(effect.entrypoint.body.clone(), &mut env, &expected_return);
        if is_sample && !returns {
            self.error(
                TextSpan { start: 0, end: 0 },
                "`sample` must return a color on all paths",
            );
        }
        CheckedEffectDecl {
            name: effect.name,
            params: effect.params,
            entrypoint: effect.entrypoint,
            body,
        }
    }

    fn check_param(&mut self, param: &ParamDecl) {
        if matches!(&param.ty, Type::Enum(options) if options.is_empty()) {
            self.error(
                TextSpan { start: 0, end: 0 },
                format!(
                    "enum param `{}` must declare an option",
                    param.name.as_str()
                ),
            );
        }
        let mut item_type = &param.ty;
        while let Type::Array(inner) = item_type {
            item_type = inner;
        }
        if matches!(
            item_type,
            Type::Signal | Type::Timeline | Type::Target | Type::TargetItems | Type::TargetItem
        ) {
            self.error(
                TextSpan { start: 0, end: 0 },
                format!(
                    "`{}` uses a context-only type and cannot be a param",
                    param.name.as_str()
                ),
            );
        }
        match (&param.ty, &param.default) {
            (_, None) => {}
            (ty, Some(value)) if value_matches_type(value, ty) => {}
            (Type::Float, Some(Value::Int(_))) => {}
            (Type::Enum(options), Some(Value::Enum(value))) => {
                if !options.iter().any(|option| option == value) {
                    self.error(
                        TextSpan { start: 0, end: 0 },
                        format!("enum default `{}` is not an option", value.as_str()),
                    );
                }
            }
            (_, Some(_)) => {
                self.error(
                    TextSpan { start: 0, end: 0 },
                    format!(
                        "default for `{}` does not match declared type",
                        param.name.as_str()
                    ),
                );
            }
        }
    }

    fn check_block(
        &mut self,
        block: Block,
        env: &mut IndexMap<Identifier, Type>,
        return_type: &Type,
    ) -> (CheckedBlock, bool) {
        let mut returned = false;
        let mut statements = Vec::with_capacity(block.statements.len());
        for statement in block.statements {
            let (statement, returns) = self.check_statement(statement, env, return_type);
            statements.push(statement);
            if returns {
                returned = true;
                break;
            }
        }
        (CheckedBlock { statements }, returned)
    }

    fn check_statement(
        &mut self,
        statement: Stmt,
        env: &mut IndexMap<Identifier, Type>,
        return_type: &Type,
    ) -> (CheckedStmt, bool) {
        match statement {
            Stmt::Local {
                ty,
                name,
                initializer,
            } => {
                if type_contains_signal(&ty) {
                    self.error(
                        TextSpan { start: 0, end: 0 },
                        "Signal can only be declared as an operator input",
                    );
                }
                let initializer = initializer.map(|initializer| {
                    let checked = self.check_expr(initializer, env, Some(&ty));
                    self.require_assignable(&ty, &checked.ty, checked.span);
                    checked
                });
                env.insert(name.clone(), ty.clone());
                (
                    CheckedStmt::Local {
                        ty,
                        name,
                        initializer,
                    },
                    false,
                )
            }
            Stmt::Assign { name, value } => {
                let Some(target_type) = env.get(&name).cloned() else {
                    self.error(
                        value.span,
                        format!("unknown assignment target `{}`", name.as_str()),
                    );
                    let checked = self.check_expr(value, env, None);
                    return (
                        CheckedStmt::Assign {
                            name,
                            value: checked,
                        },
                        false,
                    );
                };
                let checked = self.check_expr(value, env, Some(&target_type));
                self.require_assignable(&target_type, &checked.ty, checked.span);
                (
                    CheckedStmt::Assign {
                        name,
                        value: checked,
                    },
                    false,
                )
            }
            Stmt::Expr(expr) => (CheckedStmt::Expr(self.check_expr(expr, env, None)), false),
            Stmt::If {
                condition,
                then_block,
                else_block,
            } => {
                let condition = self.check_expr(condition, env, Some(&Type::Bool));
                self.require_assignable(&Type::Bool, &condition.ty, condition.span);
                let mut then_env = env.clone();
                let (then_block, then_returns) =
                    self.check_block(then_block, &mut then_env, return_type);
                let (else_block, else_returns) = if let Some(else_block) = else_block {
                    let mut else_env = env.clone();
                    let (block, returns) = self.check_block(else_block, &mut else_env, return_type);
                    (Some(block), returns)
                } else {
                    (None, false)
                };
                (
                    CheckedStmt::If {
                        condition,
                        then_block,
                        else_block,
                    },
                    then_returns && else_returns,
                )
            }
            Stmt::For {
                initializer,
                condition,
                update,
                body,
            } => {
                let mut loop_env = env.clone();
                let (initializer, _) =
                    self.check_statement(*initializer, &mut loop_env, return_type);
                let condition = self.check_expr(condition, &mut loop_env, Some(&Type::Bool));
                self.require_assignable(&Type::Bool, &condition.ty, condition.span);
                let (update, _) = self.check_statement(*update, &mut loop_env, return_type);
                let (body, _) = self.check_block(body, &mut loop_env, return_type);
                (
                    CheckedStmt::For {
                        initializer: Box::new(initializer),
                        condition,
                        update: Box::new(update),
                        body,
                    },
                    false,
                )
            }
            Stmt::Emit { effect, fields } => (
                CheckedStmt::Emit {
                    effect,
                    fields: fields
                        .into_iter()
                        .map(|(name, value)| (name, self.check_expr(value, env, None)))
                        .collect(),
                },
                false,
            ),
            Stmt::Return(expr) => {
                let checked = self.check_expr(expr, env, Some(return_type));
                self.require_assignable(return_type, &checked.ty, checked.span);
                (CheckedStmt::Return(checked), true)
            }
        }
    }

    fn check_expr(
        &mut self,
        expr: Expr,
        env: &mut IndexMap<Identifier, Type>,
        expected: Option<&Type>,
    ) -> CheckedExpr {
        let span = expr.span;
        let (kind, ty) = match expr.kind {
            ExprKind::Literal(value) => {
                let ty = type_of_value(&value);
                (CheckedExprKind::Literal(value), ty)
            }
            ExprKind::Variable(name) => {
                let ty = env.get(&name).cloned().unwrap_or_else(|| {
                    if matches!(name.as_str(), "PI" | "TAU") {
                        Type::Float
                    } else {
                        Type::Enum(vec![name.clone()])
                    }
                });
                (CheckedExprKind::Variable(name), ty)
            }
            ExprKind::Array(items) => {
                if items.is_empty() {
                    let ty = if let Some(Type::Array(item_type)) = expected {
                        Type::Array(item_type.clone())
                    } else {
                        self.error(span, "empty array literal needs an array type context");
                        Type::Array(Box::new(Type::Void))
                    };
                    (CheckedExprKind::Array(Vec::new()), ty)
                } else {
                    let expected_item = expected.and_then(|ty| match ty {
                        Type::Array(item) => Some(item.as_ref()),
                        _ => None,
                    });
                    let mut checked_items = Vec::with_capacity(items.len());
                    let mut items_iter = items.into_iter();
                    let first = self.check_expr(
                        items_iter.next().unwrap_or_else(|| unreachable!()),
                        env,
                        expected_item,
                    );
                    let first_type = first.ty.clone();
                    checked_items.push(first);
                    for item in items_iter {
                        let checked = self.check_expr(item, env, Some(&first_type));
                        self.require_assignable(&first_type, &checked.ty, checked.span);
                        checked_items.push(checked);
                    }
                    (
                        CheckedExprKind::Array(checked_items),
                        Type::Array(Box::new(first_type)),
                    )
                }
            }
            ExprKind::Index { target, index } => {
                let target = self.check_expr(*target, env, None);
                let index = match &target.ty {
                    Type::Array(_) => self.check_expr(*index, env, Some(&Type::Int)),
                    Type::Curve => self.check_expr(*index, env, Some(&Type::Float)),
                    Type::Gradient => self.check_expr(*index, env, Some(&Type::Float)),
                    _ => self.check_expr(*index, env, None),
                };
                let ty = match &target.ty {
                    Type::Array(item) => {
                        self.require_assignable(&Type::Int, &index.ty, index.span);
                        item.as_ref().clone()
                    }
                    Type::Curve => {
                        self.require_assignable(&Type::Float, &index.ty, index.span);
                        Type::Float
                    }
                    Type::Gradient => {
                        self.require_assignable(&Type::Float, &index.ty, index.span);
                        Type::Color
                    }
                    _ => {
                        self.error(
                            target.span,
                            "indexing requires an array, curve, or gradient",
                        );
                        Type::Void
                    }
                };
                (
                    CheckedExprKind::Index {
                        target: Box::new(target),
                        index: Box::new(index),
                    },
                    ty,
                )
            }
            ExprKind::Member { target, member } => {
                let target = self.check_expr(*target, env, None);
                let ty = match &target.ty {
                    Type::TargetItem => match member.as_str() {
                        "element_index" | "element_cell_index" | "pixel_index" | "pixel_count" => {
                            Type::Int
                        }
                        "pixel_fraction" => Type::Float,
                        _ => {
                            self.error(span, "unknown TargetItem member");
                            Type::Void
                        }
                    },
                    Type::Signal => {
                        self.error(span, "Signal only supports `at(float)`");
                        Type::Void
                    }
                    _ => {
                        self.error(target.span, "member access requires TargetItem");
                        Type::Void
                    }
                };
                (
                    CheckedExprKind::Member {
                        target: Box::new(target),
                        member,
                    },
                    ty,
                )
            }
            ExprKind::Call { callee, args } => {
                if let ExprKind::Member { target, member } = &callee.kind
                    && matches!(member.as_str(), "at" | "at_global")
                    && let ExprKind::Variable(input) = &target.kind
                    && env.get(input) == Some(&Type::Signal)
                {
                    use super::bytecode::SignalPixel;
                    let global = member.as_str() == "at_global";
                    if global {
                        self.require_arg_count("Signal.at_global", args.len(), 2, span);
                    } else if !(1..=2).contains(&args.len()) {
                        self.error(
                            span,
                            "Signal.at requires a time and optionally a local pixel index",
                        );
                    }
                    let seconds = args.first().cloned().unwrap_or(Expr {
                        kind: ExprKind::Literal(Value::Float(0.0)),
                        span,
                    });
                    let seconds = self.check_expr(seconds, env, Some(&Type::Float));
                    self.require_assignable(&Type::Float, &seconds.ty, seconds.span);
                    let pixel = if let Some(index) = args.get(1) {
                        let index = self.check_expr(index.clone(), env, Some(&Type::Int));
                        self.require_assignable(&Type::Int, &index.ty, index.span);
                        if global {
                            SignalPixel::Global(Box::new(index))
                        } else {
                            SignalPixel::Local(Box::new(index))
                        }
                    } else {
                        SignalPixel::Current
                    };
                    return CheckedExpr {
                        kind: CheckedExprKind::SignalSample {
                            input: input.clone(),
                            seconds: Box::new(seconds),
                            pixel,
                        },
                        span,
                        ty: Type::Color,
                    };
                }
                let callee = self.check_expr(*callee, env, None);
                let ty = if let CheckedExprKind::Variable(name) = &callee.kind {
                    self.check_builtin_call(name.as_str(), &args, env, span)
                } else {
                    self.error(callee.span, "call target must be a builtin");
                    Type::Void
                };
                let args = self.check_call_args(&callee, args, env);
                (
                    CheckedExprKind::Call {
                        callee: Box::new(callee),
                        args,
                    },
                    ty,
                )
            }
            ExprKind::Unary { op, expr: inner } => {
                let inner = self.check_expr(*inner, env, None);
                let ty = match op {
                    UnaryOp::Negate if numeric(&inner.ty) => inner.ty.clone(),
                    UnaryOp::Not if inner.ty == Type::Bool => Type::Bool,
                    UnaryOp::Negate => {
                        self.error(span, "unary `-` requires int or float");
                        Type::Void
                    }
                    UnaryOp::Not => {
                        self.error(span, "unary `!` requires bool");
                        Type::Void
                    }
                };
                (
                    CheckedExprKind::Unary {
                        op,
                        expr: Box::new(inner),
                    },
                    ty,
                )
            }
            ExprKind::Binary { op, left, right } => {
                let left = self.check_expr(*left, env, None);
                let right = self.check_expr(*right, env, Some(&left.ty));
                let ty = self.check_binary(op, &left.ty, &right.ty, span);
                (
                    CheckedExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    ty,
                )
            }
        };
        CheckedExpr { kind, span, ty }
    }

    fn check_call_args(
        &mut self,
        callee: &CheckedExpr,
        args: Vec<Expr>,
        env: &mut IndexMap<Identifier, Type>,
    ) -> Vec<CheckedExpr> {
        let name = match &callee.kind {
            CheckedExprKind::Variable(name) => name.as_str(),
            _ => {
                return args
                    .into_iter()
                    .map(|arg| self.check_expr(arg, env, None))
                    .collect();
            }
        };
        args.into_iter()
            .enumerate()
            .map(|(index, arg)| {
                let expected = builtin_arg_type(name, index);
                self.check_expr(arg, env, expected.as_ref())
            })
            .collect()
    }

    fn check_builtin_call(
        &mut self,
        name: &str,
        args: &[Expr],
        env: &mut IndexMap<Identifier, Type>,
        span: TextSpan,
    ) -> Type {
        match name {
            "progress" | "seconds" | "duration" | "pixel_fraction" => {
                self.require_arg_count(name, args.len(), 0, span);
                Type::Float
            }
            "pixel_index" | "pixel_count" => {
                self.require_arg_count(name, args.len(), 0, span);
                Type::Int
            }
            "section_position" => {
                self.require_arg_count(name, args.len(), 1, span);
                self.require_arg(args, 0, &Type::Float, env);
                Type::Float
            }
            "sin" | "cos" | "abs" | "floor" => {
                self.require_arg_count(name, args.len(), 1, span);
                self.require_arg(args, 0, &Type::Float, env);
                Type::Float
            }
            "min" => {
                self.require_arg_count(name, args.len(), 2, span);
                self.require_arg(args, 0, &Type::Float, env);
                self.require_arg(args, 1, &Type::Float, env);
                Type::Float
            }
            "max" => {
                self.require_arg_count(name, args.len(), 2, span);
                let first = self.require_arg_any(args, 0, env);
                if first == Type::Color {
                    self.require_arg(args, 1, &Type::Color, env);
                    Type::Color
                } else {
                    self.require_assignable(&Type::Float, &first, span);
                    self.require_arg(args, 1, &Type::Float, env);
                    Type::Float
                }
            }
            "intensity" => {
                self.require_arg_count(name, args.len(), 1, span);
                self.require_arg(args, 0, &Type::Color, env);
                Type::Float
            }
            "invert" => {
                self.require_arg_count(name, args.len(), 1, span);
                self.require_arg(args, 0, &Type::Color, env);
                Type::Color
            }
            "clamp" | "smoothstep" => {
                self.require_arg_count(name, args.len(), 3, span);
                for index in 0..3 {
                    self.require_arg(args, index, &Type::Float, env);
                }
                Type::Float
            }
            "mix" => {
                self.require_arg_count(name, args.len(), 3, span);
                let first = self.require_arg_any(args, 0, env);
                let second = self.require_arg_any(args, 1, env);
                self.require_assignable(&first, &second, span);
                self.require_arg(args, 2, &Type::Float, env);
                first
            }
            "rgb" | "hsv" => {
                self.require_arg_count(name, args.len(), 3, span);
                for index in 0..3 {
                    self.require_arg(args, index, &Type::Float, env);
                }
                Type::Color
            }
            "srand" => {
                self.require_arg_count(name, args.len(), 1, span);
                self.require_arg(args, 0, &Type::Float, env);
                Type::Float
            }
            "rand" => {
                for index in 0..args.len() {
                    self.require_arg(args, index, &Type::Float, env);
                }
                Type::Float
            }
            "curve_crossing" => {
                if args.len() != 2 && args.len() != 3 {
                    self.error(span, "`curve_crossing` expects 2 or 3 arguments");
                }
                self.require_arg(args, 0, &Type::Curve, env);
                self.require_arg(args, 1, &Type::Float, env);
                if args.len() == 3 {
                    self.require_arg(args, 2, &Type::Float, env);
                }
                Type::Float
            }
            "curve_clamped" => {
                self.require_arg_count(name, args.len(), 4, span);
                self.require_arg(args, 0, &Type::Curve, env);
                for index in 1..4 {
                    self.require_arg(args, index, &Type::Float, env);
                }
                Type::Float
            }
            "gradient_color_scaled" => {
                self.require_arg_count(name, args.len(), 3, span);
                self.require_arg(args, 0, &Type::Gradient, env);
                self.require_arg(args, 1, &Type::Float, env);
                self.require_arg(args, 2, &Type::Float, env);
                Type::Color
            }
            "len" => {
                self.require_arg_count(name, args.len(), 1, span);
                let _ = self.require_arg_any(args, 0, env);
                Type::Int
            }
            "mark_count" | "mark_prev_index" | "mark_next_index" => {
                self.require_mark_args(name, args, env, span);
                Type::Int
            }
            "mark_at" | "mark_prev" | "mark_elapsed" | "mark_phase" => {
                self.require_mark_args(name, args, env, span);
                Type::Float
            }
            "fixtures" | "pixels" => {
                self.require_arg_count(name, args.len(), 1, span);
                self.require_arg(args, 0, &Type::Target, env);
                Type::TargetItems
            }
            "sections" => {
                self.require_arg_count(name, args.len(), 2, span);
                self.require_arg(args, 0, &Type::Target, env);
                self.require_arg(args, 1, &Type::Float, env);
                Type::TargetItems
            }
            "count" => {
                self.require_arg_count(name, args.len(), 1, span);
                self.require_arg(args, 0, &Type::TargetItems, env);
                Type::Int
            }
            "pick" => {
                self.require_arg_count(name, args.len(), 2, span);
                self.require_arg(args, 0, &Type::TargetItems, env);
                self.require_arg(args, 1, &Type::Float, env);
                Type::TargetItem
            }
            _ => {
                self.error(span, format!("unknown function `{name}`"));
                Type::Void
            }
        }
    }

    fn require_mark_args(
        &mut self,
        name: &str,
        args: &[Expr],
        env: &mut IndexMap<Identifier, Type>,
        span: TextSpan,
    ) {
        if args.is_empty() {
            self.error(
                span,
                format!("`{name}` requires marks as the first argument"),
            );
            return;
        }
        if matches!(name, "mark_count") {
            self.require_arg_count(name, args.len(), 1, span);
            self.require_arg(args, 0, &Type::Marks, env);
            return;
        }
        self.require_arg(args, 0, &Type::Marks, env);
    }

    fn check_binary(&mut self, op: BinaryOp, left: &Type, right: &Type, span: TextSpan) -> Type {
        match op {
            BinaryOp::Add if left == &Type::Color && right == &Type::Color => Type::Color,
            BinaryOp::Multiply
                if (left == &Type::Color
                    && matches!(right, Type::Color | Type::Float | Type::Int))
                    || (right == &Type::Color && matches!(left, Type::Float | Type::Int)) =>
            {
                Type::Color
            }
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Remainder => {
                if numeric(left) && numeric(right) {
                    if op == BinaryOp::Divide || left == &Type::Float || right == &Type::Float {
                        Type::Float
                    } else {
                        Type::Int
                    }
                } else {
                    self.error(span, "arithmetic operators require numeric operands");
                    Type::Void
                }
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                if numeric(left) && numeric(right) {
                    Type::Bool
                } else {
                    self.error(span, "comparison operators require numeric operands");
                    Type::Bool
                }
            }
            BinaryOp::Equal | BinaryOp::NotEqual => {
                self.require_assignable(left, right, span);
                Type::Bool
            }
            BinaryOp::And | BinaryOp::Or => {
                self.require_assignable(&Type::Bool, left, span);
                self.require_assignable(&Type::Bool, right, span);
                Type::Bool
            }
        }
    }

    fn require_arg_count(&mut self, name: &str, actual: usize, expected: usize, span: TextSpan) {
        if actual != expected {
            self.error(
                span,
                format!("`{name}` expects {expected} arguments, got {actual}"),
            );
        }
    }

    fn require_arg(
        &mut self,
        args: &[Expr],
        index: usize,
        expected: &Type,
        env: &mut IndexMap<Identifier, Type>,
    ) {
        if let Some(arg) = args.get(index) {
            let actual = self.check_expr(arg.clone(), env, Some(expected));
            self.require_assignable(expected, &actual.ty, arg.span);
        }
    }

    fn require_arg_any(
        &mut self,
        args: &[Expr],
        index: usize,
        env: &mut IndexMap<Identifier, Type>,
    ) -> Type {
        args.get(index)
            .map(|arg| self.check_expr(arg.clone(), env, None).ty)
            .unwrap_or(Type::Void)
    }

    fn require_assignable(&mut self, expected: &Type, actual: &Type, span: TextSpan) {
        if expected == actual || (expected == &Type::Float && actual == &Type::Int) {
            return;
        }
        if let (Type::Enum(options), Type::Enum(actual_options)) = (expected, actual)
            && actual_options
                .iter()
                .all(|value| options.iter().any(|option| option == value))
        {
            return;
        }
        self.error(span, format!("expected {expected:?}, got {actual:?}"));
    }

    fn error(&mut self, span: TextSpan, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::new(span, message));
    }
}

fn builtin_arg_type(name: &str, index: usize) -> Option<Type> {
    match name {
        "progress" | "seconds" | "duration" | "pixel_index" | "pixel_count" | "pixel_fraction" => {
            None
        }
        "fixtures" | "pixels" if index == 0 => Some(Type::Target),
        "sections" if index == 0 => Some(Type::Target),
        "sections" if index == 1 => Some(Type::Float),
        "count" if index == 0 => Some(Type::TargetItems),
        "pick" if index == 0 => Some(Type::TargetItems),
        "pick" if index == 1 => Some(Type::Float),
        "mark_count" | "mark_at" | "mark_prev" | "mark_prev_index" | "mark_next_index"
        | "mark_elapsed" | "mark_phase"
            if index == 0 =>
        {
            Some(Type::Marks)
        }
        "intensity" | "invert" => Some(Type::Color),
        "rgb" | "hsv" | "rand" | "srand" | "sin" | "cos" | "abs" | "floor" | "min" | "clamp"
        | "smoothstep" | "section_position" => Some(Type::Float),
        "curve_crossing" if index == 0 => Some(Type::Curve),
        "curve_clamped" if index == 0 => Some(Type::Curve),
        "gradient_color_scaled" if index == 0 => Some(Type::Gradient),
        "curve_crossing" | "curve_clamped" | "gradient_color_scaled" => Some(Type::Float),
        _ => None,
    }
}

fn numeric(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Float)
}

fn type_contains_signal(ty: &Type) -> bool {
    match ty {
        Type::Signal => true,
        Type::Array(inner) => type_contains_signal(inner),
        _ => false,
    }
}

fn type_of_value(value: &Value) -> Type {
    match value {
        Value::Void => Type::Void,
        Value::Int(_) => Type::Int,
        Value::Float(_) => Type::Float,
        Value::Bool(_) => Type::Bool,
        Value::Color(_) => Type::Color,
        Value::Marks(_) => Type::Marks,
        Value::Target(_) => Type::Target,
        Value::TargetItems(_) => Type::TargetItems,
        Value::TargetItem(_) => Type::TargetItem,
        Value::Curve(_) => Type::Curve,
        Value::Gradient(_) => Type::Gradient,
        Value::Array(items) => items
            .first()
            .map(|item| Type::array(type_of_value(item)))
            .unwrap_or_else(|| Type::array(Type::Void)),
        Value::Enum(identifier) => Type::Enum(vec![identifier.clone()]),
    }
}

fn value_matches_type(value: &Value, ty: &Type) -> bool {
    match (value, ty) {
        (Value::Void, Type::Void)
        | (Value::Int(_), Type::Int)
        | (Value::Float(_), Type::Float)
        | (Value::Bool(_), Type::Bool)
        | (Value::Color(_), Type::Color)
        | (Value::Marks(_), Type::Marks)
        | (Value::Target(_), Type::Target)
        | (Value::TargetItems(_), Type::TargetItems)
        | (Value::TargetItem(_), Type::TargetItem) => true,
        (Value::Array(items), Type::Array(item_type)) => {
            items.iter().all(|item| value_matches_type(item, item_type))
        }
        (Value::Enum(identifier), Type::Enum(options)) => {
            options.iter().any(|option| option == identifier)
        }
        (Value::Curve(_), Type::Curve) | (Value::Gradient(_), Type::Gradient) => true,
        _ => false,
    }
}

fn static_identifier(value: &str) -> Identifier {
    match Identifier::new(value.to_string()) {
        Ok(identifier) => identifier,
        Err(_) => unreachable!("static identifier is valid"),
    }
}

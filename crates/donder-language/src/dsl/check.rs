//! Name resolution and type checking. A declaration is lowered straight into
//! its definition IR; diagnostics point at the syntax that produced a node.
use super::declarations::{OperatorInputDecl, ParamDecl, ParamRange};
use super::ir::interval::{Bounds, interval};
use super::ir::{Binary, Context, Graph, LoopSet, Node, Op, Param, Reducer, Ternary, Unary};
use super::syntax::ast::*;
use super::syntax::lexer::TextSpan;
use super::types::{Identifier, Type, Value};
use super::{Diagnostic, MAX_DSL_LOOP_ITERATIONS};
use crate::dsl::bytecode::SignalPixel;
use crate::values::Color;

/// One checked effect or operator.
#[derive(Clone, Debug)]
pub(crate) struct Definition {
    pub(crate) name: Identifier,
    pub(crate) params: Vec<ParamDecl>,
    pub(crate) inputs: Vec<OperatorInputDecl>,
    pub(crate) graph: Graph,
    /// The current pixel's color.
    pub(crate) root: Node,
    /// Iteration counts whose bound depends on the lengths of parameters.
    pub(crate) length_bounds: Vec<Node>,
    /// The declaration's name in its source.
    pub(crate) span: TextSpan,
    pub(crate) fingerprint: u64,
}

pub(crate) fn check(module: Module) -> Result<Vec<Definition>, Vec<Diagnostic>> {
    let mut definitions = Vec::new();
    let mut diagnostics = Vec::new();
    for declaration in module.declarations {
        match Checker::declaration(declaration) {
            Ok(definition) => definitions.push(definition),
            Err(errors) => diagnostics.extend(errors),
        }
    }
    if diagnostics.is_empty() {
        Ok(definitions)
    } else {
        Err(diagnostics)
    }
}

/// Names with a fixed meaning in every definition.
const RESERVED: &[&str] = &[
    "time", "duration", "progress", "pixel", "target", "PI", "TAU",
];

/// A block's result where guards may produce nothing: whether it holds a value,
/// and the value, which is meaningless when it does not. A guard's skipped
/// branch has no value at all, so a consumer that tests `valid` (a reduction
/// filter) reads the guarded value without a redundant choice.
#[derive(Clone, Copy)]
struct Outcome {
    valid: Node,
    value: Option<Node>,
}

/// Whether guards without `else` may skip a block.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Value,
    Tail,
}

/// A checked operand, or an unbound name that may be an enum option.
enum Operand {
    Node(Node),
    Option(Identifier, TextSpan),
}

struct Checker {
    graph: Graph,
    params: Vec<ParamDecl>,
    inputs: Vec<Identifier>,
    scopes: Vec<(Identifier, Node)>,
    length_bounds: Vec<Node>,
    diagnostics: Vec<Diagnostic>,
}

type Checked<T> = Option<T>;

impl Checker {
    fn declaration(declaration: Declaration) -> Result<Definition, Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        let mut params = Vec::new();
        let mut names: Vec<&Identifier> = Vec::new();
        let name_check = |name: &Name, diagnostics: &mut Vec<Diagnostic>| {
            if RESERVED.contains(&name.name.as_str()) {
                diagnostics.push(Diagnostic::new(
                    name.span,
                    format!("`{}` is a reserved name", name.name.as_str()),
                ));
            }
        };
        for input in &declaration.inputs {
            name_check(input, &mut diagnostics);
        }
        for param in &declaration.params {
            name_check(&param.name, &mut diagnostics);
            match check_param(param) {
                Ok(param) => params.push(param),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
        }
        for name in declaration
            .inputs
            .iter()
            .chain(declaration.params.iter().map(|param| &param.name))
        {
            if names.contains(&&name.name) {
                diagnostics.push(Diagnostic::new(
                    name.span,
                    format!("`{}` is declared twice", name.name.as_str()),
                ));
            }
            names.push(&name.name);
        }
        match declaration.kind {
            DeclarationKind::Effect if !declaration.inputs.is_empty() => {
                diagnostics.push(Diagnostic::new(
                    declaration.inputs[0].span,
                    "effects have no signal inputs; declare them on an operator",
                ));
            }
            DeclarationKind::Operator if declaration.inputs.is_empty() => {
                diagnostics.push(Diagnostic::new(
                    declaration.name.span,
                    "an operator declares at least one `input`",
                ));
            }
            _ => {}
        }
        let Some(sample) = &declaration.sample else {
            diagnostics.push(Diagnostic::new(
                declaration.name.span,
                "a declaration needs a `sample` block",
            ));
            return Err(diagnostics);
        };
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        let graph = Graph::new(
            params
                .iter()
                .map(|param| Param {
                    ty: param.ty.clone(),
                    domain: super::ir::Domain::PARAM,
                })
                .collect(),
            declaration.inputs.len() as u32,
        );
        let mut checker = Self {
            graph,
            params,
            inputs: declaration
                .inputs
                .iter()
                .map(|input| input.name.clone())
                .collect(),
            scopes: Vec::new(),
            length_bounds: Vec::new(),
            diagnostics,
        };
        let root = checker.tail_block(sample, Mode::Tail).and_then(|outcome| {
            let value = outcome.value?;
            checker.require(value, &Type::Color, sample.result.span)?;
            // A sample that produces nothing is black.
            let black = checker.graph.color(Color::BLACK);
            Some(checker.graph.select(outcome.valid, value, black))
        });
        match root {
            Some(root) if checker.diagnostics.is_empty() => Ok(Definition {
                span: declaration.name.span,
                fingerprint: checker.graph.fingerprint(root),
                name: declaration.name.name,
                params: checker.params,
                inputs: declaration
                    .inputs
                    .into_iter()
                    .map(|input| OperatorInputDecl { name: input.name })
                    .collect(),
                graph: checker.graph,
                root,
                length_bounds: checker.length_bounds,
            }),
            _ => Err(checker.diagnostics),
        }
    }

    fn error(&mut self, span: TextSpan, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::new(span, message));
    }

    fn ty(&self, node: Node) -> &Type {
        self.graph.ty(node)
    }

    fn require(&mut self, node: Node, expected: &Type, span: TextSpan) -> Checked<Node> {
        match (expected, self.ty(node)) {
            (expected, actual) if expected == actual => Some(node),
            (Type::Float, Type::Int) => Some(self.graph.unary(Unary::IntToFloat, node)),
            (Type::Enum(options), Type::Enum(values))
                if values.iter().all(|value| options.contains(value)) =>
            {
                Some(node)
            }
            (expected, actual) => {
                let message = format!(
                    "expected {}, found {}",
                    describe(expected),
                    describe(actual)
                );
                self.error(span, message);
                None
            }
        }
    }

    fn float(&mut self, expr: &Expr) -> Checked<Node> {
        let node = self.value(expr)?;
        self.require(node, &Type::Float, expr.span)
    }

    fn int(&mut self, expr: &Expr) -> Checked<Node> {
        let node = self.value(expr)?;
        self.require(node, &Type::Int, expr.span)
    }

    fn boolean(&mut self, expr: &Expr) -> Checked<Node> {
        let node = self.value(expr)?;
        self.require(node, &Type::Bool, expr.span)
    }

    fn typed(&mut self, expr: &Expr, expected: &Type) -> Checked<Node> {
        let node = self.value(expr)?;
        self.require(node, expected, expr.span)
    }

    /// A checked block. In `Tail` mode, guards without `else` may skip it.
    fn tail_block(&mut self, block: &Block, mode: Mode) -> Checked<Outcome> {
        let depth = self.scopes.len();
        let mut guards = Vec::new();
        let mut failed = false;
        for statement in &block.statements {
            match statement {
                Statement::Let { name, ty, value } => {
                    if RESERVED.contains(&name.name.as_str()) {
                        self.error(
                            name.span,
                            format!("`{}` is a reserved name", name.name.as_str()),
                        );
                        failed = true;
                        continue;
                    }
                    let node = match ty {
                        Some(ty) => match resolve_type(ty) {
                            Ok(ty) => self.typed(value, &ty),
                            Err(diagnostic) => {
                                self.diagnostics.push(diagnostic);
                                None
                            }
                        },
                        None => self.value(value),
                    };
                    match node {
                        Some(node) => self.scopes.push((name.name.clone(), node)),
                        None => failed = true,
                    }
                }
                Statement::Guard {
                    condition,
                    otherwise,
                    span,
                } => {
                    let condition = self.boolean(condition);
                    let otherwise = match otherwise {
                        Some(otherwise) => self.tail(otherwise, mode).map(Some),
                        None if mode == Mode::Value => {
                            self.error(
                                *span,
                                "a guard without `else` can only skip a sample block or a reduction body",
                            );
                            None
                        }
                        None => Some(None),
                    };
                    match (condition, otherwise) {
                        (Some(condition), Some(otherwise)) => {
                            guards.push((condition, otherwise, *span))
                        }
                        _ => failed = true,
                    }
                }
            }
        }
        let result = if failed {
            None
        } else {
            self.tail(&block.result, mode)
        };
        self.scopes.truncate(depth);
        let mut result = result?;
        for (condition, otherwise, span) in guards.into_iter().rev() {
            let otherwise = match otherwise {
                Some(otherwise) => otherwise,
                None => Outcome {
                    valid: self.graph.bool(false),
                    value: None,
                },
            };
            result = self.choose(condition, result, otherwise, span)?;
        }
        Some(result)
    }

    fn choose(
        &mut self,
        condition: Node,
        yes: Outcome,
        no: Outcome,
        span: TextSpan,
    ) -> Checked<Outcome> {
        let value = match (yes.value, no.value) {
            (Some(yes), Some(no)) => {
                let (yes, no) = self.unify(yes, no, span)?;
                Some(self.graph.select(condition, yes, no))
            }
            (value, None) | (None, value) => value,
        };
        Some(Outcome {
            valid: self.graph.select(condition, yes.valid, no.valid),
            value,
        })
    }

    /// An expression in a position whose block may be skipped.
    fn tail(&mut self, expr: &Expr, mode: Mode) -> Checked<Outcome> {
        match &expr.kind {
            ExprKind::Block(block) => self.tail_block(block, mode),
            ExprKind::If(condition, yes, no) => {
                let condition = self.boolean(condition);
                let yes = self.tail_block(yes, mode);
                let no = self.tail(no, mode);
                let (condition, yes, no) = (condition?, yes?, no?);
                self.choose(condition, yes, no, expr.span)
            }
            _ => {
                let value = self.value(expr)?;
                let valid = self.graph.bool(true);
                Some(Outcome {
                    valid,
                    value: Some(value),
                })
            }
        }
    }

    /// Two values of one type: equal types, or an int widened to float.
    fn unify(&mut self, a: Node, b: Node, span: TextSpan) -> Checked<(Node, Node)> {
        match (self.ty(a).clone(), self.ty(b).clone()) {
            (left, right) if left == right => Some((a, b)),
            (Type::Int, Type::Float) => Some((self.graph.unary(Unary::IntToFloat, a), b)),
            (Type::Float, Type::Int) => Some((a, self.graph.unary(Unary::IntToFloat, b))),
            (left, right) => {
                self.error(
                    span,
                    format!(
                        "the branches produce {} and {}",
                        describe(&left),
                        describe(&right)
                    ),
                );
                None
            }
        }
    }

    fn value(&mut self, expr: &Expr) -> Checked<Node> {
        match self.operand(expr)? {
            Operand::Node(node) => Some(node),
            Operand::Option(name, span) => {
                self.error(span, format!("unknown name `{}`", name.as_str()));
                None
            }
        }
    }

    fn operand(&mut self, expr: &Expr) -> Checked<Operand> {
        let node = match &expr.kind {
            ExprKind::Literal(literal) => self.literal(literal, expr.span)?,
            ExprKind::Name(name) => return self.name(name, expr.span),
            ExprKind::Field(target, field) => self.field(target, field)?,
            ExprKind::Call(name, args) => self.call(name, args, expr.span)?,
            ExprKind::Method(target, method, args) => {
                self.method(target, method, args, expr.span)?
            }
            ExprKind::Index(target, index) => self.index(target, index)?,
            ExprKind::Unary(op, operand) => {
                let value = self.value(operand)?;
                match (op, self.ty(value)) {
                    (UnaryOp::Negate, Type::Int) => self.graph.unary(Unary::IntNegate, value),
                    (UnaryOp::Negate, Type::Float) => self.graph.unary(Unary::Negate, value),
                    (UnaryOp::Not, Type::Bool) => self.graph.unary(Unary::Not, value),
                    (UnaryOp::Negate, ty) => {
                        let message = format!("`-` needs a number, found {}", describe(ty));
                        self.error(expr.span, message);
                        return None;
                    }
                    (UnaryOp::Not, ty) => {
                        let message = format!("`!` needs a bool, found {}", describe(ty));
                        self.error(expr.span, message);
                        return None;
                    }
                }
            }
            ExprKind::Binary(op, left, right) => self.binary(*op, left, right, expr.span)?,
            ExprKind::Array(items) => self.array(items, expr.span)?,
            ExprKind::If(..) | ExprKind::Block(_) => {
                let outcome = self.tail(expr, Mode::Value)?;
                outcome.value?
            }
            ExprKind::Reduce(reduction) => self.reduction(reduction, expr.span)?,
        };
        Some(Operand::Node(node))
    }

    fn literal(&mut self, literal: &LiteralKind, span: TextSpan) -> Checked<Node> {
        Some(match literal {
            LiteralKind::Int(value) => match i32::try_from(*value) {
                Ok(value) => self.graph.int(value),
                Err(_) => {
                    self.error(span, "integer literal is out of range");
                    return None;
                }
            },
            LiteralKind::Float(value) => self.graph.float(*value),
            LiteralKind::Bool(value) => self.graph.bool(*value),
            LiteralKind::Color(value) => self.graph.color(*value),
            LiteralKind::Name(name) => {
                return self.value(&Expr {
                    kind: ExprKind::Name(name.clone()),
                    span,
                });
            }
        })
    }

    fn name(&mut self, name: &Identifier, span: TextSpan) -> Checked<Operand> {
        if let Some((_, node)) = self.scopes.iter().rev().find(|(bound, _)| bound == name) {
            return Some(Operand::Node(*node));
        }
        if let Some(index) = self.params.iter().position(|param| &param.name == name) {
            return Some(Operand::Node(self.graph.add(Op::Param(index as u32))));
        }
        if let Some(input) = self.inputs.iter().position(|input| input == name) {
            // A signal used as a color samples the current pixel now.
            let time = self.graph.add(Op::Context(Context::Time));
            return Some(Operand::Node(self.graph.add(Op::Sample {
                input: input as u32,
                time,
                pixel: SignalPixel::Current,
            })));
        }
        let node = match name.as_str() {
            "time" => self.graph.add(Op::Context(Context::Time)),
            "duration" => self.graph.add(Op::Context(Context::Duration)),
            "progress" => self.graph.add(Op::Context(Context::Progress)),
            "PI" => self.graph.float(core::f32::consts::PI),
            "TAU" => self.graph.float(core::f32::consts::TAU),
            "pixel" | "target" => {
                self.error(
                    span,
                    format!("use a field of `{}`, like `pixel.index`", name.as_str()),
                );
                return None;
            }
            _ => return Some(Operand::Option(name.clone(), span)),
        };
        Some(Operand::Node(node))
    }

    fn field(&mut self, target: &Expr, field: &Name) -> Checked<Node> {
        let ExprKind::Name(scope) = &target.kind else {
            self.error(field.span, "only `pixel` and `target` have fields");
            return None;
        };
        let context = match (scope.as_str(), field.name.as_str()) {
            ("pixel", "index") => Context::PixelIndex,
            ("pixel", "fraction") => Context::PixelFraction,
            ("pixel", "x") => Context::PixelX,
            ("pixel", "y") => Context::PixelY,
            ("target", "count") => Context::TargetCount,
            ("target", "min_x") => Context::TargetMinX,
            ("target", "min_y") => Context::TargetMinY,
            ("target", "max_x") => Context::TargetMaxX,
            ("target", "max_y") => Context::TargetMaxY,
            ("pixel" | "target", _) => {
                let message = format!(
                    "`{}` has no field `{}`",
                    scope.as_str(),
                    field.name.as_str()
                );
                self.error(field.span, message);
                return None;
            }
            _ => {
                self.error(field.span, "only `pixel` and `target` have fields");
                return None;
            }
        };
        Some(self.graph.add(Op::Context(context)))
    }

    fn method(
        &mut self,
        target: &Expr,
        method: &Name,
        args: &[Expr],
        span: TextSpan,
    ) -> Checked<Node> {
        let input = match &target.kind {
            ExprKind::Name(name) => self.inputs.iter().position(|input| input == name),
            _ => None,
        };
        let Some(input) = input else {
            self.error(method.span, "only operator inputs have methods");
            return None;
        };
        let global = match method.name.as_str() {
            "at" if (1..=2).contains(&args.len()) => false,
            "at_global" if args.len() == 2 => true,
            "at" => {
                self.error(span, "`at` takes a time and optionally a local pixel index");
                return None;
            }
            "at_global" => {
                self.error(span, "`at_global` takes a time and a layout pixel index");
                return None;
            }
            _ => {
                let message = format!(
                    "signals have no method `{}`; use `at` or `at_global`",
                    method.name.as_str()
                );
                self.error(method.span, message);
                return None;
            }
        };
        let time = self.float(&args[0])?;
        let pixel = match args.get(1) {
            None => SignalPixel::Current,
            Some(index) => {
                let index = self.int(index)?;
                if global {
                    SignalPixel::Global(index)
                } else {
                    SignalPixel::Local(index)
                }
            }
        };
        Some(self.graph.add(Op::Sample {
            input: input as u32,
            time,
            pixel,
        }))
    }

    /// An array literal of one item type; ints widen when any item is a float.
    fn array(&mut self, items: &[Expr], span: TextSpan) -> Checked<Node> {
        let nodes = items
            .iter()
            .map(|item| self.value(item))
            .collect::<Vec<_>>();
        let mut nodes = nodes.into_iter().collect::<Option<Vec<_>>>()?;
        let float = nodes.iter().any(|&node| *self.ty(node) == Type::Float);
        let ty = if float {
            Type::Float
        } else {
            self.ty(nodes[0]).clone()
        };
        if !matches!(ty, Type::Int | Type::Float | Type::Bool | Type::Color) {
            self.error(span, "array literals hold numbers, bools or colors");
            return None;
        }
        for (node, item) in nodes.iter_mut().zip(items) {
            *node = self.require(*node, &ty, item.span)?;
        }
        Some(self.graph.add(Op::Items(nodes.into())))
    }

    fn index(&mut self, target: &Expr, index: &Expr) -> Checked<Node> {
        let collection = self.value(target)?;
        match self.ty(collection).clone() {
            Type::Array(_) => {
                let index = self.int(index)?;
                Some(self.graph.binary(Binary::Index, collection, index))
            }
            Type::Curve => {
                let position = self.float(index)?;
                Some(self.graph.binary(Binary::CurveSample, collection, position))
            }
            Type::Gradient => {
                let position = self.float(index)?;
                Some(
                    self.graph
                        .binary(Binary::GradientSample, collection, position),
                )
            }
            ty => {
                let message = format!(
                    "only arrays, curves and gradients can be indexed, not {}",
                    describe(&ty)
                );
                self.error(target.span, message);
                None
            }
        }
    }

    fn binary(&mut self, op: BinaryOp, left: &Expr, right: &Expr, span: TextSpan) -> Checked<Node> {
        if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) {
            return self.equality(op == BinaryOp::Equal, left, right, span);
        }
        let a = self.value(left);
        let b = self.value(right);
        let (a, b) = (a?, b?);
        let (left_ty, right_ty) = (self.ty(a).clone(), self.ty(b).clone());
        let numeric = |ty: &Type| matches!(ty, Type::Int | Type::Float);
        let ints = left_ty == Type::Int && right_ty == Type::Int;
        let graph = &mut self.graph;
        let floats = |graph: &mut Graph| {
            let a = if left_ty == Type::Int {
                graph.unary(Unary::IntToFloat, a)
            } else {
                a
            };
            let b = if right_ty == Type::Int {
                graph.unary(Unary::IntToFloat, b)
            } else {
                b
            };
            (a, b)
        };
        let node = match op {
            BinaryOp::And | BinaryOp::Or if left_ty == Type::Bool && right_ty == Type::Bool => {
                if op == BinaryOp::And {
                    graph.and(a, b)
                } else {
                    graph.or(a, b)
                }
            }
            BinaryOp::Add if left_ty == Type::Color && right_ty == Type::Color => {
                graph.binary(Binary::ColorAdd, a, b)
            }
            BinaryOp::Multiply if left_ty == Type::Color && right_ty == Type::Color => {
                graph.binary(Binary::ColorMultiply, a, b)
            }
            BinaryOp::Multiply if left_ty == Type::Color && numeric(&right_ty) => {
                let (_, scale) = floats(graph);
                graph.binary(Binary::ColorScale, a, scale)
            }
            BinaryOp::Multiply if numeric(&left_ty) && right_ty == Type::Color => {
                let (scale, _) = floats(graph);
                graph.binary(Binary::ColorScale, b, scale)
            }
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Remainder
                if ints =>
            {
                let op = match op {
                    BinaryOp::Add => Binary::IntAdd,
                    BinaryOp::Subtract => Binary::IntSubtract,
                    BinaryOp::Multiply => Binary::IntMultiply,
                    _ => Binary::IntRemainder,
                };
                graph.binary(op, a, b)
            }
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Remainder
                if numeric(&left_ty) && numeric(&right_ty) =>
            {
                let (a, b) = floats(graph);
                let op = match op {
                    BinaryOp::Add => Binary::Add,
                    BinaryOp::Subtract => Binary::Subtract,
                    BinaryOp::Multiply => Binary::Multiply,
                    BinaryOp::Divide => Binary::Divide,
                    _ => Binary::Remainder,
                };
                graph.binary(op, a, b)
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual
                if numeric(&left_ty) && numeric(&right_ty) =>
            {
                let (op, a, b) = if ints {
                    let op = match op {
                        BinaryOp::Less => Binary::IntLess,
                        BinaryOp::LessEqual => Binary::IntLessEqual,
                        BinaryOp::Greater => Binary::IntGreater,
                        _ => Binary::IntGreaterEqual,
                    };
                    (op, a, b)
                } else {
                    let (a, b) = floats(graph);
                    let op = match op {
                        BinaryOp::Less => Binary::Less,
                        BinaryOp::LessEqual => Binary::LessEqual,
                        BinaryOp::Greater => Binary::Greater,
                        _ => Binary::GreaterEqual,
                    };
                    (op, a, b)
                };
                graph.binary(op, a, b)
            }
            _ => {
                let message = format!(
                    "`{}` does not apply to {} and {}",
                    operator_text(op),
                    describe(&left_ty),
                    describe(&right_ty)
                );
                self.error(span, message);
                return None;
            }
        };
        Some(node)
    }

    fn equality(
        &mut self,
        equal: bool,
        left: &Expr,
        right: &Expr,
        span: TextSpan,
    ) -> Checked<Node> {
        let a = self.operand(left);
        let b = self.operand(right);
        let (a, b) = (a?, b?);
        let op = if equal {
            Binary::Equal
        } else {
            Binary::NotEqual
        };
        let (a, b) = match (a, b) {
            (Operand::Node(a), Operand::Node(b)) => (a, b),
            (Operand::Node(node), Operand::Option(name, option_span))
            | (Operand::Option(name, option_span), Operand::Node(node)) => {
                let option = self.option(node, name, option_span)?;
                (node, option)
            }
            (Operand::Option(name, span), Operand::Option(..)) => {
                self.error(span, format!("unknown name `{}`", name.as_str()));
                return None;
            }
        };
        let (left_ty, right_ty) = (self.ty(a).clone(), self.ty(b).clone());
        let (a, b) = match (&left_ty, &right_ty) {
            (Type::Int, Type::Float) | (Type::Float, Type::Int) => self.unify(a, b, span)?,
            (Type::Int | Type::Float | Type::Bool | Type::Color, _) if left_ty == right_ty => {
                (a, b)
            }
            (Type::Enum(_), Type::Enum(_)) => (a, b),
            _ => {
                let message = format!(
                    "{} and {} cannot be compared",
                    describe(&left_ty),
                    describe(&right_ty)
                );
                self.error(span, message);
                return None;
            }
        };
        Some(self.graph.binary(op, a, b))
    }

    /// An enum option named in a comparison with a value of that enum.
    fn option(&mut self, node: Node, name: Identifier, span: TextSpan) -> Checked<Node> {
        match self.ty(node).clone() {
            Type::Enum(options) if options.contains(&name) => Some(
                self.graph
                    .typed_constant(Value::Enum(name), Type::Enum(options)),
            ),
            Type::Enum(_) => {
                self.error(
                    span,
                    format!("`{}` is not an option of this enum", name.as_str()),
                );
                None
            }
            _ => {
                self.error(span, format!("unknown name `{}`", name.as_str()));
                None
            }
        }
    }

    fn reduction(&mut self, reduction: &Reduction, span: TextSpan) -> Checked<Node> {
        let Reduction {
            reducer,
            index,
            start,
            end,
            inclusive,
            body,
            otherwise,
        } = reduction;
        let (reducer, otherwise) = (*reducer, otherwise.as_ref());
        let start = self.int(start);
        let end = self.int(end);
        let (start, end) = (start?, end?);
        let end = if *inclusive {
            let one = self.graph.int(1);
            self.graph.binary(Binary::IntAdd, end, one)
        } else {
            end
        };
        let count = self.graph.binary(Binary::IntSubtract, end, start);
        self.bound(count, span, "reduction")?;
        let Ok((id, index_node)) = self.graph.begin_loop(start, end) else {
            self.error(
                span,
                format!("a definition has at most {} reductions", LoopSet::LIMIT),
            );
            return None;
        };
        self.scopes.push((index.name.clone(), index_node));
        let outcome = self.tail_block(body, Mode::Tail);
        self.scopes.pop();
        let outcome = outcome?;
        let value = outcome.value?;
        let ty = self.ty(value).clone();
        let filter = (self.graph.constant_value(outcome.valid) != Some(&Value::Bool(true)))
            .then_some(outcome.valid);
        let reducer_name = format!("{reducer:?}").to_lowercase();
        let (reducer, body, filter, default) = match reducer {
            ReducerKind::Max | ReducerKind::Min | ReducerKind::Sum => {
                let colors = reducer != ReducerKind::Min && ty == Type::Color;
                if !(matches!(ty, Type::Int | Type::Float) || colors) {
                    let message = format!("`{reducer_name}` cannot combine {}", describe(&ty));
                    self.error(body.result.span, message);
                    return None;
                }
                let reducer = match reducer {
                    ReducerKind::Max => Reducer::Max,
                    ReducerKind::Min => Reducer::Min,
                    _ => Reducer::Sum,
                };
                (reducer, value, filter, None)
            }
            ReducerKind::Any | ReducerKind::All => {
                let value = self.require(value, &Type::Bool, body.result.span)?;
                let (reducer, body) = if reducer == ReducerKind::Any {
                    (Reducer::Any, self.graph.and(outcome.valid, value))
                } else {
                    let skipped = self.graph.unary(Unary::Not, outcome.valid);
                    (Reducer::All, self.graph.or(skipped, value))
                };
                (reducer, body, None, None)
            }
            ReducerKind::First | ReducerKind::Last => {
                if matches!(ty, Type::Array(_)) {
                    self.error(
                        body.result.span,
                        format!("`{reducer_name}` cannot produce an array"),
                    );
                    return None;
                }
                let default = match otherwise {
                    Some(block) => {
                        let value = self.tail_block(block, Mode::Value)?.value?;
                        self.require(value, &ty, block.result.span)?
                    }
                    None if ty == Type::Color => self.graph.color(Color::BLACK),
                    None => {
                        let message = format!(
                            "`{reducer_name}` of {} needs an `else` value",
                            describe(&ty)
                        );
                        self.error(span, message);
                        return None;
                    }
                };
                let reducer = if reducer == ReducerKind::First {
                    Reducer::First
                } else {
                    Reducer::Last
                };
                (reducer, value, filter, Some(default))
            }
        };
        Some(self.graph.finish_loop(id, reducer, body, filter, default))
    }

    /// Prove that `count` stays within the iteration limit, or defer the proof
    /// to binding when it depends on parameter lengths.
    fn bound(&mut self, count: Node, span: TextSpan, what: &str) -> Checked<()> {
        let ranges = param_ranges(&self.params);
        let lengths = vec![None; self.params.len()];
        let range = interval(
            &self.graph,
            count,
            &Bounds {
                ranges: &ranges,
                lengths: &lengths,
            },
        );
        if range.max <= MAX_DSL_LOOP_ITERATIONS as f64 && !range.max.is_nan() {
            return Some(());
        }
        if depends_on_length(&self.graph, count) {
            self.length_bounds.push(count);
            return Some(());
        }
        self.error(
            span,
            format!(
                "cannot prove this {what} runs at most {MAX_DSL_LOOP_ITERATIONS} times; bound it with literals, parameter ranges or `len()`"
            ),
        );
        None
    }

    fn call(&mut self, name: &Name, args: &[Expr], span: TextSpan) -> Checked<Node> {
        let function = name.name.as_str();
        let arity = match function {
            "sin" | "cos" | "abs" | "floor" | "ceil" | "trunc" | "round_even" | "sqrt"
            | "is_nan" | "int" | "rand" | "hue" | "saturation" | "intensity" | "invert" | "len"
            | "mark_count" | "section_count" | "section_index" | "section_position" => 1,
            "min"
            | "max"
            | "value_or"
            | "atan2"
            | "pow"
            | "curve_first_crossing"
            | "mark_last"
            | "mark_last_index"
            | "mark_at" => 2,
            "clamp"
            | "smoothstep"
            | "mix"
            | "rgb"
            | "hsv"
            | "gradient_color_scaled"
            | "curve_last_crossing" => 3,
            "curve_clamped" => 4,
            _ => {
                self.error(name.span, format!("unknown function `{function}`"));
                return None;
            }
        };
        if args.len() != arity {
            let message = format!(
                "`{function}` takes {arity} argument{}, found {}",
                if arity == 1 { "" } else { "s" },
                args.len()
            );
            self.error(span, message);
            return None;
        }
        let graph_unary = |checker: &mut Self, op| {
            let value = checker.float(&args[0])?;
            Some(checker.graph.unary(op, value))
        };
        Some(match function {
            "sin" => graph_unary(self, Unary::Sin)?,
            "cos" => graph_unary(self, Unary::Cos)?,
            "abs" => graph_unary(self, Unary::Abs)?,
            "floor" => graph_unary(self, Unary::Floor)?,
            "ceil" => graph_unary(self, Unary::Ceil)?,
            "trunc" => graph_unary(self, Unary::Trunc)?,
            "round_even" => graph_unary(self, Unary::RoundEven)?,
            "sqrt" => graph_unary(self, Unary::Sqrt)?,
            "rand" => graph_unary(self, Unary::Rand)?,
            "int" => graph_unary(self, Unary::FloatToInt)?,
            "is_nan" => {
                let value = self.float(&args[0])?;
                self.graph.binary(Binary::NotEqual, value, value)
            }
            "hue" | "saturation" | "intensity" | "invert" => {
                let color = self.typed(&args[0], &Type::Color)?;
                let op = match function {
                    "hue" => Unary::Hue,
                    "saturation" => Unary::Saturation,
                    "intensity" => Unary::Intensity,
                    _ => Unary::Invert,
                };
                self.graph.unary(op, color)
            }
            "min" | "max" => {
                let a = self.value(&args[0])?;
                if *self.ty(a) == Type::Color && function == "max" {
                    let b = self.typed(&args[1], &Type::Color)?;
                    self.graph.binary(Binary::ColorMax, a, b)
                } else {
                    let a = self.require(a, &Type::Float, args[0].span)?;
                    let b = self.float(&args[1])?;
                    let op = if function == "min" {
                        Binary::Min
                    } else {
                        Binary::Max
                    };
                    self.graph.binary(op, a, b)
                }
            }
            "value_or" | "atan2" => {
                let a = self.float(&args[0])?;
                let b = self.float(&args[1])?;
                let op = if function == "value_or" {
                    Binary::ValueOr
                } else {
                    Binary::Atan2
                };
                self.graph.binary(op, a, b)
            }
            "pow" => {
                let base = self.float(&args[0])?;
                let exponent = self.int(&args[1])?;
                self.bound(exponent, args[1].span, "power")?;
                self.graph.binary(Binary::Power, base, exponent)
            }
            "clamp" | "rgb" | "hsv" => {
                let a = self.float(&args[0])?;
                let b = self.float(&args[1])?;
                let c = self.float(&args[2])?;
                let op = match function {
                    "clamp" => Ternary::Clamp,
                    "rgb" => Ternary::Rgb,
                    _ => Ternary::Hsv,
                };
                self.graph.ternary(op, a, b, c)
            }
            "smoothstep" => {
                let low = self.float(&args[0])?;
                let high = self.float(&args[1])?;
                let value = self.float(&args[2])?;
                let width = self.graph.binary(Binary::Subtract, high, low);
                let offset = self.graph.binary(Binary::Subtract, value, low);
                let position = self.graph.binary(Binary::Divide, offset, width);
                self.graph.unary(Unary::Smoothstep, position)
            }
            "mix" => {
                let a = self.value(&args[0])?;
                if *self.ty(a) == Type::Color {
                    let b = self.typed(&args[1], &Type::Color)?;
                    let amount = self.float(&args[2])?;
                    self.graph.ternary(Ternary::MixColor, a, b, amount)
                } else {
                    let a = self.require(a, &Type::Float, args[0].span)?;
                    let b = self.float(&args[1])?;
                    let amount = self.float(&args[2])?;
                    self.graph.ternary(Ternary::Mix, a, b, amount)
                }
            }
            "curve_clamped" => {
                let curve = self.typed(&args[0], &Type::Curve)?;
                let position = self.float(&args[1])?;
                let min = self.float(&args[2])?;
                let max = self.float(&args[3])?;
                let value = self.graph.binary(Binary::CurveSample, curve, position);
                self.graph.ternary(Ternary::Clamp, value, min, max)
            }
            "gradient_color_scaled" => {
                let gradient = self.typed(&args[0], &Type::Gradient)?;
                let position = self.float(&args[1])?;
                let scale = self.float(&args[2])?;
                let (zero, one) = (self.graph.float(0.0), self.graph.float(1.0));
                let scale = self.graph.ternary(Ternary::Clamp, scale, zero, one);
                let color = self
                    .graph
                    .binary(Binary::GradientSample, gradient, position);
                self.graph.binary(Binary::ColorScale, color, scale)
            }
            "curve_first_crossing" => {
                let curve = self.typed(&args[0], &Type::Curve)?;
                let value = self.float(&args[1])?;
                self.graph.binary(Binary::CurveFirstCrossing, curve, value)
            }
            "curve_last_crossing" => {
                let curve = self.typed(&args[0], &Type::Curve)?;
                let value = self.float(&args[1])?;
                let before = self.float(&args[2])?;
                self.graph
                    .ternary(Ternary::CurveLastCrossing, curve, value, before)
            }
            "mark_count" => {
                let marks = self.typed(&args[0], &Type::Marks)?;
                self.graph.unary(Unary::MarkCount, marks)
            }
            "mark_at" => {
                let marks = self.typed(&args[0], &Type::Marks)?;
                let index = self.int(&args[1])?;
                self.graph.binary(Binary::MarkAt, marks, index)
            }
            "mark_last" | "mark_last_index" => {
                let marks = self.typed(&args[0], &Type::Marks)?;
                let time = self.float(&args[1])?;
                let op = if function == "mark_last" {
                    Binary::MarkLast
                } else {
                    Binary::MarkLastIndex
                };
                self.graph.binary(op, marks, time)
            }
            "len" => {
                let value = self.value(&args[0])?;
                match self.ty(value) {
                    Type::Array(_) => self.graph.unary(Unary::Len, value),
                    Type::Marks => self.graph.unary(Unary::MarkCount, value),
                    ty => {
                        let message =
                            format!("`len` needs an array or marks, found {}", describe(ty));
                        self.error(args[0].span, message);
                        return None;
                    }
                }
            }
            "section_count" | "section_index" => {
                let width = self.int(&args[0])?;
                let op = if function == "section_count" {
                    Unary::SectionCount
                } else {
                    Unary::SectionIndex
                };
                self.graph.unary(op, width)
            }
            "section_position" => {
                let width = self.float(&args[0])?;
                let one = self.graph.float(1.0);
                let width = self.graph.binary(Binary::Max, width, one);
                let inverse = self.graph.binary(Binary::Divide, one, width);
                self.graph.binary(Binary::SectionPosition, width, inverse)
            }
            _ => unreachable!("arity table covers every builtin"),
        })
    }
}

fn param_ranges(params: &[ParamDecl]) -> Vec<Option<(f64, f64)>> {
    params
        .iter()
        .map(|param| match (&param.ty, param.range) {
            (Type::Int, Some(ParamRange::Int { min, max })) => {
                Some((f64::from(min), f64::from(max)))
            }
            (Type::Float, Some(ParamRange::Float { min, max })) => {
                Some((f64::from(min), f64::from(max)))
            }
            _ => None,
        })
        .collect()
}

/// Whether `node` reads the length of an array or marks parameter.
fn depends_on_length(graph: &Graph, node: Node) -> bool {
    let mut pending = vec![node];
    let mut seen = std::collections::HashSet::new();
    while let Some(node) = pending.pop() {
        if !seen.insert(node) {
            continue;
        }
        match graph.op(node) {
            Op::Unary(Unary::Len | Unary::MarkCount, operand)
            | Op::Binary(Binary::MarkLastIndex, operand, _)
                if matches!(graph.op(*operand), Op::Param(_)) =>
            {
                return true;
            }
            Op::LoopIndex(id) => {
                let data = graph.loop_(*id);
                pending.extend([data.start, data.end]);
            }
            op => pending.extend(op.operands()),
        }
    }
    false
}

/// Check one parameter declaration: its type, range and default.
fn check_param(param: &super::syntax::ast::Param) -> Result<ParamDecl, Diagnostic> {
    let ty = resolve_type(&param.ty)?;
    let span = param.name.span;
    let name = param.name.name.as_str();
    let ranged = matches!(ty, Type::Int | Type::Float | Type::Curve);
    let range = match (&param.range, ranged) {
        (None, true) => {
            return Err(Diagnostic::new(
                span,
                format!("`{name}` must declare its range, like `in 0.0..1.0`"),
            ));
        }
        (Some((min, _)), false) => {
            return Err(Diagnostic::new(
                min.span,
                "only int, float and curve parameters take a range",
            ));
        }
        (None, false) => None,
        (Some((min, max)), true) => {
            let range = match (&ty, &min.kind, &max.kind) {
                (Type::Int, LiteralKind::Int(min), LiteralKind::Int(max)) => {
                    match (i32::try_from(*min), i32::try_from(*max)) {
                        (Ok(min), Ok(max)) => Some(ParamRange::Int { min, max }),
                        _ => None,
                    }
                }
                (Type::Float | Type::Curve, min, max) => number(min)
                    .zip(number(max))
                    .map(|(min, max)| ParamRange::Float { min, max }),
                _ => None,
            };
            match range {
                Some(range) if range.fits(&ty) => Some(range),
                _ => {
                    return Err(Diagnostic::new(
                        min.span.to(max.span),
                        "a range is two finite literals of the parameter's type, minimum first",
                    ));
                }
            }
        }
    };
    let default = match &param.default {
        None => None,
        Some(literal) => {
            let value = match (&ty, &literal.kind) {
                (Type::Int, LiteralKind::Int(value)) => i32::try_from(*value).ok().map(Value::Int),
                (Type::Float, kind) => number(kind).map(Value::Float),
                (Type::Bool, LiteralKind::Bool(value)) => Some(Value::Bool(*value)),
                (Type::Color, LiteralKind::Color(value)) => Some(Value::Color(*value)),
                (Type::Enum(options), LiteralKind::Name(value)) if options.contains(value) => {
                    Some(Value::Enum(value.clone()))
                }
                _ => None,
            };
            let Some(value) = value else {
                let message = match ty {
                    Type::Int | Type::Float | Type::Bool | Type::Color | Type::Enum(_) => {
                        format!("the default of `{name}` must be {}", describe(&ty))
                    }
                    _ => format!(
                        "{} has no literal, so `{name}` has no default",
                        describe(&ty)
                    ),
                };
                return Err(Diagnostic::new(literal.span, message));
            };
            Some(value)
        }
    };
    let declaration = ParamDecl {
        name: param.name.name.clone(),
        ty,
        range,
        default,
    };
    if let Some(default) = &declaration.default
        && !declaration.accepts_value(default)
    {
        return Err(Diagnostic::new(
            span,
            format!("the default of `{name}` is outside its range"),
        ));
    }
    Ok(declaration)
}

fn number(kind: &LiteralKind) -> Option<f32> {
    match *kind {
        LiteralKind::Int(value) => i32::try_from(value).ok().map(|value| value as f32),
        LiteralKind::Float(value) => Some(value),
        _ => None,
    }
}

fn resolve_type(ty: &TypeExpr) -> Result<Type, Diagnostic> {
    Ok(match &ty.kind {
        TypeKind::Named(name) => match name.as_str() {
            "int" => Type::Int,
            "float" => Type::Float,
            "bool" => Type::Bool,
            "color" => Type::Color,
            "curve" => Type::Curve,
            "gradient" => Type::Gradient,
            "marks" => Type::Marks,
            other => {
                return Err(Diagnostic::new(ty.span, format!("unknown type `{other}`")));
            }
        },
        TypeKind::Enum(options) => {
            let mut names = Vec::new();
            for option in options {
                if names.contains(&option.name) {
                    return Err(Diagnostic::new(
                        option.span,
                        format!("option `{}` is listed twice", option.name.as_str()),
                    ));
                }
                names.push(option.name.clone());
            }
            Type::Enum(names)
        }
        TypeKind::Array(item) => match resolve_type(item)? {
            Type::Array(_) => {
                return Err(Diagnostic::new(
                    item.span,
                    "an array's items cannot be arrays",
                ));
            }
            item => Type::array(item),
        },
    })
}

fn describe(ty: &Type) -> String {
    match ty {
        Type::Void => "nothing".into(),
        Type::Int => "an int".into(),
        Type::Float => "a float".into(),
        Type::Bool => "a bool".into(),
        Type::Color => "a color".into(),
        Type::Signal => "a signal".into(),
        Type::Marks => "marks".into(),
        Type::Curve => "a curve".into(),
        Type::Gradient => "a gradient".into(),
        Type::Array(item) => format!(
            "an array of {}",
            describe(item)
                .trim_start_matches("a ")
                .trim_start_matches("an ")
        ),
        Type::Enum(options) => format!(
            "an enum of {}",
            options
                .iter()
                .map(|option| option.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn operator_text(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Remainder => "%",
        BinaryOp::Less => "<",
        BinaryOp::LessEqual => "<=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

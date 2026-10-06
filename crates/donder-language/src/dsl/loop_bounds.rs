//! Loop trip-count bounds. Every `range(count)` loop must have a count bounded
//! by literals, declared parameter ranges, and parameter lengths. A bound
//! known at compile time becomes the loop's cap; a bound that depends on an
//! array or marks parameter's length is checked when values are bound.
use super::MAX_DSL_LOOP_ITERATIONS;
use super::ParamDecl;
use super::ast::{BinaryOp, UnaryOp};
use super::checked::{CheckedBlock, CheckedExpr, CheckedExprKind, CheckedStmt};
use super::declarations::ParamRange;
use super::diagnostic::Diagnostic;
use super::types::{Identifier, Type, Value};
use indexmap::IndexMap;
use std::collections::HashSet;

/// A bound is finite when it is finite for this stand-in length; only an
/// unknown value or a division by a possibly-zero length makes it infinite.
const PROBE_LENGTH: f64 = 1.0;

/// A loop bound that depends on parameter lengths.
#[derive(Clone, Debug, PartialEq)]
pub struct LoopBound(Bound);

impl LoopBound {
    /// The bound for these positional values, when it exceeds the limit.
    pub(super) fn exceeded(&self, values: &[Value]) -> Option<f64> {
        let high = self
            .0
            .eval(&|index| {
                let length = match values.get(index) {
                    Some(Value::Array(values)) => values.len(),
                    Some(Value::Marks(marks)) => marks.as_slice().len(),
                    _ => return Interval::TOP,
                };
                Interval::constant(length as f64)
            })
            .high
            .floor();
        (high > MAX_DSL_LOOP_ITERATIONS as f64).then_some(high)
    }
}

/// Inclusive real interval; infinite ends are unbounded.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Interval {
    low: f64,
    high: f64,
}

impl Interval {
    const TOP: Self = Self {
        low: f64::NEG_INFINITY,
        high: f64::INFINITY,
    };

    fn new(low: f64, high: f64) -> Self {
        if low.is_nan() || high.is_nan() || low > high {
            Self::TOP
        } else {
            Self { low, high }
        }
    }

    fn constant(value: f64) -> Self {
        Self::new(value, value)
    }

    fn hull(self, other: Self) -> Self {
        Self::new(self.low.min(other.low), self.high.max(other.high))
    }

    /// Apply a nondecreasing function.
    fn map(self, f: impl Fn(f64) -> f64) -> Self {
        Self::new(f(self.low), f(self.high))
    }

    /// Corners of a two-operand product or quotient. `0 * inf` is zero:
    /// the operands are finite at runtime.
    fn corners(self, other: Self, f: impl Fn(f64, f64) -> f64) -> Self {
        let corners = [
            f(self.low, other.low),
            f(self.low, other.high),
            f(self.high, other.low),
            f(self.high, other.high),
        ]
        .map(|value| if value.is_nan() { 0.0 } else { value });
        Self::new(
            corners.into_iter().fold(f64::INFINITY, f64::min),
            corners.into_iter().fold(f64::NEG_INFINITY, f64::max),
        )
    }

    /// Covers f32 rounding before a float becomes an integer.
    fn widened(self) -> Self {
        let slack = |value: f64| value.abs() * 1e-5 + 1e-5;
        Self::new(self.low - slack(self.low), self.high + slack(self.high))
    }

    fn unary(self, op: BoundUnary) -> Self {
        match op {
            BoundUnary::Negate => Self::new(-self.high, -self.low),
            BoundUnary::Abs if self.low >= 0.0 => self,
            BoundUnary::Abs if self.high <= 0.0 => Self::new(-self.high, -self.low),
            BoundUnary::Abs => Self::new(0.0, (-self.low).max(self.high)),
            BoundUnary::Floor => self.widened().map(f64::floor),
            BoundUnary::Ceil => self.widened().map(f64::ceil),
            BoundUnary::Trunc => self.widened().map(f64::trunc),
            BoundUnary::RoundEven => self.widened().map(libm::roundeven),
            BoundUnary::Sqrt if self.low >= 0.0 => self.map(f64::sqrt),
            BoundUnary::Sqrt => Self::TOP,
            // Saturating truncation; NaN becomes zero. An unbounded maximum stays
            // unbounded rather than saturating into a misleading finite bound.
            BoundUnary::ToInt if self.high == f64::INFINITY => Self::TOP,
            BoundUnary::ToInt => self
                .widened()
                .map(|value| value.trunc().clamp(i32::MIN as f64, i32::MAX as f64))
                .hull(Self::constant(0.0)),
            // Integer arithmetic wraps; a wrapped result has no useful bound.
            BoundUnary::Wrapping if self.low >= i32::MIN as f64 && self.high <= i32::MAX as f64 => {
                self
            }
            BoundUnary::Wrapping => Self::TOP,
            BoundUnary::Index => Self::new(0.0, (self.high - 1.0).max(0.0)),
        }
    }

    fn binary(self, op: BoundBinary, other: Self) -> Self {
        match op {
            BoundBinary::Add => Self::new(self.low + other.low, self.high + other.high),
            BoundBinary::Subtract => Self::new(self.low - other.high, self.high - other.low),
            BoundBinary::Multiply => self.corners(other, |a, b| a * b),
            BoundBinary::Divide if other.low > 0.0 || other.high < 0.0 => {
                self.corners(other, |a, b| a / b)
            }
            BoundBinary::Divide => Self::TOP,
            // Floored remainder takes the divisor's sign; a zero divisor gives zero.
            BoundBinary::Remainder => Self::new(other.low.min(0.0), other.high.max(0.0)),
            BoundBinary::Min => Self::new(self.low.min(other.low), self.high.min(other.high)),
            BoundBinary::Max => Self::new(self.low.max(other.low), self.high.max(other.high)),
            BoundBinary::Hull => self.hull(other),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BoundUnary {
    Negate,
    Abs,
    Floor,
    Ceil,
    Trunc,
    RoundEven,
    Sqrt,
    ToInt,
    Wrapping,
    /// The index of a loop with this count.
    Index,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BoundBinary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Min,
    Max,
    Hull,
}

/// A value's interval, symbolic only in parameter lengths. Constant operands
/// fold, so a tree that remains depends on a length.
#[derive(Clone, Debug, PartialEq)]
enum Bound {
    Interval(Interval),
    /// Element count of the positional parameter.
    Length(usize),
    Unary(BoundUnary, Box<Bound>),
    Binary(BoundBinary, Box<Bound>, Box<Bound>),
}

impl Bound {
    const TOP: Self = Self::Interval(Interval::TOP);

    fn constant(value: f64) -> Self {
        Self::Interval(Interval::constant(value))
    }

    fn unary(op: BoundUnary, operand: Self) -> Self {
        match operand {
            Self::Interval(interval) => Self::Interval(interval.unary(op)),
            operand => Self::Unary(op, Box::new(operand)),
        }
    }

    fn binary(op: BoundBinary, left: Self, right: Self) -> Self {
        match (left, right) {
            (Self::Interval(left), Self::Interval(right)) => Self::Interval(left.binary(op, right)),
            (left, right) if op == BoundBinary::Hull && left == right => left,
            (left, right) => Self::Binary(op, Box::new(left), Box::new(right)),
        }
    }

    fn eval(&self, length: &impl Fn(usize) -> Interval) -> Interval {
        match self {
            Self::Interval(interval) => *interval,
            Self::Length(index) => length(*index),
            Self::Unary(op, operand) => operand.eval(length).unary(*op),
            Self::Binary(op, left, right) => left.eval(length).binary(*op, right.eval(length)),
        }
    }
}

/// Narrow every `range` loop's cap to its proven bound and return the bounds
/// that must wait for parameter lengths.
pub(super) fn bound_loops(
    params: &[ParamDecl],
    body: &mut CheckedBlock,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<LoopBound> {
    let scope = params
        .iter()
        .enumerate()
        .map(|(index, param)| {
            let bound = match (&param.ty, param.range) {
                (Type::Array(_) | Type::Marks, _) => Bound::Length(index),
                (_, Some(ParamRange::Int { min, max })) => {
                    Bound::Interval(Interval::new(min as f64, max as f64))
                }
                (Type::Float, Some(ParamRange::Float { min, max })) => {
                    Bound::Interval(Interval::new(min as f64, max as f64))
                }
                _ => Bound::TOP,
            };
            (param.name.clone(), bound)
        })
        .collect();
    let mut analyzer = Analyzer {
        scopes: vec![scope],
        deferred: Vec::new(),
        diagnostics,
    };
    analyzer.block(body);
    analyzer.deferred
}

/// Values follow statement order. Branches join; a loop body may run any
/// number of times, so everything it assigns is unbounded.
struct Analyzer<'a> {
    scopes: Vec<IndexMap<Identifier, Bound>>,
    deferred: Vec<LoopBound>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl Analyzer<'_> {
    fn block(&mut self, block: &mut CheckedBlock) {
        self.scopes.push(IndexMap::new());
        for statement in &mut block.statements {
            self.statement(statement);
        }
        let _ = self.scopes.pop();
    }

    fn statement(&mut self, statement: &mut CheckedStmt) {
        match statement {
            CheckedStmt::Local {
                ty,
                name,
                initializer,
            } => {
                let bound = match initializer {
                    Some(initializer) => self.value(initializer),
                    None if matches!(ty, Type::Int | Type::Float) => Bound::constant(0.0),
                    None if matches!(ty, Type::Array(_) | Type::Marks) => Bound::constant(0.0),
                    None => Bound::TOP,
                };
                self.declare(name.clone(), bound);
            }
            CheckedStmt::Assign { name, value } => {
                let bound = self.value(value);
                self.assign(name, bound);
            }
            CheckedStmt::Expr(_) | CheckedStmt::Return(_) => {}
            CheckedStmt::If {
                then_block,
                else_block,
                ..
            } => {
                let before = self.scopes.clone();
                self.block(then_block);
                let after_then = std::mem::replace(&mut self.scopes, before);
                if let Some(else_block) = else_block {
                    self.block(else_block);
                }
                for (scope, then_scope) in self.scopes.iter_mut().zip(after_then) {
                    for (bound, then_bound) in scope.values_mut().zip(then_scope.into_values()) {
                        let joined = std::mem::replace(bound, Bound::TOP);
                        *bound = Bound::binary(BoundBinary::Hull, joined, then_bound);
                    }
                }
            }
            CheckedStmt::For {
                initializer,
                update,
                body,
                ..
            } => {
                self.scopes.push(IndexMap::new());
                self.statement(initializer);
                let mut assigned = HashSet::new();
                collect_statement_assigned_names(update, &mut assigned);
                collect_assigned_names(body, &mut assigned);
                self.havoc(&assigned);
                self.block(body);
                self.statement(update);
                let _ = self.scopes.pop();
                self.havoc(&assigned);
            }
            CheckedStmt::ForMarks { index, marks, body } => {
                let count = self.value(marks);
                self.loop_body(index, count, body);
            }
            CheckedStmt::ForRange {
                index,
                count,
                cap,
                body,
            } => {
                let bound = self.value(count);
                if let Some(proven) = self.check(&bound, count) {
                    *cap = proven;
                }
                self.loop_body(index, bound, body);
            }
        }
    }

    fn loop_body(&mut self, index: &Identifier, count: Bound, body: &mut CheckedBlock) {
        let mut assigned = HashSet::new();
        collect_assigned_names(body, &mut assigned);
        self.havoc(&assigned);
        self.scopes.push(IndexMap::from([(
            index.clone(),
            Bound::unary(BoundUnary::Index, count),
        )]));
        self.block(body);
        let _ = self.scopes.pop();
        self.havoc(&assigned);
    }

    /// The compile-time cap, or `None` when the bound waits for lengths.
    fn check(&mut self, bound: &Bound, count: &CheckedExpr) -> Option<i32> {
        let probe = bound.eval(&|_| Interval::new(0.0, PROBE_LENGTH));
        if probe.high.is_infinite() {
            self.diagnostics.push(Diagnostic::new(
                count.span,
                "loop count has no upper bound; derive it from literals, parameter ranges, and parameter lengths",
            ));
            return None;
        }
        let Bound::Interval(interval) = bound else {
            self.deferred.push(LoopBound(bound.clone()));
            return None;
        };
        let high = interval.high.floor().max(0.0);
        if high > MAX_DSL_LOOP_ITERATIONS as f64 {
            self.diagnostics.push(Diagnostic::new(
                count.span,
                format!("loop can run {high} times; the limit is {MAX_DSL_LOOP_ITERATIONS}"),
            ));
            return None;
        }
        // An empty loop still has a valid, positive cap.
        Some((high as i32).max(1))
    }

    fn declare(&mut self, name: Identifier, bound: Bound) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, bound);
        }
    }

    fn assign(&mut self, name: &Identifier, bound: Bound) {
        if let Some(slot) = self
            .scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
        {
            *slot = bound;
        }
    }

    fn havoc(&mut self, names: &HashSet<Identifier>) {
        for name in names {
            self.assign(name, Bound::TOP);
        }
    }

    fn lookup(&self, name: &Identifier) -> Bound {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .cloned()
            .unwrap_or(Bound::TOP)
    }

    /// A number's interval, or an array's or marks value's length.
    fn value(&self, expr: &CheckedExpr) -> Bound {
        let wrapping = |bound| {
            if expr.ty == Type::Int {
                Bound::unary(BoundUnary::Wrapping, bound)
            } else {
                bound
            }
        };
        match &expr.kind {
            CheckedExprKind::Literal(Value::Int(value)) => Bound::constant(*value as f64),
            CheckedExprKind::Literal(Value::Float(value)) => Bound::constant(*value as f64),
            CheckedExprKind::Literal(Value::Array(values)) => Bound::constant(values.len() as f64),
            CheckedExprKind::Array(items) => Bound::constant(items.len() as f64),
            CheckedExprKind::Variable(name) => self.lookup(name),
            CheckedExprKind::Unary {
                op: UnaryOp::Negate,
                expr: operand,
            } => wrapping(Bound::unary(BoundUnary::Negate, self.value(operand))),
            CheckedExprKind::Binary { op, left, right } => {
                let op = match op {
                    BinaryOp::Add => BoundBinary::Add,
                    BinaryOp::Subtract => BoundBinary::Subtract,
                    BinaryOp::Multiply => BoundBinary::Multiply,
                    BinaryOp::Divide => BoundBinary::Divide,
                    BinaryOp::Remainder => BoundBinary::Remainder,
                    _ => return Bound::TOP,
                };
                wrapping(Bound::binary(op, self.value(left), self.value(right)))
            }
            CheckedExprKind::Call { callee, args } => {
                let CheckedExprKind::Variable(name) = &callee.kind else {
                    return Bound::TOP;
                };
                let arg = |index: usize| args.get(index).map_or(Bound::TOP, |arg| self.value(arg));
                let unary = |op| Bound::unary(op, arg(0));
                match name.as_str() {
                    "abs" => unary(BoundUnary::Abs),
                    "floor" => unary(BoundUnary::Floor),
                    "ceil" => unary(BoundUnary::Ceil),
                    "trunc" => unary(BoundUnary::Trunc),
                    "round_even" => unary(BoundUnary::RoundEven),
                    "sqrt" => unary(BoundUnary::Sqrt),
                    "int" => unary(BoundUnary::ToInt),
                    "len" | "mark_count" => arg(0),
                    "min" => Bound::binary(BoundBinary::Min, arg(0), arg(1)),
                    "max" => Bound::binary(BoundBinary::Max, arg(0), arg(1)),
                    "clamp" => Bound::binary(
                        BoundBinary::Min,
                        Bound::binary(BoundBinary::Max, arg(0), arg(1)),
                        arg(2),
                    ),
                    "rand" | "progress" | "smoothstep" => Bound::Interval(Interval::new(0.0, 1.0)),
                    "sin" | "cos" => Bound::Interval(Interval::new(-1.0, 1.0)),
                    _ => Bound::TOP,
                }
            }
            _ => Bound::TOP,
        }
    }
}

pub(super) fn collect_assigned_names(block: &CheckedBlock, assigned: &mut HashSet<Identifier>) {
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

pub(super) fn fixed_for_iterations(
    initializer: &CheckedStmt,
    condition: &CheckedExpr,
    update: &CheckedStmt,
    body: &CheckedBlock,
) -> Option<usize> {
    let CheckedStmt::Local {
        ty: Type::Int,
        name,
        initializer: Some(initial),
    } = initializer
    else {
        return None;
    };
    let mut index = constant_int(initial)?;
    let CheckedExprKind::Binary {
        op: comparison,
        left,
        right,
    } = &condition.kind
    else {
        return None;
    };
    if !is_variable(left, name) {
        return None;
    }
    let bound = constant_int(right)?;
    let CheckedStmt::Assign {
        name: updated,
        value,
    } = update
    else {
        return None;
    };
    if updated != name {
        return None;
    }
    let CheckedExprKind::Binary {
        op: operation,
        left,
        right,
    } = &value.kind
    else {
        return None;
    };
    if !is_variable(left, name) || block_assigns_name(body, name) {
        return None;
    }
    let step = constant_int(right)?;
    for count in 0..=MAX_DSL_LOOP_ITERATIONS {
        let active = match comparison {
            BinaryOp::Less => index < bound,
            BinaryOp::LessEqual => index <= bound,
            BinaryOp::Greater => index > bound,
            BinaryOp::GreaterEqual => index >= bound,
            _ => return None,
        };
        if !active {
            return Some(count);
        }
        index = match operation {
            BinaryOp::Add => index.wrapping_add(step),
            BinaryOp::Subtract => index.wrapping_sub(step),
            BinaryOp::Multiply => index.wrapping_mul(step),
            _ => return None,
        };
    }
    None
}

fn is_variable(expr: &CheckedExpr, name: &Identifier) -> bool {
    matches!(&expr.kind, CheckedExprKind::Variable(candidate) if candidate == name)
}

fn constant_int(expr: &CheckedExpr) -> Option<i32> {
    match &expr.kind {
        CheckedExprKind::Literal(Value::Int(value)) => Some(*value),
        CheckedExprKind::Unary {
            op: UnaryOp::Negate,
            expr,
        } => constant_int(expr).map(i32::wrapping_neg),
        _ => None,
    }
}

pub(super) fn block_assigns_name(block: &CheckedBlock, name: &Identifier) -> bool {
    block_assigns_name_in_scope(block, name, false)
}

fn block_assigns_name_in_scope(
    block: &CheckedBlock,
    name: &Identifier,
    mut shadowed: bool,
) -> bool {
    for statement in &block.statements {
        match statement {
            CheckedStmt::Local { name: local, .. } if local == name => shadowed = true,
            CheckedStmt::Assign { name: assigned, .. } if assigned == name && !shadowed => {
                return true;
            }
            CheckedStmt::If {
                then_block,
                else_block,
                ..
            } => {
                if block_assigns_name_in_scope(then_block, name, shadowed)
                    || else_block
                        .as_ref()
                        .is_some_and(|block| block_assigns_name_in_scope(block, name, shadowed))
                {
                    return true;
                }
            }
            CheckedStmt::For {
                initializer,
                update,
                body,
                ..
            } => {
                let loop_shadowed = shadowed
                    || matches!(initializer.as_ref(), CheckedStmt::Local { name: local, .. } if local == name);
                if (!loop_shadowed
                    && (matches!(initializer.as_ref(), CheckedStmt::Assign { name: assigned, .. } if assigned == name)
                        || matches!(update.as_ref(), CheckedStmt::Assign { name: assigned, .. } if assigned == name)))
                    || block_assigns_name_in_scope(body, name, loop_shadowed)
                {
                    return true;
                }
            }
            CheckedStmt::ForMarks { index, body, .. }
            | CheckedStmt::ForRange { index, body, .. }
                if block_assigns_name_in_scope(body, name, shadowed || index == name) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

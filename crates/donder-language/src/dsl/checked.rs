use super::EmittedReference;
use super::ast::{BinaryOp, Block, FunctionDecl, OperatorInputDecl, ParamDecl, UnaryOp};
use super::lexer::TextSpan;
use super::types::{Identifier, Type, Value};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CheckedModule {
    pub effects: Vec<CheckedEffectDecl>,
    pub operators: Vec<CheckedOperatorDecl>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CheckedOperatorDecl {
    pub name: Identifier,
    pub inputs: Vec<OperatorInputDecl>,
    pub params: Vec<ParamDecl>,
    pub entrypoint: FunctionDecl,
    pub body: CheckedBlock,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CheckedEffectDecl {
    pub name: Identifier,
    pub params: Vec<ParamDecl>,
    pub entrypoint: FunctionDecl,
    pub body: CheckedBlock,
    pub preparation_controls: super::staging::PreparationControls,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CheckedBlock {
    pub statements: Vec<CheckedStmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CheckedStmt {
    Local {
        ty: Type,
        name: Identifier,
        initializer: Option<CheckedExpr>,
    },
    Assign {
        name: Identifier,
        value: CheckedExpr,
    },
    Expr(CheckedExpr),
    If {
        condition: CheckedExpr,
        then_block: CheckedBlock,
        else_block: Option<CheckedBlock>,
    },
    For {
        initializer: Box<CheckedStmt>,
        condition: CheckedExpr,
        update: Box<CheckedStmt>,
        body: CheckedBlock,
    },
    ForMarks {
        index: Identifier,
        marks: CheckedExpr,
        body: CheckedBlock,
    },
    ForRange {
        index: Identifier,
        count: CheckedExpr,
        cap: CheckedExpr,
        body: CheckedBlock,
    },
    Emit {
        effect: EmittedReference,
        fields: Vec<(Identifier, CheckedExpr)>,
    },
    Return(CheckedExpr),
}

#[derive(Clone, Debug)]
pub(crate) struct CheckedExpr {
    pub kind: CheckedExprKind,
    pub span: TextSpan,
    pub ty: Type,
}

impl PartialEq for CheckedExpr {
    fn eq(&self, other: &Self) -> bool {
        self.ty == other.ty && self.kind == other.kind
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CheckedExprKind {
    Literal(Value),
    Variable(Identifier),
    Array(Vec<CheckedExpr>),
    Index {
        target: Box<CheckedExpr>,
        index: Box<CheckedExpr>,
    },
    Member {
        target: Box<CheckedExpr>,
        member: Identifier,
    },
    Call {
        callee: Box<CheckedExpr>,
        args: Vec<CheckedExpr>,
    },
    SignalSample {
        input: Identifier,
        seconds: Box<CheckedExpr>,
        pixel: super::bytecode::SignalPixel<Box<CheckedExpr>>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<CheckedExpr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<CheckedExpr>,
        right: Box<CheckedExpr>,
    },
}

impl From<Block> for CheckedBlock {
    fn from(block: Block) -> Self {
        Self {
            statements: block
                .statements
                .into_iter()
                .map(CheckedStmt::unchecked)
                .collect(),
        }
    }
}

impl CheckedStmt {
    /// Outer bindings assigned by this statement, excluding block and loop locals.
    pub(crate) fn assigned_names(&self) -> indexmap::IndexSet<Identifier> {
        let mut assigned = indexmap::IndexSet::new();
        collect_assignments(self, &mut std::collections::HashSet::new(), &mut assigned);
        assigned
    }

    fn unchecked(statement: super::ast::Stmt) -> Self {
        match statement {
            super::ast::Stmt::Local {
                ty,
                name,
                initializer,
            } => Self::Local {
                ty,
                name,
                initializer: initializer.map(CheckedExpr::unchecked),
            },
            super::ast::Stmt::Assign { name, value } => Self::Assign {
                name,
                value: CheckedExpr::unchecked(value),
            },
            super::ast::Stmt::Expr(expr) => Self::Expr(CheckedExpr::unchecked(expr)),
            super::ast::Stmt::If {
                condition,
                then_block,
                else_block,
            } => Self::If {
                condition: CheckedExpr::unchecked(condition),
                then_block: then_block.into(),
                else_block: else_block.map(Into::into),
            },
            super::ast::Stmt::For {
                initializer,
                condition,
                update,
                body,
            } => Self::For {
                initializer: Box::new(Self::unchecked(*initializer)),
                condition: CheckedExpr::unchecked(condition),
                update: Box::new(Self::unchecked(*update)),
                body: body.into(),
            },
            super::ast::Stmt::ForMarks { index, marks, body } => Self::ForMarks {
                index,
                marks: CheckedExpr::unchecked(marks),
                body: body.into(),
            },
            super::ast::Stmt::ForRange {
                index,
                count,
                cap,
                body,
            } => Self::ForRange {
                index,
                count: CheckedExpr::unchecked(count),
                cap: CheckedExpr::unchecked(cap),
                body: body.into(),
            },
            super::ast::Stmt::Emit { effect, fields } => Self::Emit {
                effect,
                fields: fields
                    .into_iter()
                    .map(|(name, expr)| (name, CheckedExpr::unchecked(expr)))
                    .collect(),
            },
            super::ast::Stmt::Return(expr) => Self::Return(CheckedExpr::unchecked(expr)),
        }
    }
}

fn collect_assignments(
    statement: &CheckedStmt,
    locals: &mut std::collections::HashSet<Identifier>,
    assigned: &mut indexmap::IndexSet<Identifier>,
) {
    match statement {
        CheckedStmt::Local { name, .. } => {
            locals.insert(name.clone());
        }
        CheckedStmt::Assign { name, .. } if !locals.contains(name) => {
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
                    collect_assignments(statement, &mut scoped, assigned);
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
            collect_assignments(initializer, &mut scoped, assigned);
            let mut body_scope = scoped.clone();
            for statement in &body.statements {
                collect_assignments(statement, &mut body_scope, assigned);
            }
            collect_assignments(update, &mut scoped, assigned);
        }
        CheckedStmt::ForMarks { index, body, .. } | CheckedStmt::ForRange { index, body, .. } => {
            let mut scoped = locals.clone();
            scoped.insert(index.clone());
            for statement in &body.statements {
                collect_assignments(statement, &mut scoped, assigned);
            }
        }
        _ => {}
    }
}

impl CheckedExpr {
    fn unchecked(expr: super::ast::Expr) -> Self {
        let kind = match expr.kind {
            super::ast::ExprKind::Literal(value) => CheckedExprKind::Literal(value),
            super::ast::ExprKind::Variable(name) => CheckedExprKind::Variable(name),
            super::ast::ExprKind::Array(items) => {
                CheckedExprKind::Array(items.into_iter().map(Self::unchecked).collect())
            }
            super::ast::ExprKind::Index { target, index } => CheckedExprKind::Index {
                target: Box::new(Self::unchecked(*target)),
                index: Box::new(Self::unchecked(*index)),
            },
            super::ast::ExprKind::Member { target, member } => CheckedExprKind::Member {
                target: Box::new(Self::unchecked(*target)),
                member,
            },
            super::ast::ExprKind::Call { callee, args } => CheckedExprKind::Call {
                callee: Box::new(Self::unchecked(*callee)),
                args: args.into_iter().map(Self::unchecked).collect(),
            },
            super::ast::ExprKind::Unary { op, expr } => CheckedExprKind::Unary {
                op,
                expr: Box::new(Self::unchecked(*expr)),
            },
            super::ast::ExprKind::Binary { op, left, right } => CheckedExprKind::Binary {
                op,
                left: Box::new(Self::unchecked(*left)),
                right: Box::new(Self::unchecked(*right)),
            },
        };
        Self {
            kind,
            span: expr.span,
            ty: Type::Void,
        }
    }
}

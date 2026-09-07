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
    Emit {
        effect: EmittedReference,
        fields: Vec<(Identifier, CheckedExpr)>,
    },
    Return(CheckedExpr),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CheckedExpr {
    pub kind: CheckedExprKind,
    pub span: TextSpan,
    pub ty: Type,
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

use super::ast::{BinaryOp, OperatorInputDecl, ParamDecl, UnaryOp};
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
    pub body: CheckedBlock,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CheckedEffectDecl {
    pub name: Identifier,
    pub params: Vec<ParamDecl>,
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

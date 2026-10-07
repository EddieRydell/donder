//! Untyped syntax tree with source spans. Names are resolved and types
//! checked while the tree is lowered to IR.
use super::lexer::TextSpan;
use crate::dsl::types::Identifier;
use crate::values::Color;

#[derive(Clone, Debug)]
pub(crate) struct Module {
    pub(crate) declarations: Vec<Declaration>,
    pub(crate) functions: Vec<Function>,
}

/// `fn name(arg: type, ...) -> type { body }`, inlined where it is called.
#[derive(Clone, Debug)]
pub(crate) struct Function {
    pub(crate) name: Name,
    pub(crate) description: Option<String>,
    pub(crate) args: Vec<(Name, TypeExpr)>,
    pub(crate) result: TypeExpr,
    pub(crate) body: Block,
    pub(crate) span: TextSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclarationKind {
    Effect,
    Operator,
}

/// A top-level declaration's kind, name and source range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationSpan {
    pub kind: DeclarationKind,
    pub name: Identifier,
    pub span: TextSpan,
}

#[derive(Clone, Debug)]
pub(crate) struct Declaration {
    pub(crate) kind: DeclarationKind,
    pub(crate) name: Name,
    pub(crate) description: Option<String>,
    pub(crate) params: Vec<Param>,
    pub(crate) inputs: Vec<Name>,
    pub(crate) sample: Option<Block>,
    pub(crate) span: TextSpan,
}

#[derive(Clone, Debug)]
pub(crate) struct Name {
    pub(crate) name: Identifier,
    pub(crate) span: TextSpan,
}

#[derive(Clone, Debug)]
pub(crate) struct Param {
    pub(crate) name: Name,
    pub(crate) ty: TypeExpr,
    pub(crate) range: Option<(Literal, Literal)>,
    pub(crate) default: Option<Literal>,
    pub(crate) description: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct TypeExpr {
    pub(crate) kind: TypeKind,
    pub(crate) span: TextSpan,
}

#[derive(Clone, Debug)]
pub(crate) enum TypeKind {
    Named(Identifier),
    Enum(Vec<Name>),
    Array(Box<TypeExpr>),
}

#[derive(Clone, Debug)]
pub(crate) struct Literal {
    pub(crate) kind: LiteralKind,
    pub(crate) span: TextSpan,
}

#[derive(Clone, Debug)]
pub(crate) enum LiteralKind {
    Int(i64),
    Float(f32),
    Bool(bool),
    Color(Color),
    Name(Identifier),
}

#[derive(Clone, Debug)]
pub(crate) struct Block {
    pub(crate) statements: Vec<Statement>,
    pub(crate) result: Box<Expr>,
    pub(crate) span: TextSpan,
}

#[derive(Clone, Debug)]
pub(crate) enum Statement {
    Let {
        name: Name,
        ty: Option<TypeExpr>,
        value: Expr,
    },
    /// Continue only when `condition` holds; otherwise produce `otherwise`, or
    /// nothing when it is absent.
    Guard {
        condition: Expr,
        otherwise: Option<Expr>,
        span: TextSpan,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct Expr {
    pub(crate) kind: ExprKind,
    pub(crate) span: TextSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UnaryOp {
    Negate,
    Not,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    FloorDivide,
    Remainder,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReducerKind {
    Max,
    Min,
    Sum,
    Any,
    All,
    First,
    Last,
}

impl ReducerKind {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "max" => Self::Max,
            "min" => Self::Min,
            "sum" => Self::Sum,
            "any" => Self::Any,
            "all" => Self::All,
            "first" => Self::First,
            "last" => Self::Last,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) enum ExprKind {
    Literal(LiteralKind),
    Name(Identifier),
    /// `pixel.index`, `target.count`.
    Field(Box<Expr>, Name),
    Call(Name, Vec<Expr>),
    /// `source.at(time)`.
    Method(Box<Expr>, Name, Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Block, Box<Expr>),
    /// `[a, b, c]`, which may only be indexed or measured.
    Array(Vec<Expr>),
    /// Boxed to keep expressions, and the parser's frames, small.
    Reduce(Box<Reduction>),
    Block(Block),
}

#[derive(Clone, Debug)]
pub(crate) struct Reduction {
    pub(crate) reducer: ReducerKind,
    pub(crate) index: Name,
    pub(crate) start: Expr,
    pub(crate) end: Expr,
    pub(crate) inclusive: bool,
    pub(crate) body: Block,
    pub(crate) otherwise: Option<Block>,
}

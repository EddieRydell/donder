//! Recursive-descent parser. A syntax error abandons its declaration and
//! parsing resumes at the next one, so one source can report several errors.
//! Syntax trees are at most [`MAX_NESTING`] levels deep, so checking and
//! dropping them cannot exhaust the stack.
use super::ast::*;
use super::lexer::{Keyword, LexError, TextSpan, Token, TokenKind, lex};
use crate::dsl::Diagnostic;
use crate::dsl::types::Identifier;
use crate::values::Color;

const MAX_NESTING: usize = 128;

pub(crate) fn parse(source: &str) -> Result<Module, Vec<Diagnostic>> {
    let mut parser = Parser {
        source,
        tokens: lex(source),
        position: 0,
        depth: 0,
    };
    let mut declarations = Vec::new();
    let mut diagnostics = Vec::new();
    while parser.peek().kind != TokenKind::Eof {
        parser.depth = 0;
        match parser.declaration() {
            Ok(declaration) => declarations.push(declaration),
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                parser.recover();
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(Module { declarations })
    } else {
        Err(diagnostics)
    }
}

type Parsed<T> = Result<T, Diagnostic>;

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    position: usize,
    /// Syntax tree levels open at the current token.
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Token {
        self.tokens[self.position]
    }

    fn peek_at(&self, offset: usize) -> Token {
        self.tokens[(self.position + offset).min(self.tokens.len() - 1)]
    }

    fn advance(&mut self) -> Token {
        let token = self.peek();
        if token.kind != TokenKind::Eof {
            self.position += 1;
        }
        token
    }

    fn previous_span(&self) -> TextSpan {
        self.tokens[self.position.saturating_sub(1)].span
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        let matched = self.at(kind);
        if matched {
            self.advance();
        }
        matched
    }

    fn expect(&mut self, kind: TokenKind, what: &str) -> Parsed<Token> {
        if self.at(kind) {
            Ok(self.advance())
        } else {
            Err(self.unexpected(what))
        }
    }

    fn unexpected(&self, what: &str) -> Diagnostic {
        let token = self.peek();
        let message = match token.kind {
            TokenKind::Error(LexError::UnexpectedCharacter) => "unexpected character".into(),
            TokenKind::Error(LexError::InvalidColor) => {
                "a color literal has six hexadecimal digits, like #ff8800".into()
            }
            TokenKind::Error(LexError::UnterminatedComment) => "unterminated block comment".into(),
            TokenKind::Eof => format!("expected {what}, found the end of the source"),
            _ => format!("expected {what}, found `{}`", self.text(token.span)),
        };
        Diagnostic::new(token.span, message)
    }

    /// Open one more syntax tree level at the current token.
    fn nest(&mut self) -> Parsed<()> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            return Err(Diagnostic::new(
                self.peek().span,
                format!("syntax nesting exceeds {MAX_NESTING} levels"),
            ));
        }
        Ok(())
    }

    fn text(&self, span: TextSpan) -> &str {
        &self.source[span.start..span.end]
    }

    /// Skip to the next declaration keyword.
    fn recover(&mut self) {
        self.advance();
        while !matches!(
            self.peek().kind,
            TokenKind::Eof | TokenKind::Keyword(Keyword::Effect | Keyword::Operator)
        ) {
            self.advance();
        }
    }

    fn name(&mut self, what: &str) -> Parsed<Name> {
        let token = self.expect(TokenKind::Identifier, what)?;
        Ok(Name {
            name: Identifier::new(self.text(token.span).to_string())
                .unwrap_or_else(|_| unreachable!("identifier tokens are valid identifiers")),
            span: token.span,
        })
    }

    fn declaration(&mut self) -> Parsed<Declaration> {
        let start = self.peek().span;
        let kind = match self.peek().kind {
            TokenKind::Keyword(Keyword::Effect) => DeclarationKind::Effect,
            TokenKind::Keyword(Keyword::Operator) => DeclarationKind::Operator,
            _ => return Err(self.unexpected("`effect` or `operator`")),
        };
        self.advance();
        let name = self.name("a declaration name")?;
        self.expect(TokenKind::LeftBrace, "`{`")?;
        let mut params = Vec::new();
        let mut inputs = Vec::new();
        let mut sample = None;
        // Member words are reserved only where a member starts.
        loop {
            let token = self.peek();
            let word = (token.kind == TokenKind::Identifier).then(|| self.text(token.span));
            match (token.kind, word) {
                (TokenKind::RightBrace, _) => break,
                (_, Some("param")) => {
                    self.advance();
                    params.push(self.param()?);
                }
                (_, Some("input")) => {
                    self.advance();
                    inputs.push(self.name("an input name")?);
                    self.expect(TokenKind::Semicolon, "`;`")?;
                }
                (_, Some("sample")) => {
                    let keyword = self.advance();
                    if sample.is_some() {
                        return Err(Diagnostic::new(
                            keyword.span,
                            "a declaration has one `sample` block",
                        ));
                    }
                    sample = Some(self.block()?);
                }
                _ => return Err(self.unexpected("`param`, `input`, `sample` or `}`")),
            }
        }
        let end = self.advance().span;
        Ok(Declaration {
            kind,
            name,
            params,
            inputs,
            sample,
            span: start.to(end),
        })
    }

    fn param(&mut self) -> Parsed<Param> {
        let name = self.name("a parameter name")?;
        self.expect(TokenKind::Colon, "`:` and a type")?;
        let ty = self.type_expr()?;
        let range = if self.eat(TokenKind::Keyword(Keyword::In)) {
            let min = self.literal()?;
            self.expect(TokenKind::DotDot, "`..`")?;
            Some((min, self.literal()?))
        } else {
            None
        };
        let default = if self.eat(TokenKind::Equals) {
            Some(self.literal()?)
        } else {
            None
        };
        self.expect(TokenKind::Semicolon, "`;`")?;
        Ok(Param {
            name,
            ty,
            range,
            default,
        })
    }

    fn type_expr(&mut self) -> Parsed<TypeExpr> {
        let name = self.name("a type")?;
        let kind = match name.name.as_str() {
            "enum" => {
                self.expect(TokenKind::LeftBrace, "`{`")?;
                let mut options = vec![self.name("an enum option")?];
                while self.eat(TokenKind::Comma) {
                    if self.at(TokenKind::RightBrace) {
                        break;
                    }
                    options.push(self.name("an enum option")?);
                }
                self.expect(TokenKind::RightBrace, "`}`")?;
                TypeKind::Enum(options)
            }
            "array" => {
                self.expect(TokenKind::Less, "`<`")?;
                self.nest()?;
                let item = self.type_expr()?;
                self.depth -= 1;
                self.expect(TokenKind::Greater, "`>`")?;
                TypeKind::Array(Box::new(item))
            }
            _ => TypeKind::Named(name.name),
        };
        Ok(TypeExpr {
            kind,
            span: name.span.to(self.previous_span()),
        })
    }

    /// A declaration literal: a number, possibly negative, a bool, a color or
    /// an enum option.
    fn literal(&mut self) -> Parsed<Literal> {
        let start = self.peek().span;
        let negative = self.eat(TokenKind::Minus);
        let token = self.peek();
        let kind = match token.kind {
            TokenKind::Integer | TokenKind::Float => {
                let number = self.number(token)?;
                match (number, negative) {
                    (LiteralKind::Int(value), true) => LiteralKind::Int(-value),
                    (LiteralKind::Float(value), true) => LiteralKind::Float(-value),
                    (number, false) => number,
                    _ => unreachable!(),
                }
            }
            TokenKind::Keyword(Keyword::True) if !negative => LiteralKind::Bool(true),
            TokenKind::Keyword(Keyword::False) if !negative => LiteralKind::Bool(false),
            TokenKind::Color if !negative => self.color(token)?,
            TokenKind::Identifier if !negative => LiteralKind::Name(
                Identifier::new(self.text(token.span).to_string())
                    .unwrap_or_else(|_| unreachable!("identifier tokens are valid identifiers")),
            ),
            _ => return Err(self.unexpected("a literal")),
        };
        self.advance();
        Ok(Literal {
            kind,
            span: start.to(token.span),
        })
    }

    fn number(&self, token: Token) -> Parsed<LiteralKind> {
        let text = self.text(token.span);
        let invalid = || Diagnostic::new(token.span, format!("invalid number `{text}`"));
        Ok(match token.kind {
            TokenKind::Integer => LiteralKind::Int(text.parse().map_err(|_| invalid())?),
            _ => match text.parse::<f32>() {
                Ok(value) if value.is_finite() => LiteralKind::Float(value),
                _ => return Err(Diagnostic::new(token.span, "float literal is out of range")),
            },
        })
    }

    fn color(&self, token: Token) -> Parsed<LiteralKind> {
        Color::from_hex(self.text(token.span))
            .map(LiteralKind::Color)
            .ok_or_else(|| Diagnostic::new(token.span, "invalid color literal"))
    }

    fn block(&mut self) -> Parsed<Block> {
        let start = self.expect(TokenKind::LeftBrace, "`{`")?.span;
        self.nest()?;
        let mut statements = Vec::new();
        loop {
            match self.peek().kind {
                TokenKind::Keyword(Keyword::Let) => {
                    self.advance();
                    let name = self.name("a binding name")?;
                    let ty = if self.eat(TokenKind::Colon) {
                        Some(self.type_expr()?)
                    } else {
                        None
                    };
                    self.expect(TokenKind::Equals, "`=`")?;
                    let value = self.expression()?;
                    self.expect(TokenKind::Semicolon, "`;`")?;
                    statements.push(Statement::Let { name, ty, value });
                }
                TokenKind::Keyword(Keyword::Guard) => {
                    let keyword = self.advance().span;
                    let condition = self.expression()?;
                    let otherwise = if self.eat(TokenKind::Keyword(Keyword::Else)) {
                        Some(self.expression()?)
                    } else {
                        None
                    };
                    let end = self.expect(TokenKind::Semicolon, "`;`")?.span;
                    statements.push(Statement::Guard {
                        condition,
                        otherwise,
                        span: keyword.to(end),
                    });
                }
                _ => break,
            }
        }
        if self.at(TokenKind::RightBrace) {
            return Err(Diagnostic::new(
                self.peek().span,
                "a block ends with the expression it produces",
            ));
        }
        let result = self.expression()?;
        let end = self.expect(TokenKind::RightBrace, "`}` after the block's result")?;
        self.depth -= 1;
        Ok(Block {
            statements,
            result: Box::new(result),
            span: start.to(end.span),
        })
    }

    fn expression(&mut self) -> Parsed<Expr> {
        self.nest()?;
        let expr = self.binary(OR)?;
        self.depth -= 1;
        Ok(expr)
    }

    /// Operators binding at least as tightly as `min`, by precedence climbing.
    /// The left operand deepens with every operator of a chain.
    fn binary(&mut self, min: u8) -> Parsed<Expr> {
        let depth = self.depth;
        let mut left = self.unary()?;
        let mut compared = false;
        while let Some((op, precedence)) = binary_op(self.peek().kind) {
            if precedence < min {
                break;
            }
            if precedence == COMPARISON {
                if compared {
                    return Err(Diagnostic::new(
                        self.peek().span,
                        "comparisons do not chain; combine them with `&&`",
                    ));
                }
                compared = true;
            }
            self.advance();
            self.nest()?;
            let right = self.binary(precedence + 1)?;
            let span = left.span.to(right.span);
            left = Expr {
                kind: ExprKind::Binary(op, Box::new(left), Box::new(right)),
                span,
            };
        }
        self.depth = depth;
        Ok(left)
    }

    fn unary(&mut self) -> Parsed<Expr> {
        let op = match self.peek().kind {
            TokenKind::Minus => UnaryOp::Negate,
            TokenKind::Bang => UnaryOp::Not,
            _ => return self.postfix(),
        };
        let start = self.advance().span;
        self.nest()?;
        let operand = self.unary()?;
        self.depth -= 1;
        let span = start.to(operand.span);
        Ok(Expr {
            kind: ExprKind::Unary(op, Box::new(operand)),
            span,
        })
    }

    fn postfix(&mut self) -> Parsed<Expr> {
        let depth = self.depth;
        let mut expr = self.primary()?;
        loop {
            if matches!(self.peek().kind, TokenKind::LeftBracket | TokenKind::Dot) {
                self.nest()?;
            }
            if self.eat(TokenKind::LeftBracket) {
                let index = self.expression()?;
                let end = self.expect(TokenKind::RightBracket, "`]`")?.span;
                let span = expr.span.to(end);
                expr = Expr {
                    kind: ExprKind::Index(Box::new(expr), Box::new(index)),
                    span,
                };
            } else if self.eat(TokenKind::Dot) {
                let member = self.name("a field or method name")?;
                if self.at(TokenKind::LeftParen) {
                    let (args, end) = self.arguments()?;
                    let span = expr.span.to(end);
                    expr = Expr {
                        kind: ExprKind::Method(Box::new(expr), member, args),
                        span,
                    };
                } else {
                    let span = expr.span.to(member.span);
                    expr = Expr {
                        kind: ExprKind::Field(Box::new(expr), member),
                        span,
                    };
                }
            } else {
                self.depth = depth;
                return Ok(expr);
            }
        }
    }

    fn arguments(&mut self) -> Parsed<(Vec<Expr>, TextSpan)> {
        self.expect(TokenKind::LeftParen, "`(`")?;
        let mut args = Vec::new();
        if !self.at(TokenKind::RightParen) {
            args.push(self.expression()?);
            while self.eat(TokenKind::Comma) {
                args.push(self.expression()?);
            }
        }
        let end = self.expect(TokenKind::RightParen, "`)`")?.span;
        Ok((args, end))
    }

    fn primary(&mut self) -> Parsed<Expr> {
        let token = self.peek();
        let literal = |kind| {
            Ok(Expr {
                kind: ExprKind::Literal(kind),
                span: token.span,
            })
        };
        match token.kind {
            TokenKind::Integer | TokenKind::Float => {
                self.advance();
                literal(self.number(token)?)
            }
            TokenKind::Color => {
                self.advance();
                literal(self.color(token)?)
            }
            TokenKind::Keyword(Keyword::True) => {
                self.advance();
                literal(LiteralKind::Bool(true))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.advance();
                literal(LiteralKind::Bool(false))
            }
            TokenKind::LeftParen => {
                self.advance();
                let inner = self.expression()?;
                let end = self.expect(TokenKind::RightParen, "`)`")?.span;
                Ok(Expr {
                    kind: inner.kind,
                    span: token.span.to(end),
                })
            }
            TokenKind::LeftBracket => {
                self.advance();
                let mut items = vec![self.expression()?];
                while self.eat(TokenKind::Comma) {
                    if self.at(TokenKind::RightBracket) {
                        break;
                    }
                    items.push(self.expression()?);
                }
                let end = self.expect(TokenKind::RightBracket, "`]`")?.span;
                Ok(Expr {
                    kind: ExprKind::Array(items),
                    span: token.span.to(end),
                })
            }
            TokenKind::LeftBrace => {
                let block = self.block()?;
                let span = block.span;
                Ok(Expr {
                    kind: ExprKind::Block(block),
                    span,
                })
            }
            TokenKind::Keyword(Keyword::If) => self.if_expression(),
            TokenKind::Identifier if self.peek_at(1).kind == TokenKind::Keyword(Keyword::For) => {
                self.reduction()
            }
            TokenKind::Identifier => {
                let name = self.name("a name")?;
                if self.at(TokenKind::LeftParen) {
                    let (args, end) = self.arguments()?;
                    let span = name.span.to(end);
                    Ok(Expr {
                        kind: ExprKind::Call(name, args),
                        span,
                    })
                } else {
                    Ok(Expr {
                        kind: ExprKind::Name(name.name),
                        span: name.span,
                    })
                }
            }
            _ => Err(self.unexpected("an expression")),
        }
    }

    fn if_expression(&mut self) -> Parsed<Expr> {
        let start = self.expect(TokenKind::Keyword(Keyword::If), "`if`")?.span;
        let condition = self.expression()?;
        let then = self.block()?;
        if !self.eat(TokenKind::Keyword(Keyword::Else)) {
            return Err(Diagnostic::new(
                then.span,
                "an `if` expression needs an `else`; use `guard` to continue only when a condition holds",
            ));
        }
        let otherwise = if self.at(TokenKind::Keyword(Keyword::If)) {
            self.nest()?;
            let chained = self.if_expression()?;
            self.depth -= 1;
            chained
        } else {
            let block = self.block()?;
            let span = block.span;
            Expr {
                kind: ExprKind::Block(block),
                span,
            }
        };
        let span = start.to(otherwise.span);
        Ok(Expr {
            kind: ExprKind::If(Box::new(condition), then, Box::new(otherwise)),
            span,
        })
    }

    fn reduction(&mut self) -> Parsed<Expr> {
        let name = self.name("a reducer")?;
        let Some(reducer) = ReducerKind::from_name(name.name.as_str()) else {
            return Err(Diagnostic::new(
                name.span,
                format!(
                    "`{}` is not a reducer; use max, min, sum, any, all, first or last",
                    name.name.as_str()
                ),
            ));
        };
        self.expect(TokenKind::Keyword(Keyword::For), "`for`")?;
        let index = self.name("an index name")?;
        self.expect(TokenKind::Keyword(Keyword::In), "`in`")?;
        let start = self.binary(ADDITIVE)?;
        let inclusive = match self.peek().kind {
            TokenKind::DotDot => false,
            TokenKind::DotDotEqual => true,
            _ => return Err(self.unexpected("`..` or `..=`")),
        };
        self.advance();
        let end = self.binary(ADDITIVE)?;
        let body = self.block()?;
        // Only `first` and `last` take a default, so `guard all for .. {..}
        // else value;` keeps its `else` for the guard.
        let defaults = matches!(reducer, ReducerKind::First | ReducerKind::Last);
        let numeric = matches!(
            reducer,
            ReducerKind::Max | ReducerKind::Min | ReducerKind::Sum
        );
        if numeric && self.at(TokenKind::Keyword(Keyword::Else)) {
            return Err(Diagnostic::new(
                self.peek().span,
                format!(
                    "`{}` has no `else`; only `first` and `last` do",
                    name.name.as_str()
                ),
            ));
        }
        let otherwise = if defaults && self.eat(TokenKind::Keyword(Keyword::Else)) {
            Some(self.block()?)
        } else {
            None
        };
        let span = name.span.to(self.previous_span());
        Ok(Expr {
            kind: ExprKind::Reduce(Box::new(Reduction {
                reducer,
                index,
                start,
                end,
                inclusive,
                body,
                otherwise,
            })),
            span,
        })
    }
}

const OR: u8 = 1;
const COMPARISON: u8 = 3;
const ADDITIVE: u8 = 4;

fn binary_op(kind: TokenKind) -> Option<(BinaryOp, u8)> {
    Some(match kind {
        TokenKind::PipePipe => (BinaryOp::Or, OR),
        TokenKind::AmpAmp => (BinaryOp::And, 2),
        TokenKind::Less => (BinaryOp::Less, COMPARISON),
        TokenKind::LessEqual => (BinaryOp::LessEqual, COMPARISON),
        TokenKind::Greater => (BinaryOp::Greater, COMPARISON),
        TokenKind::GreaterEqual => (BinaryOp::GreaterEqual, COMPARISON),
        TokenKind::EqualEqual => (BinaryOp::Equal, COMPARISON),
        TokenKind::BangEqual => (BinaryOp::NotEqual, COMPARISON),
        TokenKind::Plus => (BinaryOp::Add, ADDITIVE),
        TokenKind::Minus => (BinaryOp::Subtract, ADDITIVE),
        TokenKind::Star => (BinaryOp::Multiply, 5),
        TokenKind::Slash => (BinaryOp::Divide, 5),
        TokenKind::Percent => (BinaryOp::Remainder, 5),
        _ => return None,
    })
}

/// Each top-level declaration's kind, name and span, in source order.
pub(crate) fn declaration_spans(source: &str) -> Result<Vec<DeclarationSpan>, Vec<Diagnostic>> {
    Ok(parse(source)?
        .declarations
        .into_iter()
        .map(|declaration| DeclarationSpan {
            kind: declaration.kind,
            name: declaration.name.name,
            span: declaration.span,
        })
        .collect())
}

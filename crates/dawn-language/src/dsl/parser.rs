use super::ast::{
    BinaryOp, Block, EffectDecl, Expr, ExprKind, FunctionDecl, FunctionParam, Module, OperatorDecl,
    OperatorInputDecl, ParamDecl, Stmt, UnaryOp,
};
use super::diagnostic::Diagnostic;
use super::lexer::{Keyword, TextSpan, Token, TokenKind, lex};
use super::types::{Identifier, Type, Value};
use crate::imports::SourceReference;
use crate::imports::{ImportDeclaration, ImportSource};
use crate::values::Color;
use std::sync::Arc;

pub(crate) fn parse_module(source: &str) -> Result<Module, Vec<Diagnostic>> {
    let mut parser = Parser::new(source);
    let module = parser.parse_module();
    if parser.diagnostics.is_empty() {
        Ok(module)
    } else {
        Err(parser.diagnostics)
    }
}

struct Parser<'source> {
    source: &'source str,
    tokens: Vec<Token>,
    cursor: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'source> Parser<'source> {
    fn new(source: &'source str) -> Self {
        let tokens = lex(source);
        let diagnostics = tokens
            .iter()
            .filter_map(|token| match token.kind {
                TokenKind::Error(kind) => {
                    Some(Diagnostic::new(token.span, format!("lex error: {kind:?}")))
                }
                _ => None,
            })
            .collect();
        Self {
            source,
            tokens,
            cursor: 0,
            diagnostics,
        }
    }

    fn parse_module(&mut self) -> Module {
        let mut imports = Vec::new();
        let mut effects = Vec::new();
        let mut operators = Vec::new();
        while !self.at(TokenKind::Eof) {
            let start_cursor = self.cursor;
            if self.consume_keyword(Keyword::Import) {
                if !effects.is_empty() || !operators.is_empty() {
                    self.error_here("imports must precede effect declarations");
                }
                if let Some(import) = self.parse_import() {
                    imports.push(import);
                }
            } else if self.consume_keyword(Keyword::Effect) {
                if let Some(effect) = self.parse_effect() {
                    effects.push(effect);
                }
            } else if self.consume_keyword(Keyword::Operator) {
                if let Some(operator) = self.parse_operator() {
                    operators.push(operator);
                }
            } else {
                self.error_here("expected `effect` or `operator` declaration");
                self.advance();
            }
            self.ensure_progress(start_cursor, "parser made no progress in module");
        }
        Module {
            imports,
            effects,
            operators,
        }
    }

    fn parse_import(&mut self) -> Option<super::EffectImport> {
        let start = self.current().span.start;
        let token = self.current().clone();
        self.advance();
        let alias = match crate::imports::ImportAlias::new(self.text(token.span)) {
            Ok(alias) => alias,
            Err(message) => {
                self.error(token.span, message);
                return None;
            }
        };
        if !self.consume_keyword(Keyword::From) {
            self.error_here("expected `from` after import alias");
            return None;
        }
        let mut source_spans = Vec::new();
        let source = if self.consume(TokenKind::LeftBracket) {
            let mut documents = Vec::new();
            if self.at(TokenKind::RightBracket) {
                self.error_here("local import document list must not be empty");
            }
            while !self.at(TokenKind::RightBracket) && !self.at(TokenKind::Eof) {
                let token = self.current().clone();
                if token.kind != TokenKind::StringLiteral {
                    self.error(token.span, "local import documents must be strings");
                    self.advance();
                } else {
                    self.advance();
                    // Import paths use forward slashes, without string escapes.
                    documents.push(camino::Utf8PathBuf::from(
                        self.source[token.span.start + 1..token.span.end - 1].to_string(),
                    ));
                    source_spans.push(token.span);
                }
                if !self.consume(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(
                TokenKind::RightBracket,
                "expected `]` after local import documents",
            );
            ImportSource::LocalDocuments { documents }
        } else {
            let start = self.current().span.start;
            let dependency = self.parse_package_name()?;
            source_spans.push(TextSpan {
                start,
                end: self.tokens[self.cursor - 1].span.end,
            });
            self.expect(TokenKind::Dot, "expected `.` between dependency and export");
            let start = self.current().span.start;
            let export = self.parse_package_name()?;
            source_spans.push(TextSpan {
                start,
                end: self.tokens[self.cursor - 1].span.end,
            });
            ImportSource::DependencyExport { dependency, export }
        };
        let end = self.current().span.end;
        self.expect(TokenKind::Semicolon, "expected `;` after import");
        Some(super::EffectImport {
            declaration: ImportDeclaration { alias, source },
            span: TextSpan { start, end },
            source_spans,
        })
    }

    fn parse_package_name(&mut self) -> Option<String> {
        let start = self.current().span.start;
        let mut end = start;
        while matches!(
            self.current().kind,
            TokenKind::Identifier
                | TokenKind::Keyword(_)
                | TokenKind::Minus
                | TokenKind::IntegerLiteral
        ) {
            end = self.current().span.end;
            self.advance();
        }
        let name = &self.source[start..end];
        if name.is_empty() {
            self.error(
                TextSpan { start, end },
                "expected a package dependency or export name",
            );
            return None;
        }
        Some(name.to_string())
    }

    fn parse_operator(&mut self) -> Option<OperatorDecl> {
        let name = self.parse_identifier()?;
        self.expect(TokenKind::LeftBrace, "expected `{` after operator name");
        let mut inputs = Vec::new();
        let mut params = Vec::new();
        let mut entrypoint = None;

        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::Eof) {
            let start_cursor = self.cursor;
            if self.consume_keyword(Keyword::Input) {
                let ty = self.parse_type()?;
                let name = self.parse_identifier()?;
                self.expect(TokenKind::Semicolon, "expected `;` after input");
                if ty != Type::Signal {
                    self.error_here("operator inputs must have type `Signal`");
                }
                inputs.push(OperatorInputDecl { name });
            } else if self.at(TokenKind::Keyword(Keyword::Param))
                || self.at(TokenKind::Keyword(Keyword::Fixed))
            {
                if let Some(param) = self.parse_param() {
                    params.push(param);
                }
            } else {
                if entrypoint.is_some() {
                    self.error_here("operator may contain only one entrypoint function");
                }
                entrypoint = self.parse_function();
            }
            self.ensure_progress(start_cursor, "parser made no progress in operator");
        }

        self.expect(TokenKind::RightBrace, "expected `}` after operator body");
        let entrypoint = entrypoint.unwrap_or_else(|| FunctionDecl {
            return_type: Type::Void,
            name: Identifier::new("sample".to_string())
                .unwrap_or_else(|_| unreachable!("static identifier is valid")),
            params: Vec::new(),
            body: Block {
                statements: Vec::new(),
            },
        });
        if entrypoint.return_type == Type::Void && entrypoint.body.statements.is_empty() {
            self.error_here("operator must contain `color sample()`");
        }
        Some(OperatorDecl {
            name,
            inputs,
            params,
            entrypoint,
        })
    }

    fn parse_effect(&mut self) -> Option<EffectDecl> {
        let name = self.parse_identifier()?;
        self.expect(TokenKind::LeftBrace, "expected `{` after effect name");
        let mut params = Vec::new();
        let mut entrypoint = None;

        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::Eof) {
            let start_cursor = self.cursor;
            if self.at(TokenKind::Keyword(Keyword::Param))
                || self.at(TokenKind::Keyword(Keyword::Fixed))
            {
                if let Some(param) = self.parse_param() {
                    params.push(param);
                }
                self.ensure_progress(start_cursor, "parser made no progress in effect");
                continue;
            }

            if entrypoint.is_some() {
                self.error_here("effect may contain only one entrypoint function");
            }
            entrypoint = self.parse_function();
            self.ensure_progress(start_cursor, "parser made no progress in effect");
        }

        self.expect(TokenKind::RightBrace, "expected `}` after effect body");
        let entrypoint = match entrypoint {
            Some(entrypoint) => entrypoint,
            None => {
                self.error_here("effect must contain `color sample()` or `void generate()`");
                let name = match Identifier::new("sample".to_string()) {
                    Ok(identifier) => identifier,
                    Err(_) => return None,
                };
                FunctionDecl {
                    return_type: Type::Void,
                    name,
                    params: Vec::new(),
                    body: Block {
                        statements: Vec::new(),
                    },
                }
            }
        };

        Some(EffectDecl {
            name,
            params,
            entrypoint,
        })
    }

    fn parse_param(&mut self) -> Option<ParamDecl> {
        let fixed = self.consume_keyword(Keyword::Fixed);
        if !self.consume_keyword(Keyword::Param) {
            self.error_here("expected `param` after `fixed`");
            return None;
        }
        if self.consume_keyword(Keyword::Enum) {
            let name = self.parse_identifier()?;
            let ty = self.parse_enum_options()?;
            let default = if self.consume(TokenKind::Equals) {
                let expr = self.parse_expression();
                self.const_value(&expr)
            } else {
                None
            };
            self.expect(TokenKind::Semicolon, "expected `;` after param");
            return Some(ParamDecl {
                name,
                ty,
                default,
                fixed,
            });
        }

        let ty = self.parse_type()?;
        let name = self.parse_identifier()?;
        let default = if self.consume(TokenKind::Equals) {
            let expr = self.parse_expression();
            self.const_value(&expr)
        } else {
            None
        };
        self.expect(TokenKind::Semicolon, "expected `;` after param");
        Some(ParamDecl {
            name,
            ty,
            default,
            fixed,
        })
    }

    fn parse_function(&mut self) -> Option<FunctionDecl> {
        let return_type = self.parse_type()?;
        let name = self.parse_identifier()?;
        self.expect(TokenKind::LeftParen, "expected `(` after function name");
        let mut params = Vec::new();
        if !self.at(TokenKind::RightParen) {
            loop {
                let ty = self.parse_type()?;
                let name = self.parse_identifier()?;
                params.push(FunctionParam { name, ty });
                if !self.consume(TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(TokenKind::RightParen, "expected `)` after function params");
        let body = self.parse_block()?;
        Some(FunctionDecl {
            return_type,
            name,
            params,
            body,
        })
    }

    fn parse_block(&mut self) -> Option<Block> {
        self.expect(TokenKind::LeftBrace, "expected `{` before block");
        let mut statements = Vec::new();
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::Eof) {
            let start_cursor = self.cursor;
            statements.push(self.parse_statement()?);
            self.ensure_progress(start_cursor, "parser made no progress in block");
        }
        self.expect(TokenKind::RightBrace, "expected `}` after block");
        Some(Block { statements })
    }

    fn parse_statement(&mut self) -> Option<Stmt> {
        if self.current().kind == TokenKind::Identifier
            && self.text(self.current().span) == "timeline"
            && self
                .tokens
                .get(self.cursor + 1)
                .is_some_and(|token| token.kind == TokenKind::Dot)
            && self.tokens.get(self.cursor + 2).is_some_and(|token| {
                token.kind == TokenKind::Identifier && self.text(token.span) == "emit"
            })
        {
            return self.parse_emit_statement();
        }

        if self.consume_keyword(Keyword::If) {
            self.expect(TokenKind::LeftParen, "expected `(` after `if`");
            let condition = self.parse_expression();
            self.expect(TokenKind::RightParen, "expected `)` after condition");
            let then_block = self.parse_block()?;
            let else_block = if self.consume_keyword(Keyword::Else) {
                Some(self.parse_block()?)
            } else {
                None
            };
            return Some(Stmt::If {
                condition,
                then_block,
                else_block,
            });
        }

        if self.consume_keyword(Keyword::For) {
            self.expect(TokenKind::LeftParen, "expected `(` after `for`");
            let initializer = Box::new(self.parse_for_clause()?);
            self.expect(TokenKind::Semicolon, "expected `;` after for initializer");
            let condition = self.parse_expression();
            self.expect(TokenKind::Semicolon, "expected `;` after for condition");
            let update = Box::new(self.parse_for_clause()?);
            self.expect(TokenKind::RightParen, "expected `)` after for update");
            let body = self.parse_block()?;
            return Some(Stmt::For {
                initializer,
                condition,
                update,
                body,
            });
        }

        if self.consume_keyword(Keyword::Return) {
            let expr = self.parse_expression();
            self.expect(TokenKind::Semicolon, "expected `;` after return");
            return Some(Stmt::Return(expr));
        }

        if let Some(ty) = self.try_parse_type() {
            let name = self.parse_identifier()?;
            let initializer = if self.consume(TokenKind::Equals) {
                Some(self.parse_expression())
            } else {
                None
            };
            self.expect(TokenKind::Semicolon, "expected `;` after local declaration");
            return Some(Stmt::Local {
                ty,
                name,
                initializer,
            });
        }

        let expr = self.parse_expression();
        if self.consume(TokenKind::Equals) {
            let ExprKind::Variable(name) = expr.kind else {
                self.error(expr.span, "assignment target must be a local or param name");
                self.skip_to_statement_end();
                return Some(Stmt::Expr(expr));
            };
            let value = self.parse_expression();
            self.expect(TokenKind::Semicolon, "expected `;` after assignment");
            return Some(Stmt::Assign { name, value });
        }

        self.expect(TokenKind::Semicolon, "expected `;` after expression");
        Some(Stmt::Expr(expr))
    }

    fn parse_emit_statement(&mut self) -> Option<Stmt> {
        self.advance();
        self.expect(TokenKind::Dot, "expected `.` after `timeline`");
        let emit_name = self.parse_identifier()?;
        if emit_name.as_str() != "emit" {
            self.error_here("expected `emit` after `timeline.`");
        }
        let start = self.current().span.start;
        let reference = self.parse_generated_effect_ref()?;
        let effect = super::EmittedReference {
            arguments: Vec::new(),
            reference,
            span: TextSpan {
                start,
                end: self.tokens[self.cursor - 1].span.end,
            },
        };
        self.expect(TokenKind::LeftBrace, "expected `{` after emitted effect id");
        let mut fields = Vec::new();
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::Eof) {
            let name = self.parse_identifier()?;
            self.expect(TokenKind::Colon, "expected `:` after emit field name");
            let value = self.parse_expression();
            fields.push((name, value));
            if !self.consume(TokenKind::Comma) {
                let _ = self.consume(TokenKind::Semicolon);
            }
        }
        self.expect(TokenKind::RightBrace, "expected `}` after emit fields");
        let _ = self.consume(TokenKind::Semicolon);
        Some(Stmt::Emit { effect, fields })
    }

    fn parse_generated_effect_ref(&mut self) -> Option<SourceReference> {
        let local = self.parse_identifier()?;
        if !self.consume(TokenKind::Dot) {
            return Some(SourceReference::Local(local));
        }
        let name = self.parse_identifier()?;
        if self.at(TokenKind::Dot) {
            self.error_here("generated effect reference must contain exactly two segments");
            return None;
        }
        if local.as_str() == "builtins" {
            return Some(SourceReference::Builtin(name));
        }
        let alias = crate::imports::ImportAlias::new(local.as_str())
            .map_err(|message| self.error_here(message))
            .ok()?;
        Some(SourceReference::Qualified { alias, name })
    }

    fn parse_for_clause(&mut self) -> Option<Stmt> {
        if let Some(ty) = self.try_parse_type() {
            let name = self.parse_identifier()?;
            let initializer = if self.consume(TokenKind::Equals) {
                Some(self.parse_expression())
            } else {
                None
            };
            return Some(Stmt::Local {
                ty,
                name,
                initializer,
            });
        }

        let expr = self.parse_expression();
        if self.consume(TokenKind::Equals) {
            let ExprKind::Variable(name) = expr.kind else {
                self.error(expr.span, "assignment target must be a local or param name");
                return Some(Stmt::Expr(expr));
            };
            let value = self.parse_expression();
            return Some(Stmt::Assign { name, value });
        }

        Some(Stmt::Expr(expr))
    }

    fn parse_expression(&mut self) -> Expr {
        self.parse_precedence(0)
    }

    fn parse_precedence(&mut self, min_precedence: u8) -> Expr {
        let mut left = self.parse_unary();
        while let Some((op, precedence)) = self.current_binary_op() {
            if precedence < min_precedence {
                break;
            }
            self.advance();
            let right = self.parse_precedence(precedence + 1);
            let span = TextSpan {
                start: left.span.start,
                end: right.span.end,
            };
            left = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }
        left
    }

    fn parse_unary(&mut self) -> Expr {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Minus => {
                self.advance();
                let expr = self.parse_unary();
                Expr {
                    span: TextSpan {
                        start: token.span.start,
                        end: expr.span.end,
                    },
                    kind: ExprKind::Unary {
                        op: UnaryOp::Negate,
                        expr: Box::new(expr),
                    },
                }
            }
            TokenKind::Bang => {
                self.advance();
                let expr = self.parse_unary();
                Expr {
                    span: TextSpan {
                        start: token.span.start,
                        end: expr.span.end,
                    },
                    kind: ExprKind::Unary {
                        op: UnaryOp::Not,
                        expr: Box::new(expr),
                    },
                }
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Expr {
        let mut expr = self.parse_primary();
        loop {
            if self.consume(TokenKind::LeftParen) {
                let mut args = Vec::new();
                if !self.at(TokenKind::RightParen) {
                    loop {
                        args.push(self.parse_expression());
                        if !self.consume(TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let end = self.current().span.end;
                self.expect(TokenKind::RightParen, "expected `)` after call arguments");
                expr = Expr {
                    span: TextSpan {
                        start: expr.span.start,
                        end,
                    },
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                };
                continue;
            }

            if self.consume(TokenKind::LeftBracket) {
                let index = self.parse_expression();
                let end = self.current().span.end;
                self.expect(TokenKind::RightBracket, "expected `]` after index");
                expr = Expr {
                    span: TextSpan {
                        start: expr.span.start,
                        end,
                    },
                    kind: ExprKind::Index {
                        target: Box::new(expr),
                        index: Box::new(index),
                    },
                };
                continue;
            }

            if self.consume(TokenKind::Dot) {
                let member = match self.parse_identifier() {
                    Some(member) => member,
                    None => break,
                };
                let end = self.current().span.end;
                expr = Expr {
                    span: TextSpan {
                        start: expr.span.start,
                        end,
                    },
                    kind: ExprKind::Member {
                        target: Box::new(expr),
                        member,
                    },
                };
                continue;
            }

            break;
        }
        expr
    }

    fn parse_primary(&mut self) -> Expr {
        let token = self.current().clone();
        self.advance();
        match token.kind {
            TokenKind::IntegerLiteral => Expr {
                span: token.span,
                kind: ExprKind::Literal(Value::Int(match self.text(token.span).parse() {
                    Ok(value) => value,
                    Err(_) => {
                        self.error(token.span, "integer literal is out of range");
                        0
                    }
                })),
            },
            TokenKind::FloatLiteral => Expr {
                span: token.span,
                kind: ExprKind::Literal(Value::Float(match self.text(token.span).parse::<f32>() {
                    Ok(value) if value.is_finite() => value,
                    _ => {
                        self.error(token.span, "float literal must be finite");
                        0.0
                    }
                })),
            },
            TokenKind::ColorLiteral => Expr {
                span: token.span,
                kind: ExprKind::Literal(Value::Color(self.parse_color(token.span))),
            },
            TokenKind::Keyword(Keyword::True) => Expr {
                span: token.span,
                kind: ExprKind::Literal(Value::Bool(true)),
            },
            TokenKind::Keyword(Keyword::False) => Expr {
                span: token.span,
                kind: ExprKind::Literal(Value::Bool(false)),
            },
            TokenKind::Identifier | TokenKind::Keyword(Keyword::Input) => {
                match self.identifier_from_span(token.span) {
                    Some(identifier) => Expr {
                        span: token.span,
                        kind: ExprKind::Variable(identifier),
                    },
                    None => Expr {
                        span: token.span,
                        kind: ExprKind::Literal(Value::Void),
                    },
                }
            }
            TokenKind::LeftParen => {
                let expr = self.parse_expression();
                self.expect(TokenKind::RightParen, "expected `)` after expression");
                expr
            }
            TokenKind::LeftBracket => {
                let mut items = Vec::new();
                if !self.at(TokenKind::RightBracket) {
                    loop {
                        items.push(self.parse_expression());
                        if !self.consume(TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let end = self.current().span.end;
                self.expect(TokenKind::RightBracket, "expected `]` after array literal");
                Expr {
                    span: TextSpan {
                        start: token.span.start,
                        end,
                    },
                    kind: ExprKind::Array(items),
                }
            }
            _ => {
                self.error(token.span, "expected expression");
                Expr {
                    span: token.span,
                    kind: ExprKind::Literal(Value::Void),
                }
            }
        }
    }

    fn parse_type(&mut self) -> Option<Type> {
        self.try_parse_type().or_else(|| {
            self.error_here("expected type");
            None
        })
    }

    fn try_parse_type(&mut self) -> Option<Type> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Keyword(Keyword::Void) => {
                self.advance();
                Some(Type::Void)
            }
            TokenKind::Keyword(Keyword::Int) => {
                self.advance();
                Some(Type::Int)
            }
            TokenKind::Keyword(Keyword::Float) => {
                self.advance();
                Some(Type::Float)
            }
            TokenKind::Keyword(Keyword::Bool) => {
                self.advance();
                Some(Type::Bool)
            }
            TokenKind::Keyword(Keyword::Color) => {
                self.advance();
                Some(Type::Color)
            }
            TokenKind::Identifier if self.text(token.span) == "Signal" => {
                self.advance();
                Some(Type::Signal)
            }
            TokenKind::Keyword(Keyword::Curve) => {
                self.advance();
                Some(Type::Curve)
            }
            TokenKind::Identifier if self.text(token.span) == "gradient" => {
                self.advance();
                Some(Type::Gradient)
            }
            TokenKind::Keyword(Keyword::Array) => {
                self.advance();
                self.expect(TokenKind::LessThan, "expected `<` after `array`");
                let item = self.parse_type()?;
                if type_contains_signal(&item) {
                    self.error_here("Signal cannot be used as a generic type");
                }
                self.expect(TokenKind::GreaterThan, "expected `>` after array type");
                Some(Type::array(item))
            }
            TokenKind::Keyword(Keyword::Enum) => {
                self.advance();
                self.parse_enum_options()
            }
            TokenKind::Identifier if self.text(token.span) == "marks" => {
                self.advance();
                Some(Type::Marks)
            }
            TokenKind::Identifier if self.text(token.span) == "Timeline" => {
                self.advance();
                Some(Type::Timeline)
            }
            TokenKind::Identifier if self.text(token.span) == "Target" => {
                self.advance();
                Some(Type::Target)
            }
            TokenKind::Identifier if self.text(token.span) == "TargetItems" => {
                self.advance();
                Some(Type::TargetItems)
            }
            TokenKind::Identifier if self.text(token.span) == "TargetItem" => {
                self.advance();
                Some(Type::TargetItem)
            }
            _ => None,
        }
    }

    fn parse_identifier(&mut self) -> Option<Identifier> {
        let token = self.current().clone();
        if token.kind != TokenKind::Identifier && token.kind != TokenKind::Keyword(Keyword::Input) {
            self.error(token.span, "expected identifier");
            return None;
        }
        self.advance();
        self.identifier_from_span(token.span)
    }

    fn parse_enum_options(&mut self) -> Option<Type> {
        self.expect(TokenKind::LeftBrace, "expected `{` after `enum`");
        let mut options = Vec::new();
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::Eof) {
            if let Some(option) = self.parse_identifier() {
                options.push(option);
            }
            if !self.consume(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RightBrace, "expected `}` after enum options");
        Some(Type::Enum(options))
    }

    fn identifier_from_span(&mut self, span: TextSpan) -> Option<Identifier> {
        match Identifier::new(self.text(span).to_string()) {
            Ok(identifier) => Some(identifier),
            Err(error) => {
                self.error(span, format!("invalid identifier: {error:?}"));
                None
            }
        }
    }

    fn const_value(&mut self, expr: &Expr) -> Option<Value> {
        match &expr.kind {
            ExprKind::Literal(value) => Some(value.clone()),
            ExprKind::Variable(identifier) => Some(Value::Enum(identifier.clone())),
            ExprKind::Array(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    let Some(value) = self.const_value(item) else {
                        self.error(item.span, "param array defaults must be literal values");
                        return None;
                    };
                    values.push(value);
                }
                Some(Value::Array(Arc::from(values)))
            }
            _ => {
                self.error(expr.span, "param defaults must be literal values");
                None
            }
        }
    }

    fn current_binary_op(&self) -> Option<(BinaryOp, u8)> {
        match self.current().kind {
            TokenKind::PipePipe => Some((BinaryOp::Or, 1)),
            TokenKind::AmpAmp => Some((BinaryOp::And, 2)),
            TokenKind::EqualEqual => Some((BinaryOp::Equal, 3)),
            TokenKind::BangEqual => Some((BinaryOp::NotEqual, 3)),
            TokenKind::LessThan => Some((BinaryOp::Less, 4)),
            TokenKind::LessEqual => Some((BinaryOp::LessEqual, 4)),
            TokenKind::GreaterThan => Some((BinaryOp::Greater, 4)),
            TokenKind::GreaterEqual => Some((BinaryOp::GreaterEqual, 4)),
            TokenKind::Plus => Some((BinaryOp::Add, 5)),
            TokenKind::Minus => Some((BinaryOp::Subtract, 5)),
            TokenKind::Star => Some((BinaryOp::Multiply, 6)),
            TokenKind::Slash => Some((BinaryOp::Divide, 6)),
            TokenKind::Percent => Some((BinaryOp::Remainder, 6)),
            _ => None,
        }
    }

    fn parse_color(&mut self, span: TextSpan) -> Color {
        let text = self.text(span);
        match Color::from_hex(text) {
            Some(color) => color,
            None => {
                self.error(span, "invalid color literal");
                Color {
                    red: 0,
                    green: 0,
                    blue: 0,
                }
            }
        }
    }

    fn expect(&mut self, kind: TokenKind, message: &str) {
        if !self.consume(kind) {
            self.error_here(message);
        }
    }

    fn consume_keyword(&mut self, keyword: Keyword) -> bool {
        if self.current().kind == TokenKind::Keyword(keyword) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn consume(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn current(&self) -> &Token {
        self.tokens
            .get(self.cursor)
            .or_else(|| self.tokens.last())
            .unwrap_or(&EOF_TOKEN)
    }

    fn advance(&mut self) {
        if self.cursor + 1 < self.tokens.len() {
            self.cursor += 1;
        }
    }

    fn skip_to_statement_end(&mut self) {
        while !self.at(TokenKind::Semicolon)
            && !self.at(TokenKind::RightBrace)
            && !self.at(TokenKind::Eof)
        {
            self.advance();
        }
        let _ = self.consume(TokenKind::Semicolon);
    }

    fn text(&self, span: TextSpan) -> &str {
        &self.source[span.start..span.end]
    }

    fn error_here(&mut self, message: impl Into<String>) {
        self.error(self.current().span, message);
    }

    fn error(&mut self, span: TextSpan, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::new(span, message));
    }

    fn ensure_progress(&mut self, start_cursor: usize, message: &str) {
        if self.cursor == start_cursor && !self.at(TokenKind::Eof) {
            self.error_here(message);
            self.advance();
        }
    }
}

static EOF_TOKEN: Token = Token {
    kind: TokenKind::Eof,
    span: TextSpan { start: 0, end: 0 },
};

fn type_contains_signal(ty: &Type) -> bool {
    match ty {
        Type::Signal => true,
        Type::Array(inner) => type_contains_signal(inner),
        _ => false,
    }
}

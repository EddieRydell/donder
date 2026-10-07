//! Error-tolerant parser for data documents. A malformed field or item is
//! reported and replaced by [`DataValue::Error`], and parsing resumes at the
//! next `,` or closing bracket, so the rest of the document keeps its tree.
use super::literal;
use super::tree::*;
use crate::dsl::Diagnostic;
use crate::dsl::Identifier;
use crate::dsl::TextSpan;
use crate::dsl::syntax::lexer::{Keyword, LexMode, Token, TokenKind, lex_mode};
use crate::values::Color;

const MAX_NESTING: usize = 128;

/// Parse a data document. The tree is always returned, with every error
/// reported in `diagnostics`; a document is valid when there are none.
pub fn parse(source: &str) -> (DataDocument, Vec<Diagnostic>) {
    let mut parser = Parser {
        source,
        tokens: lex_mode(source, LexMode::Data),
        position: 0,
        depth: 0,
        diagnostics: Vec::new(),
    };
    let document = parser.document();
    (document, parser.diagnostics)
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    position: usize,
    depth: usize,
    diagnostics: Vec<Diagnostic>,
}

/// A syntax error already reported; the caller recovers.
struct Failed;

type Parsed<T> = Result<T, Failed>;

impl Parser<'_> {
    fn peek(&self) -> Token {
        self.tokens[self.position]
    }

    fn peek_at(&self, offset: usize) -> Token {
        self.tokens[(self.position + offset).min(self.tokens.len() - 1)]
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    fn advance(&mut self) -> Token {
        let token = self.peek();
        if token.kind != TokenKind::Eof {
            self.position += 1;
        }
        token
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        let matched = self.at(kind);
        if matched {
            self.advance();
        }
        matched
    }

    fn text(&self, span: TextSpan) -> &str {
        &self.source[span.start..span.end]
    }

    fn error(&mut self, span: TextSpan, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::new(span, message));
    }

    /// Report the current token as unexpected, once: enclosing containers
    /// recovering from the same token add nothing.
    fn unexpected(&mut self, what: &str) -> Failed {
        let token = self.peek();
        if self
            .diagnostics
            .last()
            .is_some_and(|last| last.span == token.span)
        {
            return Failed;
        }
        let message = match token.kind {
            TokenKind::Error(error) => error.message().to_string(),
            TokenKind::Eof => format!("expected {what}, found the end of the document"),
            _ => format!("expected {what}, found `{}`", self.text(token.span)),
        };
        self.error(token.span, message);
        Failed
    }

    fn expect(&mut self, kind: TokenKind, what: &str) -> Parsed<Token> {
        if self.at(kind) {
            Ok(self.advance())
        } else {
            Err(self.unexpected(what))
        }
    }

    /// An identifier with a casing rule.
    fn identifier(&mut self, what: &str, pascal: bool) -> Parsed<Spanned<Identifier>> {
        let token = self.expect(TokenKind::Identifier, what)?;
        let text = self.text(token.span).to_string();
        let cased = if pascal {
            is_pascal_case(&text)
        } else {
            is_snake_case(&text)
        };
        if !cased {
            let rule = if pascal { "PascalCase" } else { "snake_case" };
            self.error(token.span, format!("{what} must be {rule}"));
        }
        let identifier = Identifier::new(text)
            .unwrap_or_else(|_| unreachable!("identifier tokens are valid identifiers"));
        Ok(Spanned::new(identifier, token.span))
    }

    fn document(&mut self) -> DataDocument {
        let mut document = DataDocument::default();
        while !self.at(TokenKind::Eof) {
            let parsed = if self.at(TokenKind::Keyword(Keyword::Import)) {
                self.import().map(|import| document.imports.push(import))
            } else {
                self.declaration()
                    .map(|declaration| document.declarations.push(declaration))
            };
            if parsed.is_err() {
                self.recover_declaration();
            }
        }
        document
    }

    /// Skip to the next `import` or `Type name {` at the top level.
    fn recover_declaration(&mut self) {
        self.advance();
        let mut depth = 0_usize;
        loop {
            let kind = self.peek().kind;
            match kind {
                TokenKind::Eof => return,
                TokenKind::LeftBrace | TokenKind::LeftBracket | TokenKind::LeftParen => depth += 1,
                TokenKind::RightBrace | TokenKind::RightBracket | TokenKind::RightParen => {
                    depth = depth.saturating_sub(1)
                }
                TokenKind::Keyword(Keyword::Import) if depth == 0 => return,
                TokenKind::Identifier
                    if depth == 0
                        && self.peek_at(1).kind == TokenKind::Identifier
                        && self.peek_at(2).kind == TokenKind::LeftBrace =>
                {
                    return;
                }
                _ => {}
            }
            self.advance();
        }
    }

    fn import(&mut self) -> Parsed<DataImport> {
        self.advance();
        let alias = self.identifier("an import alias", false)?;
        self.expect(TokenKind::Keyword(Keyword::From), "`from`")?;
        let mut paths = vec![self.path()?];
        while self.eat(TokenKind::Comma) {
            paths.push(self.path()?);
        }
        self.expect(TokenKind::Semicolon, "`;` after the import")?;
        Ok(DataImport { alias, paths })
    }

    fn path(&mut self) -> Parsed<Spanned<String>> {
        let token = self.expect(
            TokenKind::Path,
            "a path like `<sequences/main.data.donder>`",
        )?;
        let text = self.text(token.span);
        Ok(Spanned::new(
            text[1..text.len() - 1].to_string(),
            token.span,
        ))
    }

    fn declaration(&mut self) -> Parsed<DataDeclaration> {
        let ty = self.identifier("a declaration's type", true)?;
        let name = self.identifier("a declaration's name", false)?;
        let fields = self.fields()?;
        Ok(DataDeclaration { ty, name, fields })
    }

    fn nest(&mut self) -> Parsed<()> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            let span = self.peek().span;
            self.error(span, format!("values nest at most {MAX_NESTING} levels"));
            return Err(Failed);
        }
        Ok(())
    }

    /// `{ name: value, ... }`. A malformed field is reported and skipped.
    fn fields(&mut self) -> Parsed<Spanned<Vec<DataField>>> {
        let open = self.expect(TokenKind::LeftBrace, "`{`")?.span;
        self.nest()?;
        let mut fields = Vec::new();
        loop {
            if self.at(TokenKind::RightBrace) {
                break;
            }
            if self.at(TokenKind::Eof) || self.at_other_close(TokenKind::RightBrace) {
                self.unexpected("`}`");
                self.depth -= 1;
                return Err(Failed);
            }
            match self.field() {
                Ok(field) => fields.push(field),
                Err(Failed) => self.recover_item(TokenKind::RightBrace),
            }
            if !self.eat(TokenKind::Comma) && !self.at(TokenKind::RightBrace) {
                self.unexpected("`,` or `}`");
                self.recover_item(TokenKind::RightBrace);
                self.eat(TokenKind::Comma);
            }
        }
        let close = self.advance().span;
        self.depth -= 1;
        Ok(Spanned::new(fields, open.to(close)))
    }

    fn field(&mut self) -> Parsed<DataField> {
        // `import` and `from` only begin imports, so before a `:` they are
        // field names, like an edge's `from`.
        let name = match self.peek().kind {
            TokenKind::Keyword(Keyword::Import | Keyword::From) => {
                let token = self.advance();
                let identifier = Identifier::new(self.text(token.span).to_string())
                    .unwrap_or_else(|_| unreachable!("keywords are valid identifiers"));
                Spanned::new(identifier, token.span)
            }
            _ => self.identifier("a field name", false)?,
        };
        // A named field whose value fails stays, holding an error, so the
        // schema knows the record is incomplete rather than missing a field.
        let span = self.peek().span;
        let value = match self.expect(TokenKind::Colon, "`:` and a value") {
            Ok(_) => self.value(),
            Err(failed) => Err(failed),
        };
        let value = value.unwrap_or_else(|Failed| {
            self.recover_item(TokenKind::RightBrace);
            Spanned::new(DataValue::Error, span)
        });
        Ok(DataField { name, value })
    }

    /// Whether the next token closes some other bracket than `close`, which
    /// ends this one too: the enclosing container recovers from it.
    fn at_other_close(&self, close: TokenKind) -> bool {
        let kind = self.peek().kind;
        kind != close
            && matches!(
                kind,
                TokenKind::RightBrace | TokenKind::RightBracket | TokenKind::RightParen
            )
    }

    /// Skip to the next `,` or `close` at this nesting level.
    fn recover_item(&mut self, close: TokenKind) {
        let mut depth = 0_usize;
        loop {
            let kind = self.peek().kind;
            match kind {
                TokenKind::Eof => return,
                TokenKind::Comma if depth == 0 => return,
                kind if kind == close && depth == 0 => return,
                TokenKind::LeftBrace | TokenKind::LeftBracket | TokenKind::LeftParen => depth += 1,
                TokenKind::RightBrace | TokenKind::RightBracket | TokenKind::RightParen => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                }
                _ => {}
            }
            self.advance();
        }
    }

    /// Items between brackets, each parsed by `item`.
    fn items(
        &mut self,
        close: TokenKind,
        what: &str,
        mut item: impl FnMut(&mut Self) -> Parsed<Spanned<DataValue>>,
    ) -> Parsed<(Vec<Spanned<DataValue>>, TextSpan)> {
        let open = self.advance().span;
        self.nest()?;
        let mut items = Vec::new();
        loop {
            if self.at(close) {
                break;
            }
            if self.at(TokenKind::Eof) || self.at_other_close(close) {
                self.unexpected(what);
                self.depth -= 1;
                return Err(Failed);
            }
            match item(self) {
                Ok(value) => items.push(value),
                Err(Failed) => {
                    let span = self.peek().span;
                    self.recover_item(close);
                    items.push(Spanned::new(DataValue::Error, span));
                }
            }
            if !self.eat(TokenKind::Comma) && !self.at(close) {
                self.unexpected(&format!("`,` or {what}"));
                self.recover_item(close);
                self.eat(TokenKind::Comma);
            }
        }
        let end = self.advance().span;
        self.depth -= 1;
        Ok((items, open.to(end)))
    }

    fn value(&mut self) -> Parsed<Spanned<DataValue>> {
        let token = self.peek();
        let literal = |parser: &mut Self, value| {
            parser.advance();
            Ok(Spanned::new(value, token.span))
        };
        match token.kind {
            TokenKind::Minus => self.number(),
            TokenKind::Integer | TokenKind::Float | TokenKind::Duration | TokenKind::Distance => {
                self.number()
            }
            TokenKind::Color => {
                let text = self.text(token.span).to_string();
                let color = Color::from_hex(&text).unwrap_or(Color::BLACK);
                if color.to_hex() != text {
                    let message = format!("write `{}` in lowercase", color.to_hex());
                    self.diagnostics
                        .push(Diagnostic::new(token.span, message).with_fix(color.to_hex()));
                }
                literal(self, DataValue::Color(color))
            }
            TokenKind::String => {
                let value = self.string(token.span);
                literal(self, DataValue::String(value))
            }
            TokenKind::Path => {
                let text = self.text(token.span);
                let path = text[1..text.len() - 1].to_string();
                literal(self, DataValue::Path(path))
            }
            TokenKind::Keyword(Keyword::True) => literal(self, DataValue::Bool(true)),
            TokenKind::Keyword(Keyword::False) => literal(self, DataValue::Bool(false)),
            TokenKind::Keyword(Keyword::None) => literal(self, DataValue::None),
            TokenKind::LeftBrace => {
                let fields = self.fields()?;
                Ok(Spanned::new(DataValue::Map(fields.value), fields.span))
            }
            TokenKind::LeftBracket => {
                let (items, span) = self.items(TokenKind::RightBracket, "`]`", Self::value)?;
                Ok(Spanned::new(DataValue::List(items), span))
            }
            TokenKind::LeftParen => {
                let (items, span) = self.items(TokenKind::RightParen, "`)`", Self::value)?;
                if items.len() < 2 {
                    self.error(span, "a tuple has at least two items");
                }
                Ok(Spanned::new(DataValue::Tuple(items), span))
            }
            TokenKind::Identifier => self.name_value(),
            _ => Err(self.unexpected("a value")),
        }
    }

    /// A reference, a fieldless variant or a record, told apart by case and
    /// what follows: `alias.name`, `name`, `Multicast`, `Port { ... }`.
    fn name_value(&mut self) -> Parsed<Spanned<DataValue>> {
        let first = self.peek();
        let text = self.text(first.span).to_string();
        let dotted = self.peek_at(1).kind == TokenKind::Dot;
        if is_pascal_case(&text) && !dotted {
            let ty = self.identifier("a type", true)?;
            if self.at(TokenKind::Identifier) && self.peek_at(1).kind == TokenKind::LeftBrace {
                let name = self.identifier("an object's name", false)?;
                let fields = self.fields()?;
                let span = ty.span.to(fields.span);
                return Ok(Spanned::new(DataValue::Named(ty, name, fields), span));
            }
            if self.at(TokenKind::LeftBrace) {
                let fields = self.fields()?;
                let span = ty.span.to(fields.span);
                return Ok(Spanned::new(DataValue::Record(ty, fields), span));
            }
            let span = ty.span;
            return Ok(Spanned::new(DataValue::Variant(ty), span));
        }
        let mut segments = vec![self.identifier("a reference", false)?];
        while self.eat(TokenKind::Dot) {
            // Later segments name declarations, which may be PascalCase
            // effect and operator definitions.
            let token = self.expect(TokenKind::Identifier, "a name after `.`")?;
            let identifier = Identifier::new(self.text(token.span).to_string())
                .unwrap_or_else(|_| unreachable!("identifier tokens are valid identifiers"));
            segments.push(Spanned::new(identifier, token.span));
        }
        let span = segments[0].span.to(segments[segments.len() - 1].span);
        Ok(Spanned::new(DataValue::Reference(segments), span))
    }

    fn number(&mut self) -> Parsed<Spanned<DataValue>> {
        let start = self.peek().span;
        let negative = self.eat(TokenKind::Minus);
        let token = self.peek();
        let span = start.to(token.span);
        let text = format!(
            "{}{}",
            if negative { "-" } else { "" },
            self.text(token.span)
        );
        let value = match token.kind {
            TokenKind::Integer => literal::integer(&text).map(DataValue::Integer),
            TokenKind::Float => literal::float(&text).map(DataValue::Float),
            TokenKind::Duration if negative => Err("durations are never negative".into()),
            TokenKind::Duration => {
                literal::duration(&text[..text.len() - 1]).map(DataValue::Duration)
            }
            TokenKind::Distance => {
                literal::distance(&text[..text.len() - 1]).map(DataValue::Distance)
            }
            _ => return Err(self.unexpected("a number")),
        };
        self.advance();
        match value {
            Ok(value) => Ok(Spanned::new(value, span)),
            Err(invalid) => {
                let mut diagnostic = Diagnostic::new(span, invalid.message);
                diagnostic.fix = invalid.fix;
                self.diagnostics.push(diagnostic);
                Ok(Spanned::new(DataValue::Error, span))
            }
        }
    }

    /// Decode a string token's escapes, reporting invalid ones.
    fn string(&mut self, span: TextSpan) -> String {
        match literal::string(self.text(span)) {
            Ok(value) => value,
            Err(message) => {
                self.error(span, message);
                String::new()
            }
        }
    }
}

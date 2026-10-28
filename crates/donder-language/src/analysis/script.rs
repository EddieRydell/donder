//! Script analysis: the names a check of every declaration that parses
//! declares and resolves; completion and signature help read tokens, so they
//! work while the text does not parse.
use super::{
    Completion, CompletionKind, SemanticToken, Signature, SignatureHelp, Symbol, SymbolKind,
    TokenClass, contains,
};
use crate::compiler::builtins::{
    ARRAY_WORD, AT, BUILTINS, Builtin, CONTEXT, ContextValue, ENUM_WORD, Method, NAMED_TYPES,
    REDUCERS, Reducer, builtin, type_text,
};
use crate::compiler::syntax::lexer::{Keyword, LexMode, Token, TokenKind, lex_with_comments};
use crate::compiler::{Diagnostic, TextSpan};

/// What a name refers to.
#[derive(Clone, Copy, Debug)]
pub enum ScriptTarget {
    Symbol(usize),
    Builtin(&'static Builtin),
    Context(&'static ContextValue),
    Reducer(&'static Reducer),
    Method(&'static Method),
}

#[derive(Clone, Copy, Debug)]
pub struct ScriptReference {
    pub span: TextSpan,
    pub target: ScriptTarget,
}

/// One script document, analyzed.
#[derive(Clone, Debug, Default)]
pub struct ScriptAnalysis {
    /// Every error: syntax, checking and lowering.
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<Symbol>,
    pub references: Vec<ScriptReference>,
    pub tokens: Vec<SemanticToken>,
}

impl ScriptAnalysis {
    /// The symbol or builtin named at `offset`, and the name's span.
    pub fn target_at(&self, offset: usize) -> Option<(TextSpan, ScriptTarget)> {
        self.symbols
            .iter()
            .enumerate()
            .find(|(_, symbol)| contains(symbol.span, offset))
            .map(|(index, symbol)| (symbol.span, ScriptTarget::Symbol(index)))
            .or_else(|| {
                self.references
                    .iter()
                    .find(|reference| contains(reference.span, offset))
                    .map(|reference| (reference.span, reference.target))
            })
    }

    /// The declaration and every use of a symbol.
    pub fn occurrences(&self, symbol: usize) -> Vec<TextSpan> {
        std::iter::once(self.symbols[symbol].span)
            .chain(self.references.iter().filter_map(|reference| {
                matches!(reference.target, ScriptTarget::Symbol(target) if target == symbol)
                    .then_some(reference.span)
            }))
            .collect()
    }

    /// Markdown describing a target.
    pub fn hover(&self, target: ScriptTarget) -> String {
        match target {
            ScriptTarget::Symbol(index) => {
                let symbol = &self.symbols[index];
                let mut text = format!("```donder\n{}\n```", symbol.detail);
                if let Some(description) = &symbol.description {
                    text.push_str("\n\n");
                    text.push_str(description);
                }
                text
            }
            ScriptTarget::Builtin(builtin) => builtin_documentation(builtin),
            ScriptTarget::Context(value) => {
                format!(
                    "```donder\n{}: {}\n```\n\n{}",
                    value.name,
                    type_text(&value.ty),
                    value.summary
                )
            }
            ScriptTarget::Reducer(reducer) => format!(
                "```donder\n{} for i in start..end {{ body }}\n```\n\n{}",
                reducer.name, reducer.summary
            ),
            ScriptTarget::Method(method) => {
                format!("```donder\n{}\n```\n\n{}", method.signature, method.summary)
            }
        }
    }
}

fn builtin_documentation(builtin: &Builtin) -> String {
    let signatures = builtin
        .signatures
        .iter()
        .map(|signature| signature.label(builtin.name))
        .collect::<Vec<_>>()
        .join("\n");
    let mut text = format!("```donder\n{signatures}\n```\n\n{}", builtin.summary);
    if !builtin.details.is_empty() {
        text.push_str("\n\n");
        text.push_str(builtin.details);
    }
    if !builtin.example.is_empty() {
        text.push_str(&format!("\n\n```donder\n{}\n```", builtin.example));
    }
    text
}

/// The words that start a type: its names, `enum` and `array`.
fn type_words() -> impl Iterator<Item = &'static str> {
    NAMED_TYPES
        .iter()
        .map(|(name, _)| *name)
        .chain([ENUM_WORD, ARRAY_WORD])
}

pub fn analyze_script(source: &str) -> ScriptAnalysis {
    let (module, parse_diagnostics) = crate::compiler::syntax::parse_partial(source);
    let (checked, names) = crate::compiler::check::check_recording(module, source, true);
    // The same errors as compiling: syntax first, then checking, then lowering.
    let diagnostics = if !parse_diagnostics.is_empty() {
        parse_diagnostics
    } else {
        match checked {
            Ok(definitions) => crate::compiler::lowering_diagnostics(&definitions),
            Err(diagnostics) => diagnostics,
        }
    };
    let tokens = semantic_tokens(source, &names.symbols, &names.references);
    ScriptAnalysis {
        diagnostics,
        symbols: names.symbols,
        references: names.references,
        tokens,
    }
}

fn symbol_class(kind: SymbolKind) -> TokenClass {
    match kind {
        SymbolKind::Effect | SymbolKind::Operator => TokenClass::Type,
        SymbolKind::Function => TokenClass::Function,
        SymbolKind::Param | SymbolKind::Input | SymbolKind::Argument => TokenClass::Parameter,
        SymbolKind::Let | SymbolKind::Index => TokenClass::Variable,
        SymbolKind::EnumOption => TokenClass::EnumMember,
    }
}

fn semantic_tokens(
    source: &str,
    symbols: &[Symbol],
    references: &[ScriptReference],
) -> Vec<SemanticToken> {
    let (tokens, comments) = lex_with_comments(source, LexMode::Script);
    let mut result = comments
        .into_iter()
        .map(|span| SemanticToken {
            span,
            class: TokenClass::Comment,
            declaration: false,
        })
        .collect::<Vec<_>>();
    for (index, token) in tokens.iter().enumerate() {
        let class = match token.kind {
            TokenKind::Keyword(Keyword::True | Keyword::False) => TokenClass::Number,
            TokenKind::Keyword(_) => TokenClass::Keyword,
            TokenKind::Integer | TokenKind::Float => TokenClass::Number,
            TokenKind::String | TokenKind::Color => TokenClass::String,
            TokenKind::Identifier => {
                let text = &source[token.span.start..token.span.end];
                if let Some(symbol) = symbols.iter().find(|symbol| symbol.span == token.span) {
                    result.push(SemanticToken {
                        span: token.span,
                        class: symbol_class(symbol.kind),
                        declaration: true,
                    });
                    continue;
                }
                if let Some(reference) = references
                    .iter()
                    .find(|reference| reference.span == token.span)
                {
                    match reference.target {
                        ScriptTarget::Symbol(symbol) => symbol_class(symbols[symbol].kind),
                        ScriptTarget::Builtin(_) | ScriptTarget::Reducer(_) => TokenClass::Builtin,
                        ScriptTarget::Context(_) => TokenClass::Context,
                        ScriptTarget::Method(_) => TokenClass::Function,
                    }
                } else if member_word(&tokens, index, text) {
                    TokenClass::Keyword
                } else if type_words().any(|word| word == text) {
                    TokenClass::Type
                } else {
                    TokenClass::Variable
                }
            }
            TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::SlashSlash
            | TokenKind::Percent
            | TokenKind::EqualEqual
            | TokenKind::BangEqual
            | TokenKind::Less
            | TokenKind::LessEqual
            | TokenKind::Greater
            | TokenKind::GreaterEqual
            | TokenKind::AmpAmp
            | TokenKind::PipePipe
            | TokenKind::Bang
            | TokenKind::Arrow
            | TokenKind::DotDot
            | TokenKind::DotDotEqual
            | TokenKind::Equals => TokenClass::Operator,
            _ => continue,
        };
        result.push(SemanticToken {
            span: token.span,
            class,
            declaration: false,
        });
    }
    result.sort_by_key(|token| token.span.start);
    result
}

/// `param`, `input` and `sample` where a declaration's member starts.
fn member_word(tokens: &[Token], index: usize, text: &str) -> bool {
    matches!(text, "param" | "input" | "sample")
        && index > 0
        && matches!(
            tokens[index - 1].kind,
            TokenKind::LeftBrace | TokenKind::Semicolon | TokenKind::RightBrace
        )
}

/// What the cursor is in, read from the tokens before it.
struct Context<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    /// Tokens before the cursor, excluding a partly typed name.
    before: usize,
}

impl<'a> Context<'a> {
    fn new(source: &'a str, offset: usize) -> Self {
        let (tokens, _) = lex_with_comments(source, LexMode::Script);
        let before = tokens
            .iter()
            .position(|token| {
                token.kind == TokenKind::Eof
                    || token.span.end >= offset && token.kind == TokenKind::Identifier
                    || token.span.start >= offset
            })
            .unwrap_or(tokens.len());
        Self {
            source,
            tokens,
            before,
        }
    }

    fn text(&self, token: Token) -> &'a str {
        &self.source[token.span.start..token.span.end]
    }

    fn previous(&self, back: usize) -> Option<Token> {
        self.before
            .checked_sub(back)
            .and_then(|index| self.tokens.get(index))
            .copied()
    }

    /// The tokens of the innermost declaration or function the cursor is in.
    fn enclosing(&self) -> Option<(Token, &[Token])> {
        let mut depth = 0_usize;
        for index in (0..self.before).rev() {
            let token = self.tokens[index];
            match token.kind {
                TokenKind::RightBrace => depth += 1,
                TokenKind::LeftBrace => depth = depth.saturating_sub(1),
                TokenKind::Keyword(Keyword::Effect | Keyword::Operator | Keyword::Fn)
                    if depth == 0 =>
                {
                    return Some((token, &self.tokens[index..self.before]));
                }
                _ => {}
            }
        }
        None
    }
}

/// Completions at `offset` in a script.
pub fn script_completions(source: &str, offset: usize) -> Vec<Completion> {
    let context = Context::new(source, offset);
    let previous = context.previous(1);
    // After `pixel.` or `target.`, or an input's `.`.
    if previous.is_some_and(|token| token.kind == TokenKind::Dot) {
        let Some(scope) = context.previous(2) else {
            return Vec::new();
        };
        let scope = context.text(scope);
        let fields = CONTEXT
            .iter()
            .filter_map(|value| {
                let (owner, field) = value.field()?;
                (owner == scope).then(|| {
                    Completion::new(field, CompletionKind::Field)
                        .detail(type_text(&value.ty))
                        .documentation(value.summary)
                })
            })
            .collect::<Vec<_>>();
        if !fields.is_empty() {
            return fields;
        }
        return vec![
            Completion::new(AT.name, CompletionKind::Function)
                .detail(AT.signature)
                .documentation(AT.summary),
        ];
    }
    // A type after `:` in a parameter, a `let` or an argument.
    if previous.is_some_and(|token| token.kind == TokenKind::Colon) {
        return type_words()
            .map(|ty| Completion::new(ty, CompletionKind::Type))
            .collect();
    }
    let Some((keyword, tokens)) = context.enclosing() else {
        return [Keyword::Effect, Keyword::Operator, Keyword::Fn]
            .into_iter()
            .map(|keyword| Completion::new(keyword.text(), CompletionKind::Keyword))
            .collect();
    };
    let mut completions = Vec::new();
    let in_function = keyword.kind == TokenKind::Keyword(Keyword::Fn);
    let in_sample = tokens
        .iter()
        .any(|token| context.text(*token) == "sample" && token.kind == TokenKind::Identifier);
    if !in_function && !in_sample {
        for word in ["param", "input", "sample"] {
            completions.push(Completion::new(word, CompletionKind::Keyword));
        }
        return completions;
    }
    // Names in scope: parameters, inputs and enum options of the declaration,
    // or a function's arguments, and `let`s and indexes of open blocks.
    let mut open = vec![Vec::new()];
    let mut pending_index = None;
    for (index, token) in tokens.iter().enumerate() {
        let next = tokens.get(index + 1).copied();
        match token.kind {
            TokenKind::LeftBrace => {
                let mut scope = Vec::new();
                if let Some(name) = pending_index.take() {
                    scope.push(name);
                }
                open.push(scope);
            }
            TokenKind::RightBrace => {
                open.pop();
                if open.is_empty() {
                    open.push(Vec::new());
                }
            }
            TokenKind::Keyword(Keyword::Let)
                if next.is_some_and(|next| next.kind == TokenKind::Identifier) =>
            {
                if let (Some(scope), Some(next)) = (open.last_mut(), next) {
                    scope.push((context.text(next), CompletionKind::Variable, "let"));
                }
            }
            TokenKind::Keyword(Keyword::For)
                if next.is_some_and(|next| next.kind == TokenKind::Identifier) =>
            {
                pending_index =
                    next.map(|next| (context.text(next), CompletionKind::Variable, "int"));
            }
            TokenKind::Identifier
                if matches!(context.text(*token), "param" | "input")
                    && next.is_some_and(|next| next.kind == TokenKind::Identifier) =>
            {
                if let (Some(scope), Some(next)) = (open.first_mut(), next) {
                    scope.push((
                        context.text(next),
                        CompletionKind::Variable,
                        context.text(*token),
                    ));
                }
            }
            _ => {}
        }
    }
    let mut seen = std::collections::HashSet::new();
    for (name, kind, detail) in open.iter().rev().flatten() {
        if seen.insert(*name) {
            completions.push(Completion::new(*name, *kind).detail(*detail));
        }
    }
    if in_function {
        // A function's arguments: `fn name(a: float, b: int)`.
        let mut iter = tokens.iter().peekable();
        while let Some(token) = iter.next() {
            if token.kind == TokenKind::Identifier
                && iter
                    .peek()
                    .is_some_and(|next| next.kind == TokenKind::Colon)
                && seen.insert(context.text(*token))
            {
                completions.push(
                    Completion::new(context.text(*token), CompletionKind::Variable)
                        .detail("argument"),
                );
            }
        }
    } else {
        // Enum options of the declaration's parameters.
        let mut in_enum = false;
        for token in tokens {
            match token.kind {
                TokenKind::Identifier if context.text(*token) == "enum" => in_enum = true,
                TokenKind::RightBrace => in_enum = false,
                TokenKind::Identifier if in_enum && seen.insert(context.text(*token)) => {
                    completions.push(Completion::new(
                        context.text(*token),
                        CompletionKind::EnumMember,
                    ));
                }
                _ => {}
            }
        }
    }
    // Functions of the document.
    let all = &context.tokens;
    for (index, token) in all.iter().enumerate() {
        if token.kind == TokenKind::Keyword(Keyword::Fn)
            && let Some(name) = all.get(index + 1)
            && name.kind == TokenKind::Identifier
            && seen.insert(context.text(*name))
        {
            completions
                .push(Completion::new(context.text(*name), CompletionKind::Function).detail("fn"));
        }
    }
    for builtin in BUILTINS {
        completions.push(
            Completion::new(builtin.name, CompletionKind::Function)
                .detail(builtin.signatures[0].label(builtin.name))
                .documentation(builtin.summary),
        );
    }
    for value in CONTEXT.iter().filter(|value| value.field().is_none()) {
        completions.push(
            Completion::new(value.name, CompletionKind::Variable)
                .detail(type_text(&value.ty))
                .documentation(value.summary),
        );
    }
    let mut scopes = Vec::new();
    for (scope, _) in CONTEXT.iter().filter_map(ContextValue::field) {
        if !scopes.contains(&scope) {
            scopes.push(scope);
            completions.push(Completion::new(scope, CompletionKind::Variable).detail("context"));
        }
    }
    for reducer in REDUCERS {
        let name = reducer.name;
        completions.push(
            Completion::new(name, CompletionKind::Snippet)
                .detail(format!("{name} for i in 0..n {{ ... }}"))
                .documentation(reducer.summary)
                .insert(format!(
                    "{name} for ${{1:i}} in ${{2:0}}..${{3:n}} {{ $0 }}"
                )),
        );
    }
    for keyword in [
        Keyword::Let,
        Keyword::Guard,
        Keyword::If,
        Keyword::Else,
        Keyword::True,
        Keyword::False,
    ] {
        completions.push(Completion::new(keyword.text(), CompletionKind::Keyword));
    }
    completions
}

/// Signature help inside a call's parentheses at `offset`.
pub fn script_signature(source: &str, offset: usize) -> Option<SignatureHelp> {
    let context = Context::new(source, offset);
    let mut depth = 0_usize;
    let mut commas = 0_usize;
    for index in (0..context.before).rev() {
        let token = context.tokens[index];
        match token.kind {
            TokenKind::RightParen | TokenKind::RightBracket => depth += 1,
            TokenKind::LeftBracket => depth = depth.checked_sub(1)?,
            TokenKind::LeftParen if depth > 0 => depth -= 1,
            TokenKind::Comma if depth == 0 => commas += 1,
            TokenKind::LeftParen => {
                let name = context.tokens.get(index.checked_sub(1)?)?;
                if name.kind != TokenKind::Identifier {
                    return None;
                }
                let name = context.text(*name);
                let signatures = if let Some(builtin) = builtin(name) {
                    builtin
                        .signatures
                        .iter()
                        .map(|signature| Signature {
                            label: signature.label(builtin.name),
                            parameters: signature
                                .args
                                .iter()
                                .map(|(arg, ty)| format!("{arg}: {}", ty.text()))
                                .collect(),
                            documentation: Some(builtin.summary.to_string()),
                        })
                        .collect()
                } else {
                    let analysis = analyze_script(source);
                    let (index, function) =
                        analysis.symbols.iter().enumerate().find(|(_, symbol)| {
                            symbol.kind == SymbolKind::Function && symbol.name == name
                        })?;
                    // An argument's detail is its `name: type` in the function's.
                    let parameters = analysis
                        .symbols
                        .iter()
                        .filter(|symbol| {
                            symbol.kind == SymbolKind::Argument && symbol.parent == Some(index)
                        })
                        .map(|argument| argument.detail.clone())
                        .collect();
                    vec![Signature {
                        label: function.detail.clone(),
                        parameters,
                        documentation: function.description.clone(),
                    }]
                };
                return Some(SignatureHelp {
                    signatures,
                    active_parameter: commas,
                });
            }
            TokenKind::LeftBrace | TokenKind::RightBrace | TokenKind::Semicolon => return None,
            _ => {}
        }
    }
    None
}

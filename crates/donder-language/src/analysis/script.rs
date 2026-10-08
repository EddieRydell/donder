//! Script analysis: names resolved over the syntax tree of every declaration
//! that parses, with the checker's binding types; completion and signature
//! help read tokens, so they work while the text does not parse.
use super::{
    Completion, CompletionKind, SemanticToken, Signature, SignatureHelp, Symbol, SymbolKind,
    TokenClass, contains,
};
use crate::compiler::builtins::{BUILTINS, Builtin, CONTEXT, ContextValue, builtin};
use crate::compiler::syntax::ast::*;
use crate::compiler::syntax::lexer::{Keyword, LexMode, Token, TokenKind, lex_with_comments};
use crate::compiler::{Diagnostic, TextSpan};
use donder_runtime_types::{Identifier, Type};

/// What a name refers to.
#[derive(Clone, Copy, Debug)]
pub enum ScriptTarget {
    Symbol(usize),
    Builtin(&'static Builtin),
    Context(&'static ContextValue),
    Reducer(&'static str),
    Method(&'static str),
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
                    value.name, value.ty, value.summary
                )
            }
            ScriptTarget::Reducer(name) => format!(
                "```donder\n{name} for i in start..end {{ body }}\n```\n\n{}",
                reducer_summary(name)
            ),
            ScriptTarget::Method(name) => method_documentation(name).to_string(),
        }
    }
}

fn builtin_documentation(builtin: &Builtin) -> String {
    let signatures = builtin
        .signatures
        .iter()
        .map(|signature| signature_label(builtin.name, signature))
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

fn signature_label(name: &str, signature: &crate::compiler::builtins::Signature) -> String {
    format!(
        "{name}({}) -> {}",
        signature
            .args
            .iter()
            .map(|(arg, ty)| format!("{arg}: {ty}"))
            .collect::<Vec<_>>()
            .join(", "),
        signature.result
    )
}

const REDUCERS: [(&str, &str); 7] = [
    ("max", "The largest value of the body over the range."),
    ("min", "The smallest value of the body over the range."),
    ("sum", "The sum of the body over the range."),
    ("any", "Whether the body holds for some index."),
    ("all", "Whether the body holds for every index."),
    (
        "first",
        "The body at the first index whose guards pass, or `else`.",
    ),
    (
        "last",
        "The body at the last index whose guards pass, or `else`.",
    ),
];

fn reducer_summary(name: &str) -> &'static str {
    REDUCERS
        .iter()
        .find(|(reducer, _)| *reducer == name)
        .map_or("", |(_, summary)| summary)
}

const METHODS: [(&str, &str, &str); 2] = [
    (
        "at",
        "at(time: float) -> color\nat(time: float, pixel: int) -> color",
        "The input at another time, at this pixel or another pixel of this fixture.",
    ),
    (
        "at_global",
        "at_global(time: float, pixel: int) -> color",
        "The input at another time and another pixel of the whole layout.",
    ),
];

fn method_documentation(name: &str) -> String {
    METHODS
        .iter()
        .find(|(method, ..)| *method == name)
        .map_or_else(String::new, |(_, signature, summary)| {
            format!("```donder\n{signature}\n```\n\n{summary}")
        })
}

/// A type as scripts spell it.
pub fn type_text(ty: &Type) -> String {
    match ty {
        Type::Void => "void".into(),
        Type::Int => "int".into(),
        Type::Float => "float".into(),
        Type::Bool => "bool".into(),
        Type::Color => "color".into(),
        Type::Signal => "signal".into(),
        Type::Marks => "marks".into(),
        Type::Curve => "curve".into(),
        Type::Gradient => "gradient".into(),
        Type::Array(item) => format!("array<{}>", type_text(item)),
        Type::Enum(options) => format!(
            "enum {{ {} }}",
            options
                .iter()
                .map(Identifier::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

const TYPES: [&str; 9] = [
    "int", "float", "bool", "color", "curve", "gradient", "marks", "enum", "array",
];

pub fn analyze_script(source: &str) -> ScriptAnalysis {
    let diagnostics = match crate::compiler::compile_script(source) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics,
    };
    let (module, _) = crate::compiler::syntax::parse_partial(source);
    let mut bindings = Vec::new();
    let _ = crate::compiler::check::check_recording(module.clone(), &mut bindings);
    let mut walker = Walker {
        source,
        symbols: Vec::new(),
        references: Vec::new(),
        scopes: Vec::new(),
        functions: Vec::new(),
        options: Vec::new(),
        bindings: &bindings,
        parent: None,
    };
    walker.module(&module);
    let tokens = semantic_tokens(source, &walker.symbols, &walker.references);
    ScriptAnalysis {
        diagnostics,
        symbols: walker.symbols,
        references: walker.references,
        tokens,
    }
}

struct Walker<'a> {
    source: &'a str,
    symbols: Vec<Symbol>,
    references: Vec<ScriptReference>,
    scopes: Vec<(Identifier, usize)>,
    functions: Vec<(Identifier, usize)>,
    /// The current declaration's enum options.
    options: Vec<(Identifier, usize)>,
    bindings: &'a [(TextSpan, Type)],
    parent: Option<usize>,
}

impl Walker<'_> {
    fn text(&self, span: TextSpan) -> &str {
        &self.source[span.start.min(self.source.len())..span.end.min(self.source.len())]
    }

    fn define(
        &mut self,
        name: &Name,
        kind: SymbolKind,
        extent: TextSpan,
        detail: String,
        description: Option<String>,
    ) -> usize {
        self.symbols.push(Symbol {
            name: name.name.as_str().to_string(),
            kind,
            span: name.span,
            extent,
            detail,
            description,
            parent: self.parent,
        });
        self.symbols.len() - 1
    }

    fn refer(&mut self, span: TextSpan, target: ScriptTarget) {
        self.references.push(ScriptReference { span, target });
    }

    fn module(&mut self, module: &Module) {
        for function in &module.functions {
            let args = function
                .args
                .iter()
                .map(|(name, ty)| format!("{}: {}", name.name.as_str(), self.text(ty.span)))
                .collect::<Vec<_>>()
                .join(", ");
            let detail = format!(
                "fn {}({args}) -> {}",
                function.name.name.as_str(),
                self.text(function.result.span)
            );
            let symbol = self.define(
                &function.name,
                SymbolKind::Function,
                function.span,
                detail,
                function.description.clone(),
            );
            self.functions.push((function.name.name.clone(), symbol));
        }
        for (function, (_, symbol)) in module.functions.iter().zip(self.functions.clone()) {
            self.parent = Some(symbol);
            self.options.clear();
            for (name, ty) in &function.args {
                let detail = format!("{}: {}", name.name.as_str(), self.text(ty.span));
                let argument = self.define(name, SymbolKind::Argument, name.span, detail, None);
                self.scopes.push((name.name.clone(), argument));
            }
            self.block(&function.body);
            self.scopes.clear();
        }
        for declaration in &module.declarations {
            self.declaration(declaration);
        }
        self.parent = None;
    }

    fn declaration(&mut self, declaration: &Declaration) {
        self.parent = None;
        let (kind, keyword) = match declaration.kind {
            DeclarationKind::Effect => (SymbolKind::Effect, "effect"),
            DeclarationKind::Operator => (SymbolKind::Operator, "operator"),
        };
        let symbol = self.define(
            &declaration.name,
            kind,
            declaration.span,
            format!("{keyword} {}", declaration.name.name.as_str()),
            declaration.description.clone(),
        );
        self.parent = Some(symbol);
        self.options.clear();
        for input in &declaration.inputs {
            let input_symbol = self.define(
                input,
                SymbolKind::Input,
                input.span,
                format!("input {}", input.name.as_str()),
                None,
            );
            self.scopes.push((input.name.clone(), input_symbol));
        }
        for param in &declaration.params {
            let end = param
                .default
                .as_ref()
                .map(|literal| literal.span)
                .or(param.range.as_ref().map(|(_, max)| max.span))
                .unwrap_or(param.ty.span);
            let detail = format!(
                "param {}",
                self.text(TextSpan {
                    start: param.name.span.start,
                    end: end.end,
                })
            );
            let param_symbol = self.define(
                &param.name,
                SymbolKind::Param,
                param.name.span.to(end),
                detail,
                param.description.clone(),
            );
            self.scopes.push((param.name.name.clone(), param_symbol));
            let parent = self.parent.replace(param_symbol);
            let mut options = Vec::new();
            if let TypeKind::Enum(names) = &param.ty.kind {
                for option in names {
                    let detail = format!(
                        "{} (an option of `{}`)",
                        option.name.as_str(),
                        param.name.name.as_str()
                    );
                    let option_symbol =
                        self.define(option, SymbolKind::EnumOption, option.span, detail, None);
                    options.push((option.name.clone(), option_symbol));
                }
            }
            self.parent = parent;
            if let Some(Literal {
                kind: LiteralKind::Name(name),
                span,
            }) = &param.default
                && let Some((_, option)) = options.iter().find(|(option, _)| option == name)
            {
                self.refer(*span, ScriptTarget::Symbol(*option));
            }
            self.options.extend(options);
        }
        if let Some(sample) = &declaration.sample {
            self.block(sample);
        }
        self.scopes.clear();
    }

    fn block(&mut self, block: &Block) {
        let depth = self.scopes.len();
        for statement in &block.statements {
            match statement {
                Statement::Let { name, ty, value } => {
                    self.expr(value);
                    let ty = ty
                        .as_ref()
                        .map(|ty| self.text(ty.span).to_string())
                        .or_else(|| {
                            self.bindings
                                .iter()
                                .find(|(span, _)| *span == name.span)
                                .map(|(_, ty)| type_text(ty))
                        });
                    let detail = match ty {
                        Some(ty) => format!("let {}: {ty}", name.name.as_str()),
                        None => format!("let {}", name.name.as_str()),
                    };
                    let symbol = self.define(name, SymbolKind::Let, name.span, detail, None);
                    self.scopes.push((name.name.clone(), symbol));
                }
                Statement::Guard {
                    condition,
                    otherwise,
                    ..
                } => {
                    self.expr(condition);
                    if let Some(otherwise) = otherwise {
                        self.expr(otherwise);
                    }
                }
            }
        }
        self.expr(&block.result);
        self.scopes.truncate(depth);
    }

    fn name(&mut self, name: &Identifier, span: TextSpan) {
        if let Some((_, symbol)) = self.scopes.iter().rev().find(|(bound, _)| bound == name) {
            self.refer(span, ScriptTarget::Symbol(*symbol));
        } else if let Some((_, option)) = self.options.iter().find(|(option, _)| option == name) {
            self.refer(span, ScriptTarget::Symbol(*option));
        } else if let Some(value) = CONTEXT.iter().find(|value| value.name == name.as_str()) {
            self.refer(span, ScriptTarget::Context(value));
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Literal(_) => {}
            ExprKind::Name(name) => self.name(name, expr.span),
            ExprKind::Field(target, field) => {
                if let ExprKind::Name(scope) = &target.kind
                    && let Some(value) = CONTEXT.iter().find(|value| {
                        value.name.split_once('.') == Some((scope.as_str(), field.name.as_str()))
                    })
                {
                    self.refer(target.span, ScriptTarget::Context(value));
                    self.refer(field.span, ScriptTarget::Context(value));
                } else {
                    self.expr(target);
                }
            }
            ExprKind::Call(name, args) => {
                if let Some((_, function)) = self
                    .functions
                    .iter()
                    .find(|(function, _)| *function == name.name)
                {
                    self.refer(name.span, ScriptTarget::Symbol(*function));
                } else if let Some(builtin) = builtin(name.name.as_str()) {
                    self.refer(name.span, ScriptTarget::Builtin(builtin));
                }
                for arg in args {
                    self.expr(arg);
                }
            }
            ExprKind::Method(target, method, args) => {
                self.expr(target);
                if let Some((name, ..)) = METHODS
                    .iter()
                    .find(|(name, ..)| *name == method.name.as_str())
                {
                    self.refer(method.span, ScriptTarget::Method(name));
                }
                for arg in args {
                    self.expr(arg);
                }
            }
            ExprKind::Index(target, index) => {
                self.expr(target);
                self.expr(index);
            }
            ExprKind::Unary(_, operand) => self.expr(operand),
            ExprKind::Binary(_, left, right) => {
                self.expr(left);
                self.expr(right);
            }
            ExprKind::If(condition, then, otherwise) => {
                self.expr(condition);
                self.block(then);
                self.expr(otherwise);
            }
            ExprKind::Array(items) => {
                for item in items {
                    self.expr(item);
                }
            }
            ExprKind::Reduce(reduction) => {
                let (name, _) = REDUCERS[reducer_index(reduction.reducer)];
                self.refer(
                    TextSpan {
                        start: expr.span.start,
                        end: expr.span.start + name.len(),
                    },
                    ScriptTarget::Reducer(name),
                );
                self.expr(&reduction.start);
                self.expr(&reduction.end);
                let index = self.define(
                    &reduction.index,
                    SymbolKind::Index,
                    reduction.index.span,
                    format!("{}: int", reduction.index.name.as_str()),
                    None,
                );
                self.scopes.push((reduction.index.name.clone(), index));
                self.block(&reduction.body);
                self.scopes.pop();
                if let Some(otherwise) = &reduction.otherwise {
                    self.block(otherwise);
                }
            }
            ExprKind::Block(block) => self.block(block),
        }
    }
}

fn reducer_index(reducer: ReducerKind) -> usize {
    match reducer {
        ReducerKind::Max => 0,
        ReducerKind::Min => 1,
        ReducerKind::Sum => 2,
        ReducerKind::Any => 3,
        ReducerKind::All => 4,
        ReducerKind::First => 5,
        ReducerKind::Last => 6,
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
                } else if TYPES.contains(&text) {
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
                let (owner, field) = value.name.split_once('.')?;
                (owner == scope).then(|| {
                    Completion::new(field, CompletionKind::Field)
                        .detail(value.ty)
                        .documentation(value.summary)
                })
            })
            .collect::<Vec<_>>();
        if !fields.is_empty() {
            return fields;
        }
        return METHODS
            .iter()
            .map(|(name, signature, summary)| {
                Completion::new(*name, CompletionKind::Function)
                    .detail(*signature)
                    .documentation(*summary)
            })
            .collect();
    }
    // A type after `:` in a parameter, a `let` or an argument.
    if previous.is_some_and(|token| token.kind == TokenKind::Colon) {
        return TYPES
            .iter()
            .map(|ty| Completion::new(*ty, CompletionKind::Type))
            .collect();
    }
    let Some((keyword, tokens)) = context.enclosing() else {
        return ["effect", "operator", "fn"]
            .iter()
            .map(|word| Completion::new(*word, CompletionKind::Keyword))
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
                .detail(signature_label(builtin.name, &builtin.signatures[0]))
                .documentation(builtin.summary),
        );
    }
    for value in CONTEXT.iter().filter(|value| !value.name.contains('.')) {
        completions.push(
            Completion::new(value.name, CompletionKind::Variable)
                .detail(value.ty)
                .documentation(value.summary),
        );
    }
    for scope in ["pixel", "target"] {
        completions.push(Completion::new(scope, CompletionKind::Variable).detail("context"));
    }
    for (reducer, summary) in REDUCERS {
        completions.push(
            Completion::new(reducer, CompletionKind::Snippet)
                .detail(format!("{reducer} for i in 0..n {{ ... }}"))
                .documentation(summary)
                .insert(format!(
                    "{reducer} for ${{1:i}} in ${{2:0}}..${{3:n}} {{ $0 }}"
                )),
        );
    }
    for word in ["let", "guard", "if", "else", "true", "false"] {
        completions.push(Completion::new(word, CompletionKind::Keyword));
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
                            label: signature_label(builtin.name, signature),
                            parameters: signature
                                .args
                                .iter()
                                .map(|(arg, ty)| format!("{arg}: {ty}"))
                                .collect(),
                            documentation: Some(builtin.summary.to_string()),
                        })
                        .collect()
                } else {
                    let analysis = analyze_script(source);
                    let function = analysis.symbols.iter().find(|symbol| {
                        symbol.kind == SymbolKind::Function && symbol.name == name
                    })?;
                    let parameters = function
                        .detail
                        .split_once('(')
                        .and_then(|(_, rest)| rest.rsplit_once(')'))
                        .map(|(args, _)| {
                            args.split(", ")
                                .filter(|arg| !arg.is_empty())
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
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

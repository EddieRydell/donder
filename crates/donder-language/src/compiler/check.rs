//! Name resolution and type checking. A declaration is lowered straight into
//! its definition IR; diagnostics point at the syntax that produced a node.
//! A check can also record every name it declares and resolves, which is all
//! editor analysis knows of a script's names.
use super::Diagnostic;
use super::builtins::{
    self, AT, Fit, Lowering, NAMED_TYPES, Read, builtin, context_field, context_value, describe,
    is_context_scope, type_text,
};
use super::declarations::{OperatorInputDecl, ParamDecl, ParamRange};
use super::ir::interval::{Bounds, interval};
use super::ir::{
    Binary, Context, Graph, LoopSet, Node, Op, Param, Reducer, STATE, Unary, check_stencil, reads,
    source_only,
};
use super::syntax::ast::*;
use super::syntax::lexer::{Keyword, TextSpan};
use crate::analysis::{ScriptReference, ScriptTarget, Symbol, SymbolKind};
use crate::names::{NameError, NameKind, name_from_text, pascal_from_snake};
use donder_runtime_types::Color;
use donder_runtime_types::bytecode::{Edges, MAX_ITERATIONS, STRIP, SignalPixel};
use donder_runtime_types::{Identifier, Type, Value};
use std::rc::Rc;

/// One checked effect or operator.
#[derive(Clone, Debug)]
pub(crate) struct Definition {
    pub(crate) kind: DeclarationKind,
    pub(crate) name: Identifier,
    pub(crate) description: Option<String>,
    pub(crate) params: Vec<ParamDecl>,
    pub(crate) inputs: Vec<OperatorInputDecl>,
    pub(crate) graph: Graph,
    /// The current pixel's color.
    pub(crate) root: Node,
    /// Iteration counts whose bound depends on the lengths of parameters.
    pub(crate) length_bounds: Vec<Node>,
    /// The declaration's name in its source.
    pub(crate) span: TextSpan,
    pub(crate) fingerprint: u64,
}

/// Every name a check declared, and each name it resolved.
#[derive(Debug, Default)]
pub(crate) struct Names {
    pub(crate) symbols: Vec<Symbol>,
    pub(crate) references: Vec<ScriptReference>,
}

impl Names {
    fn define(&mut self, symbol: Symbol) -> usize {
        self.symbols.push(symbol);
        self.symbols.len() - 1
    }
}

/// A name that declares a symbol: what it is and where it is declared.
struct Declared<'a> {
    name: &'a super::syntax::ast::Name,
    kind: SymbolKind,
    extent: TextSpan,
    detail: String,
    description: Option<String>,
    ty: Option<Type>,
    parent: Option<usize>,
}

impl Declared<'_> {
    fn symbol(self) -> Symbol {
        Symbol {
            name: self.name.name.as_str().to_string(),
            kind: self.kind,
            span: self.name.span,
            extent: self.extent,
            detail: self.detail,
            description: self.description,
            ty: self.ty,
            parent: self.parent,
        }
    }
}

pub(crate) fn check(module: Module, source: &str) -> Result<Vec<Definition>, Vec<Diagnostic>> {
    check_recording(module, source, false).0
}

/// Check `module`, the parse of `source`. When `recording`, also record its
/// names, even of declarations that fail.
pub(crate) fn check_recording(
    module: Module,
    source: &str,
    recording: bool,
) -> (Result<Vec<Definition>, Vec<Diagnostic>>, Names) {
    let text = |span: TextSpan| &source[span.start.min(source.len())..span.end.min(source.len())];
    let mut names = Names::default();
    let function_symbols = module
        .functions
        .iter()
        .map(|function| {
            recording.then(|| {
                let args = function
                    .args
                    .iter()
                    .map(|(name, ty)| format!("{}: {}", name.name.as_str(), text(ty.span)))
                    .collect::<Vec<_>>()
                    .join(", ");
                names.define(
                    Declared {
                        name: &function.name,
                        kind: SymbolKind::Function,
                        extent: function.span,
                        detail: format!(
                            "{} {}({args}) -> {}",
                            Keyword::Fn.text(),
                            function.name.name.as_str(),
                            text(function.result.span)
                        ),
                        description: function.description.clone(),
                        ty: None,
                        parent: None,
                    }
                    .symbol(),
                )
            })
        })
        .collect::<Rc<[_]>>();
    let functions: Rc<[Function]> = module.functions.into();
    let mut diagnostics =
        check_functions(&functions, &function_symbols, source, recording, &mut names);
    let mut definitions = Vec::new();
    let mut declaration_diagnostics = Vec::new();
    for declaration in module.declarations {
        let checked = Checker::declaration(
            declaration,
            functions.clone(),
            function_symbols.clone(),
            source,
            recording,
            &mut names,
        );
        match checked {
            Ok(definition) => definitions.push(definition),
            Err(errors) => declaration_diagnostics.extend(errors),
        }
    }
    // Errors in functions make their callers' errors moot.
    if diagnostics.is_empty() {
        diagnostics = declaration_diagnostics;
    }
    // A function's error repeats at every call that inlines it.
    let mut unique = Vec::new();
    for diagnostic in diagnostics {
        if !unique.contains(&diagnostic) {
            unique.push(diagnostic);
        }
    }
    let checked = if unique.is_empty() {
        Ok(definitions)
    } else {
        Err(unique)
    };
    (checked, names)
}

/// Check every function's declaration and, once on its own, its body, so a
/// function nobody calls is still valid. Iteration bounds that depend on
/// arguments are proven where the function is inlined.
fn check_functions(
    functions: &Rc<[Function]>,
    function_symbols: &Rc<[Option<usize>]>,
    source: &str,
    recording: bool,
    names: &mut Names,
) -> Vec<Diagnostic> {
    let text = |span: TextSpan| &source[span.start.min(source.len())..span.end.min(source.len())];
    let mut diagnostics = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        let name = function.name.name.as_str();
        diagnostics.extend(name_diagnostic(NameKind::Function, &function.name));
        if functions[..index]
            .iter()
            .any(|other| other.name.name == function.name.name)
        {
            diagnostics.push(Diagnostic::new(
                function.name.span,
                format!("`{name}` is declared twice"),
            ));
        }
        let mut types = Vec::new();
        let mut arg_symbols = Vec::new();
        for (position, (arg, ty)) in function.args.iter().enumerate() {
            diagnostics.extend(name_diagnostic(NameKind::Value, arg));
            if function.args[..position]
                .iter()
                .any(|(other, _)| other.name == arg.name)
            {
                diagnostics.push(Diagnostic::new(
                    arg.span,
                    format!("`{}` is declared twice", arg.name.as_str()),
                ));
            }
            let resolved = resolve_type(ty);
            arg_symbols.push(recording.then(|| {
                names.define(
                    Declared {
                        name: arg,
                        kind: SymbolKind::Argument,
                        extent: arg.span,
                        detail: format!("{}: {}", arg.name.as_str(), text(ty.span)),
                        description: None,
                        ty: resolved.as_ref().ok().cloned(),
                        parent: function_symbols[index],
                    }
                    .symbol(),
                )
            }));
            match resolved {
                Ok(ty) => types.push(ty),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
        }
        let result =
            resolve_type(&function.result).map_err(|diagnostic| diagnostics.push(diagnostic));
        let (Ok(result), true) = (result, types.len() == function.args.len()) else {
            continue;
        };
        let graph = Graph::new(
            types
                .iter()
                .map(|ty| Param {
                    ty: ty.clone(),
                    domain: super::ir::Domain::PARAM,
                })
                .collect(),
            0,
        );
        let mut checker = Checker {
            graph,
            params: Vec::new(),
            inputs: Vec::new(),
            scopes: Vec::new(),
            length_bounds: Vec::new(),
            diagnostics: Vec::new(),
            functions: functions.clone(),
            frames: vec![index],
            standalone: true,
            recording,
            names: std::mem::take(names),
            parent: function_symbols[index],
            function_symbols: function_symbols.clone(),
            options: Vec::new(),
        };
        for (position, (arg, _)) in function.args.iter().enumerate() {
            let node = checker.graph.add(Op::Param(position as u32));
            checker
                .scopes
                .push((arg.name.clone(), node, arg_symbols[position]));
        }
        if let Some(value) = checker
            .tail_block(&function.body, Mode::Value)
            .and_then(|outcome| outcome.value)
        {
            checker.require(value, &result, function.body.result.span);
        }
        *names = std::mem::take(&mut checker.names);
        diagnostics.extend(checker.diagnostics);
    }
    diagnostics
}

/// The diagnostic for a name that breaks its kind's rule. A name in the wrong
/// case carries the right spelling as its fix.
fn name_diagnostic(kind: NameKind, name: &Name) -> Option<Diagnostic> {
    let text = name.name.as_str();
    let error = kind.check(text).err()?;
    let diagnostic = Diagnostic::new(name.span, error.message(text));
    Some(match error {
        NameError::NotPascalCase => diagnostic.with_fix(pascal_from_snake(text)),
        NameError::NotSnakeCase => diagnostic.with_fix(name_from_text(text, text)),
        NameError::Keyword | NameError::Context | NameError::Builtin => diagnostic,
    })
}

/// A block's result where guards may produce nothing: whether it holds a value,
/// and the value, which is meaningless when it does not. A guard's skipped
/// branch has no value at all, so a consumer that tests `valid` (a reduction
/// filter) reads the guarded value without a redundant choice.
#[derive(Clone, Copy)]
struct Outcome {
    valid: Node,
    value: Option<Node>,
}

/// Whether guards without `else` may skip a block.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Value,
    Tail,
}

/// A checked operand, or an unbound name that may be an enum option.
enum Operand {
    Node(Node),
    Option(Identifier, TextSpan),
}

/// A declared value of a declaration: its declaration and symbol.
struct Member<T> {
    value: T,
    symbol: Option<usize>,
}

pub(crate) struct Checker {
    pub(crate) graph: Graph,
    params: Vec<Member<ParamDecl>>,
    inputs: Vec<Member<Identifier>>,
    /// Names in scope, innermost last, with their symbols.
    scopes: Vec<(Identifier, Node, Option<usize>)>,
    length_bounds: Vec<Node>,
    diagnostics: Vec<Diagnostic>,
    functions: Rc<[Function]>,
    function_symbols: Rc<[Option<usize>]>,
    /// Functions being inlined, innermost last. Their bodies see only their
    /// arguments and context values.
    frames: Vec<usize>,
    /// Checking a function body on its own, with arguments of unknown value.
    standalone: bool,
    recording: bool,
    /// The names recorded so far; inlined function bodies are recorded once,
    /// when checked on their own.
    names: Names,
    /// The symbol whose body is checked.
    parent: Option<usize>,
    /// The enum options of the declaration's parameters, with their symbols.
    options: Vec<(Identifier, usize)>,
}

type Checked<T> = Option<T>;

impl Checker {
    fn declaration(
        declaration: Declaration,
        functions: Rc<[Function]>,
        function_symbols: Rc<[Option<usize>]>,
        source: &str,
        recording: bool,
        names: &mut Names,
    ) -> Result<Definition, Vec<Diagnostic>> {
        let text =
            |span: TextSpan| &source[span.start.min(source.len())..span.end.min(source.len())];
        let mut diagnostics = Vec::new();
        let mut params = Vec::new();
        let mut options = Vec::new();
        let (kind, keyword) = match declaration.kind {
            DeclarationKind::Effect => (SymbolKind::Effect, Keyword::Effect),
            DeclarationKind::Operator => (SymbolKind::Operator, Keyword::Operator),
        };
        let symbol = recording.then(|| {
            names.define(
                Declared {
                    name: &declaration.name,
                    kind,
                    extent: declaration.span,
                    detail: format!("{} {}", keyword.text(), declaration.name.name.as_str()),
                    description: declaration.description.clone(),
                    ty: None,
                    parent: None,
                }
                .symbol(),
            )
        });
        let inputs = declaration
            .inputs
            .iter()
            .map(|input| Member {
                value: input.name.clone(),
                symbol: recording.then(|| {
                    names.define(
                        Declared {
                            name: input,
                            kind: SymbolKind::Input,
                            extent: input.span,
                            detail: format!("input {}", input.name.as_str()),
                            description: None,
                            ty: None,
                            parent: symbol,
                        }
                        .symbol(),
                    )
                }),
            })
            .collect::<Vec<_>>();
        let mut names_seen: Vec<&Identifier> = Vec::new();
        diagnostics.extend(name_diagnostic(NameKind::Definition, &declaration.name));
        for input in &declaration.inputs {
            diagnostics.extend(name_diagnostic(NameKind::Member, input));
        }
        for param in &declaration.params {
            diagnostics.extend(name_diagnostic(NameKind::Member, &param.name));
            let param_symbol = recording.then(|| {
                let end = param
                    .default
                    .as_ref()
                    .map(|literal| literal.span)
                    .or(param.range.as_ref().map(|(_, max)| max.span))
                    .unwrap_or(param.ty.span);
                names.define(
                    Declared {
                        name: &param.name,
                        kind: SymbolKind::Param,
                        extent: param.name.span.to(end),
                        detail: format!(
                            "param {}",
                            text(TextSpan {
                                start: param.name.span.start,
                                end: end.end,
                            })
                        ),
                        description: param.description.clone(),
                        ty: resolve_type(&param.ty).ok(),
                        parent: symbol,
                    }
                    .symbol(),
                )
            });
            if let (TypeKind::Enum(choices), true) = (&param.ty.kind, recording) {
                let first = options.len();
                for option in choices {
                    let option_symbol = names.define(
                        Declared {
                            name: option,
                            kind: SymbolKind::EnumOption,
                            extent: option.span,
                            detail: format!(
                                "{} (an option of `{}`)",
                                option.name.as_str(),
                                param.name.name.as_str()
                            ),
                            description: None,
                            ty: None,
                            parent: param_symbol,
                        }
                        .symbol(),
                    );
                    options.push((option.name.clone(), option_symbol));
                }
                if let Some(Literal {
                    kind: LiteralKind::Name(name),
                    span,
                }) = &param.default
                    && let Some((_, option)) =
                        options[first..].iter().find(|(option, _)| option == name)
                {
                    names.references.push(ScriptReference {
                        span: *span,
                        target: ScriptTarget::Symbol(*option),
                    });
                }
            }
            match check_param(param) {
                Ok(value) => params.push(Member {
                    value,
                    symbol: param_symbol,
                }),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
        }
        for name in declaration
            .inputs
            .iter()
            .chain(declaration.params.iter().map(|param| &param.name))
        {
            if names_seen.contains(&&name.name) {
                diagnostics.push(Diagnostic::new(
                    name.span,
                    format!("`{}` is declared twice", name.name.as_str()),
                ));
            }
            names_seen.push(&name.name);
        }
        match declaration.kind {
            DeclarationKind::Effect if !declaration.inputs.is_empty() => {
                diagnostics.push(Diagnostic::new(
                    declaration.inputs[0].span,
                    "effects have no signal inputs; declare them on an operator",
                ));
            }
            DeclarationKind::Operator if declaration.inputs.is_empty() => {
                diagnostics.push(Diagnostic::new(
                    declaration.name.span,
                    "an operator declares at least one `input`",
                ));
            }
            _ => {}
        }
        let Some(sample) = &declaration.sample else {
            diagnostics.push(Diagnostic::new(
                declaration.name.span,
                "a declaration needs a `sample` block",
            ));
            return Err(diagnostics);
        };
        // A declaration with errors is still checked for the names it uses.
        let failed = !diagnostics.is_empty();
        if failed && !recording {
            return Err(diagnostics);
        }
        let graph = Graph::new(
            params
                .iter()
                .map(|param| Param {
                    ty: param.value.ty.clone(),
                    domain: super::ir::Domain::PARAM,
                })
                .collect(),
            declaration.inputs.len() as u32,
        );
        let mut checker = Self {
            graph,
            params,
            inputs,
            scopes: Vec::new(),
            length_bounds: Vec::new(),
            diagnostics,
            functions,
            function_symbols,
            frames: Vec::new(),
            standalone: false,
            recording,
            names: std::mem::take(names),
            parent: symbol,
            options,
        };
        let declared = checker.diagnostics.len();
        let root = checker.tail_block(sample, Mode::Tail).and_then(|outcome| {
            let value = outcome.value?;
            checker.require(value, &Type::Color, sample.result.span)?;
            // A sample that produces nothing is black.
            let black = checker.graph.color(Color::BLACK);
            Some(checker.graph.select(outcome.valid, value, black))
        });
        *names = std::mem::take(&mut checker.names);
        if failed {
            checker.diagnostics.truncate(declared);
        }
        match root {
            Some(root) if checker.diagnostics.is_empty() => Ok(Definition {
                kind: declaration.kind,
                description: declaration.description,
                span: declaration.name.span,
                fingerprint: checker.graph.fingerprint(root),
                name: declaration.name.name,
                params: checker
                    .params
                    .into_iter()
                    .map(|param| param.value)
                    .collect(),
                inputs: declaration
                    .inputs
                    .into_iter()
                    .map(|input| OperatorInputDecl { name: input.name })
                    .collect(),
                graph: checker.graph,
                root,
                length_bounds: checker.length_bounds,
            }),
            _ => Err(checker.diagnostics),
        }
    }

    fn error(&mut self, span: TextSpan, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::new(span, message));
    }

    /// Whether names are recorded here: in the checked body itself, not in a
    /// function body inlined into it.
    fn records(&self) -> bool {
        self.recording && self.frames.len() == usize::from(self.standalone)
    }

    fn refer(&mut self, span: TextSpan, target: ScriptTarget) {
        if self.records() {
            self.names.references.push(ScriptReference { span, target });
        }
    }

    fn refer_symbol(&mut self, span: TextSpan, symbol: Option<usize>) {
        if let Some(symbol) = symbol {
            self.refer(span, ScriptTarget::Symbol(symbol));
        }
    }

    /// A value the checked body declares, named at `name`, of type `ty`.
    fn define(&mut self, name: &Name, kind: SymbolKind, ty: &Type) -> Option<usize> {
        let detail = match kind {
            SymbolKind::Let => format!("let {}: {}", name.name.as_str(), type_text(ty)),
            _ => format!("{}: {}", name.name.as_str(), type_text(ty)),
        };
        self.records().then(|| {
            self.names.define(
                Declared {
                    name,
                    kind,
                    extent: name.span,
                    detail,
                    description: None,
                    ty: Some(ty.clone()),
                    parent: self.parent,
                }
                .symbol(),
            )
        })
    }

    /// A value bound by name in the scope of the block being checked.
    fn bind(&mut self, name: &Name, kind: SymbolKind, node: Node) {
        let ty = self.ty(node).clone();
        let symbol = self.define(name, kind, &ty);
        self.scopes.push((name.name.clone(), node, symbol));
    }

    fn ty(&self, node: Node) -> &Type {
        self.graph.ty(node)
    }

    fn require(&mut self, node: Node, expected: &Type, span: TextSpan) -> Checked<Node> {
        match (expected, self.ty(node)) {
            (expected, actual) if expected == actual => Some(node),
            (Type::Float, Type::Int) => Some(self.graph.unary(Unary::IntToFloat, node)),
            (Type::Enum(options), Type::Enum(values))
                if values.iter().all(|value| options.contains(value)) =>
            {
                Some(node)
            }
            (expected, actual) => {
                let message = format!(
                    "expected {}, found {}",
                    describe(expected),
                    describe(actual)
                );
                self.error(span, message);
                None
            }
        }
    }

    fn float(&mut self, expr: &Expr) -> Checked<Node> {
        let node = self.value(expr)?;
        self.require(node, &Type::Float, expr.span)
    }

    fn int(&mut self, expr: &Expr) -> Checked<Node> {
        let node = self.value(expr)?;
        self.require(node, &Type::Int, expr.span)
    }

    fn boolean(&mut self, expr: &Expr) -> Checked<Node> {
        let node = self.value(expr)?;
        self.require(node, &Type::Bool, expr.span)
    }

    fn typed(&mut self, expr: &Expr, expected: &Type) -> Checked<Node> {
        let node = self.value(expr)?;
        self.require(node, expected, expr.span)
    }

    /// A checked block. In `Tail` mode, guards without `else` may skip it.
    fn tail_block(&mut self, block: &Block, mode: Mode) -> Checked<Outcome> {
        let depth = self.scopes.len();
        let mut guards = Vec::new();
        let mut failed = false;
        for statement in &block.statements {
            match statement {
                Statement::Let { name, ty, value } => {
                    if let Some(diagnostic) = name_diagnostic(NameKind::Value, name) {
                        self.diagnostics.push(diagnostic);
                        failed = true;
                        continue;
                    }
                    let node = match ty {
                        Some(ty) => match resolve_type(ty) {
                            Ok(ty) => self.typed(value, &ty),
                            Err(diagnostic) => {
                                self.diagnostics.push(diagnostic);
                                None
                            }
                        },
                        None => self.value(value),
                    };
                    match node {
                        Some(node) => self.bind(name, SymbolKind::Let, node),
                        None => failed = true,
                    }
                }
                Statement::Guard {
                    condition,
                    otherwise,
                    span,
                } => {
                    let condition = self.boolean(condition);
                    let otherwise = match otherwise {
                        Some(otherwise) => self.tail(otherwise, mode).map(Some),
                        None if mode == Mode::Value => {
                            self.error(
                                *span,
                                "a guard without `else` can only skip a sample block or a reduction body",
                            );
                            None
                        }
                        None => Some(None),
                    };
                    match (condition, otherwise) {
                        (Some(condition), Some(otherwise)) => {
                            guards.push((condition, otherwise, *span))
                        }
                        _ => failed = true,
                    }
                }
            }
        }
        let result = if failed {
            None
        } else {
            self.tail(&block.result, mode)
        };
        self.scopes.truncate(depth);
        let mut result = result?;
        for (condition, otherwise, span) in guards.into_iter().rev() {
            let otherwise = match otherwise {
                Some(otherwise) => otherwise,
                None => Outcome {
                    valid: self.graph.bool(false),
                    value: None,
                },
            };
            result = self.choose(condition, result, otherwise, span)?;
        }
        Some(result)
    }

    fn choose(
        &mut self,
        condition: Node,
        yes: Outcome,
        no: Outcome,
        span: TextSpan,
    ) -> Checked<Outcome> {
        let value = match (yes.value, no.value) {
            (Some(yes), Some(no)) => {
                let (yes, no) = self.unify(yes, no, span)?;
                Some(self.graph.select(condition, yes, no))
            }
            (value, None) | (None, value) => value,
        };
        Some(Outcome {
            valid: self.graph.select(condition, yes.valid, no.valid),
            value,
        })
    }

    /// An expression in a position whose block may be skipped.
    fn tail(&mut self, expr: &Expr, mode: Mode) -> Checked<Outcome> {
        match &expr.kind {
            ExprKind::Block(block) => self.tail_block(block, mode),
            ExprKind::If(condition, yes, no) => {
                let condition = self.boolean(condition);
                let yes = self.tail_block(yes, mode);
                let no = self.tail(no, mode);
                let (condition, yes, no) = (condition?, yes?, no?);
                self.choose(condition, yes, no, expr.span)
            }
            _ => {
                let value = self.value(expr)?;
                let valid = self.graph.bool(true);
                Some(Outcome {
                    valid,
                    value: Some(value),
                })
            }
        }
    }

    /// Two values of one type: equal types, or an int widened to float.
    fn unify(&mut self, a: Node, b: Node, span: TextSpan) -> Checked<(Node, Node)> {
        match (self.ty(a).clone(), self.ty(b).clone()) {
            (left, right) if left == right => Some((a, b)),
            (Type::Int, Type::Float) => Some((self.graph.unary(Unary::IntToFloat, a), b)),
            (Type::Float, Type::Int) => Some((a, self.graph.unary(Unary::IntToFloat, b))),
            (left, right) => {
                self.error(
                    span,
                    format!(
                        "the branches produce {} and {}",
                        describe(&left),
                        describe(&right)
                    ),
                );
                None
            }
        }
    }

    fn value(&mut self, expr: &Expr) -> Checked<Node> {
        match self.operand(expr)? {
            Operand::Node(node) => Some(node),
            Operand::Option(name, span) => {
                let hint = if self.frames.is_empty() {
                    ""
                } else {
                    "; a function sees only its arguments and context values"
                };
                self.error(span, format!("unknown name `{}`{hint}", name.as_str()));
                None
            }
        }
    }

    fn operand(&mut self, expr: &Expr) -> Checked<Operand> {
        let node = match &expr.kind {
            ExprKind::Literal(literal) => self.literal(literal, expr.span)?,
            ExprKind::Name(name) => return self.name(name, expr.span),
            ExprKind::Field(target, field) => self.field(target, field)?,
            ExprKind::Call(name, args) => self.call(name, args, expr.span)?,
            ExprKind::Method(target, method, args) => {
                self.method(target, method, args, expr.span)?
            }
            ExprKind::Index(target, index) => self.index(target, index)?,
            ExprKind::Unary(op, operand) => {
                let value = self.value(operand)?;
                match (op, self.ty(value)) {
                    (UnaryOp::Negate, Type::Int) => self.graph.unary(Unary::IntNegate, value),
                    (UnaryOp::Negate, Type::Float) => self.graph.unary(Unary::Negate, value),
                    (UnaryOp::Not, Type::Bool) => self.graph.unary(Unary::Not, value),
                    (UnaryOp::Negate, ty) => {
                        let message = format!("`-` needs a number, found {}", describe(ty));
                        self.error(expr.span, message);
                        return None;
                    }
                    (UnaryOp::Not, ty) => {
                        let message = format!("`!` needs a bool, found {}", describe(ty));
                        self.error(expr.span, message);
                        return None;
                    }
                }
            }
            ExprKind::Binary(op, left, right) => self.binary(*op, left, right, expr.span)?,
            ExprKind::Array(items) => self.array(items, expr.span)?,
            ExprKind::If(..) | ExprKind::Block(_) => {
                let outcome = self.tail(expr, Mode::Value)?;
                outcome.value?
            }
            ExprKind::Reduce(reduction) => self.reduction(reduction, expr.span)?,
            ExprKind::Scan(scan) => self.scan(scan)?,
        };
        Some(Operand::Node(node))
    }

    fn literal(&mut self, literal: &LiteralKind, span: TextSpan) -> Checked<Node> {
        Some(match literal {
            LiteralKind::Int(value) => match i32::try_from(*value) {
                Ok(value) => self.graph.int(value),
                Err(_) => {
                    self.error(span, "integer literal is out of range");
                    return None;
                }
            },
            LiteralKind::Float(value) => self.graph.float(*value),
            LiteralKind::Bool(value) => self.graph.bool(*value),
            LiteralKind::Color(value) => self.graph.color(*value),
            LiteralKind::Name(name) => {
                return self.value(&Expr {
                    kind: ExprKind::Name(name.clone()),
                    span,
                });
            }
        })
    }

    fn name(&mut self, name: &Identifier, span: TextSpan) -> Checked<Operand> {
        if let Some(&(_, node, symbol)) = self.scopes.iter().rev().find(|(bound, ..)| bound == name)
        {
            self.refer_symbol(span, symbol);
            return Some(Operand::Node(node));
        }
        let in_function = !self.frames.is_empty();
        if let Some(index) = self
            .params
            .iter()
            .position(|param| &param.value.name == name)
            && !in_function
        {
            self.refer_symbol(span, self.params[index].symbol);
            return Some(Operand::Node(self.graph.add(Op::Param(index as u32))));
        }
        if let Some(input) = self.inputs.iter().position(|input| &input.value == name)
            && !in_function
        {
            self.refer_symbol(span, self.inputs[input].symbol);
            // A signal used as a color samples the current pixel.
            return Some(Operand::Node(self.graph.add(Op::Sample {
                input: input as u32,
                pixel: SignalPixel::Current,
            })));
        }
        if let Some(value) = context_value(name.as_str()) {
            self.refer(span, ScriptTarget::Context(value));
            return Some(Operand::Node(self.read(value.read)));
        }
        if is_context_scope(name.as_str()) {
            let example = builtins::CONTEXT
                .iter()
                .find(|value| {
                    value
                        .field()
                        .is_some_and(|(scope, _)| scope == name.as_str())
                })
                .map_or("", |value| value.name);
            self.error(
                span,
                format!("use a field of `{}`, like `{example}`", name.as_str()),
            );
            return None;
        }
        Some(Operand::Option(name.clone(), span))
    }

    /// The node a context value reads.
    fn read(&mut self, read: Read) -> Node {
        match read {
            Read::Context(context) => self.graph.add(Op::Context(context)),
            Read::Previous => self.graph.add(Op::Sample {
                input: STATE,
                pixel: SignalPixel::Current,
            }),
            Read::Constant(value) => self.graph.float(value),
        }
    }

    fn field(&mut self, target: &Expr, field: &Name) -> Checked<Node> {
        let scopes = || {
            let mut scopes = Vec::new();
            for (scope, _) in builtins::CONTEXT.iter().filter_map(|value| value.field()) {
                if !scopes.contains(&scope) {
                    scopes.push(scope);
                }
            }
            let scopes = scopes
                .iter()
                .map(|scope| format!("`{scope}`"))
                .collect::<Vec<_>>();
            format!("only {} have fields", scopes.join(" and "))
        };
        let ExprKind::Name(scope) = &target.kind else {
            self.error(field.span, scopes());
            return None;
        };
        let Some(value) = context_field(scope.as_str(), field.name.as_str()) else {
            let message = if is_context_scope(scope.as_str()) {
                format!(
                    "`{}` has no field `{}`",
                    scope.as_str(),
                    field.name.as_str()
                )
            } else {
                scopes()
            };
            self.error(field.span, message);
            return None;
        };
        self.refer(target.span, ScriptTarget::Context(value));
        self.refer(field.span, ScriptTarget::Context(value));
        Some(self.read(value.read))
    }

    /// The signal `name` reads: an operator input, or `previous`, the node's
    /// own previous frame. Not inside a function, where inputs are out of
    /// scope.
    fn signal(&mut self, name: &Name) -> Option<u32> {
        if !self.frames.is_empty() || self.scopes.iter().any(|(known, ..)| known == &name.name) {
            return None;
        }
        match self
            .inputs
            .iter()
            .position(|input| input.value == name.name)
        {
            Some(index) => {
                self.refer_symbol(name.span, self.inputs[index].symbol);
                Some(index as u32)
            }
            None => {
                let value = context_value(name.name.as_str())
                    .filter(|value| value.read == Read::Previous)?;
                self.refer(name.span, ScriptTarget::Context(value));
                Some(STATE)
            }
        }
    }

    fn method(
        &mut self,
        target: &Expr,
        method: &Name,
        args: &[Expr],
        span: TextSpan,
    ) -> Checked<Node> {
        let input = match &target.kind {
            ExprKind::Name(name) => self.signal(&Name {
                name: name.clone(),
                span: target.span,
            }),
            _ => None,
        };
        let Some(input) = input else {
            self.error(
                method.span,
                "only operator inputs and `previous` have methods",
            );
            return None;
        };
        if method.name.as_str() != AT.name {
            let message = format!(
                "signals have no method `{}`; use `{}`",
                method.name.as_str(),
                AT.name
            );
            self.error(method.span, message);
            return None;
        }
        self.refer(method.span, ScriptTarget::Method(&AT));
        if args.len() != 1 {
            self.error(
                span,
                format!(
                    "`{}` takes a local pixel index; signals are read at this frame",
                    AT.name
                ),
            );
            return None;
        }
        let pixel = SignalPixel::Local(self.int(&args[0])?);
        Some(self.graph.add(Op::Sample { input, pixel }))
    }

    /// An array literal of one item type; ints widen when any item is a float.
    fn array(&mut self, items: &[Expr], span: TextSpan) -> Checked<Node> {
        let nodes = items
            .iter()
            .map(|item| self.value(item))
            .collect::<Vec<_>>();
        let mut nodes = nodes.into_iter().collect::<Option<Vec<_>>>()?;
        let float = nodes.iter().any(|&node| *self.ty(node) == Type::Float);
        let ty = if float {
            Type::Float
        } else {
            self.ty(nodes[0]).clone()
        };
        if !matches!(ty, Type::Int | Type::Float | Type::Bool | Type::Color) {
            self.error(span, "array literals hold numbers, bools or colors");
            return None;
        }
        for (node, item) in nodes.iter_mut().zip(items) {
            *node = self.require(*node, &ty, item.span)?;
        }
        Some(self.graph.add(Op::Items(nodes.into())))
    }

    fn index(&mut self, target: &Expr, index: &Expr) -> Checked<Node> {
        let collection = self.value(target)?;
        match self.ty(collection).clone() {
            Type::Array(_) => {
                // A float index floors; clamping makes truncation agree below zero.
                let position = self.value(index)?;
                let position = match self.ty(position) {
                    Type::Float => self.graph.unary(Unary::FloatToInt, position),
                    _ => self.require(position, &Type::Int, index.span)?,
                };
                Some(self.graph.binary(Binary::Index, collection, position))
            }
            Type::Curve => {
                let position = self.float(index)?;
                Some(self.graph.binary(Binary::CurveSample, collection, position))
            }
            Type::Gradient => {
                let position = self.float(index)?;
                Some(
                    self.graph
                        .binary(Binary::GradientSample, collection, position),
                )
            }
            ty => {
                let message = format!(
                    "only arrays, curves and gradients can be indexed, not {}",
                    describe(&ty)
                );
                self.error(target.span, message);
                None
            }
        }
    }

    fn binary(&mut self, op: BinaryOp, left: &Expr, right: &Expr, span: TextSpan) -> Checked<Node> {
        if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) {
            return self.equality(op == BinaryOp::Equal, left, right, span);
        }
        let a = self.value(left);
        let b = self.value(right);
        let (a, b) = (a?, b?);
        let (left_ty, right_ty) = (self.ty(a).clone(), self.ty(b).clone());
        let numeric = |ty: &Type| matches!(ty, Type::Int | Type::Float);
        let ints = left_ty == Type::Int && right_ty == Type::Int;
        let graph = &mut self.graph;
        let floats = |graph: &mut Graph| {
            let a = if left_ty == Type::Int {
                graph.unary(Unary::IntToFloat, a)
            } else {
                a
            };
            let b = if right_ty == Type::Int {
                graph.unary(Unary::IntToFloat, b)
            } else {
                b
            };
            (a, b)
        };
        let node = match op {
            BinaryOp::And | BinaryOp::Or if left_ty == Type::Bool && right_ty == Type::Bool => {
                if op == BinaryOp::And {
                    graph.and(a, b)
                } else {
                    graph.or(a, b)
                }
            }
            BinaryOp::Add if left_ty == Type::Color && right_ty == Type::Color => {
                graph.binary(Binary::ColorAdd, a, b)
            }
            BinaryOp::Multiply if left_ty == Type::Color && right_ty == Type::Color => {
                graph.binary(Binary::ColorMultiply, a, b)
            }
            BinaryOp::Multiply if left_ty == Type::Color && numeric(&right_ty) => {
                let (_, scale) = floats(graph);
                graph.binary(Binary::ColorScale, a, scale)
            }
            BinaryOp::Multiply if numeric(&left_ty) && right_ty == Type::Color => {
                let (scale, _) = floats(graph);
                graph.binary(Binary::ColorScale, b, scale)
            }
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Remainder
            | BinaryOp::FloorDivide
                if ints =>
            {
                let op = match op {
                    BinaryOp::Add => Binary::IntAdd,
                    BinaryOp::Subtract => Binary::IntSubtract,
                    BinaryOp::Multiply => Binary::IntMultiply,
                    BinaryOp::FloorDivide => Binary::IntFloorDivide,
                    _ => Binary::IntRemainder,
                };
                graph.binary(op, a, b)
            }
            BinaryOp::FloorDivide if numeric(&left_ty) && numeric(&right_ty) => {
                let (a, b) = floats(graph);
                let quotient = graph.binary(Binary::Divide, a, b);
                graph.unary(Unary::Floor, quotient)
            }
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Remainder
                if numeric(&left_ty) && numeric(&right_ty) =>
            {
                let (a, b) = floats(graph);
                let op = match op {
                    BinaryOp::Add => Binary::Add,
                    BinaryOp::Subtract => Binary::Subtract,
                    BinaryOp::Multiply => Binary::Multiply,
                    BinaryOp::Divide => Binary::Divide,
                    _ => Binary::Remainder,
                };
                graph.binary(op, a, b)
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual
                if numeric(&left_ty) && numeric(&right_ty) =>
            {
                let (op, a, b) = if ints {
                    let op = match op {
                        BinaryOp::Less => Binary::IntLess,
                        BinaryOp::LessEqual => Binary::IntLessEqual,
                        BinaryOp::Greater => Binary::IntGreater,
                        _ => Binary::IntGreaterEqual,
                    };
                    (op, a, b)
                } else {
                    let (a, b) = floats(graph);
                    let op = match op {
                        BinaryOp::Less => Binary::Less,
                        BinaryOp::LessEqual => Binary::LessEqual,
                        BinaryOp::Greater => Binary::Greater,
                        _ => Binary::GreaterEqual,
                    };
                    (op, a, b)
                };
                graph.binary(op, a, b)
            }
            _ => {
                let message = format!(
                    "`{}` does not apply to {} and {}",
                    operator_text(op),
                    describe(&left_ty),
                    describe(&right_ty)
                );
                self.error(span, message);
                return None;
            }
        };
        Some(node)
    }

    fn equality(
        &mut self,
        equal: bool,
        left: &Expr,
        right: &Expr,
        span: TextSpan,
    ) -> Checked<Node> {
        let a = self.operand(left);
        let b = self.operand(right);
        let (a, b) = (a?, b?);
        let op = if equal {
            Binary::Equal
        } else {
            Binary::NotEqual
        };
        let (a, b) = match (a, b) {
            (Operand::Node(a), Operand::Node(b)) => (a, b),
            (Operand::Node(node), Operand::Option(name, option_span))
            | (Operand::Option(name, option_span), Operand::Node(node)) => {
                let option = self.option(node, name, option_span)?;
                (node, option)
            }
            (Operand::Option(name, span), Operand::Option(..)) => {
                self.error(span, format!("unknown name `{}`", name.as_str()));
                return None;
            }
        };
        let (left_ty, right_ty) = (self.ty(a).clone(), self.ty(b).clone());
        let (a, b) = match (&left_ty, &right_ty) {
            (Type::Int, Type::Float) | (Type::Float, Type::Int) => self.unify(a, b, span)?,
            (Type::Int | Type::Float | Type::Bool | Type::Color, _) if left_ty == right_ty => {
                (a, b)
            }
            (Type::Enum(_), Type::Enum(_)) => (a, b),
            _ => {
                let message = format!(
                    "{} and {} cannot be compared",
                    describe(&left_ty),
                    describe(&right_ty)
                );
                self.error(span, message);
                return None;
            }
        };
        Some(self.graph.binary(op, a, b))
    }

    /// An enum option named in a comparison with a value of that enum.
    fn option(&mut self, node: Node, name: Identifier, span: TextSpan) -> Checked<Node> {
        match self.ty(node).clone() {
            Type::Enum(options) if options.contains(&name) => {
                let symbol = self
                    .options
                    .iter()
                    .find(|(option, _)| option == &name)
                    .map(|(_, symbol)| *symbol);
                self.refer_symbol(span, symbol);
                Some(
                    self.graph
                        .typed_constant(Value::Enum(name), Type::Enum(options)),
                )
            }
            Type::Enum(_) => {
                self.error(
                    span,
                    format!("`{}` is not an option of this enum", name.as_str()),
                );
                None
            }
            _ => {
                self.error(span, format!("unknown name `{}`", name.as_str()));
                None
            }
        }
    }

    fn reduction(&mut self, reduction: &Reduction, span: TextSpan) -> Checked<Node> {
        let Reduction {
            reducer,
            index,
            start,
            end,
            inclusive,
            body,
            otherwise,
            around,
        } = reduction;
        let (reducer, otherwise, around) = (*reducer, otherwise.as_ref(), around.as_ref());
        let reducer_name = builtins::reducer(reducer).name;
        self.refer(
            TextSpan {
                start: span.start,
                end: span.start + reducer_name.len(),
            },
            ScriptTarget::Reducer(builtins::reducer(reducer)),
        );
        let start = self.bound_value(start);
        let end = self.bound_value(end);
        let (start, end) = (start?, end?);
        let end = if *inclusive {
            let one = self.graph.int(1);
            self.graph.binary(Binary::IntAdd, end, one)
        } else {
            end
        };
        let count = self.graph.binary(Binary::IntSubtract, end, start);
        self.bound(count, span, "reduction")?;
        if let Some(around) = around {
            self.neighborhood(start, end, around.edges, span)?;
        }
        // Neighborhoods span at most a strip, so every pixel shares the offset.
        let Ok((id, index_node)) = self.graph.begin_loop(start, end, around.is_some()) else {
            self.error(
                span,
                format!("a definition has at most {} reductions", LoopSet::LIMIT),
            );
            return None;
        };
        self.diagnostics
            .extend(name_diagnostic(NameKind::Value, index));
        let depth = self.scopes.len();
        let neighbor = match around {
            Some(around) => Some(self.neighbor(around, index_node)?),
            None => None,
        };
        if let (Some(sample), Some(name)) = (neighbor, around.and_then(|a| a.neighbor.as_ref())) {
            self.diagnostics
                .extend(name_diagnostic(NameKind::Value, name));
            self.bind(name, SymbolKind::Let, sample);
        }
        self.bind(index, SymbolKind::Index, index_node);
        let result_span = body.result.span;
        let outcome = self.tail_block(body, Mode::Tail);
        self.scopes.truncate(depth);
        let outcome = outcome?;
        let value = outcome.value?;
        let ty = self.ty(value).clone();
        let mut valid = outcome.valid;
        let stencil = neighbor.filter(|&sample| {
            reads(&self.graph, value, sample) || reads(&self.graph, valid, sample)
        });
        // A neighborhood that ignores its neighbors runs for the offsets that
        // have one.
        if let Some(around) = around
            && stencil.is_none()
            && around.edges == Edges::Skip
        {
            let present = self.present(index_node);
            valid = self.graph.and(present, valid);
        }
        let outcome = Outcome { valid, ..outcome };
        let filter = (self.graph.constant_value(outcome.valid) != Some(&Value::Bool(true)))
            .then_some(outcome.valid);
        let (reducer, body, filter, default) = match reducer {
            ReducerKind::Max | ReducerKind::Min | ReducerKind::Sum => {
                let colors = reducer != ReducerKind::Min && ty == Type::Color;
                if !(matches!(ty, Type::Int | Type::Float) || colors) {
                    let message = format!("`{reducer_name}` cannot combine {}", describe(&ty));
                    self.error(body.result.span, message);
                    return None;
                }
                let reducer = match reducer {
                    ReducerKind::Max => Reducer::Max,
                    ReducerKind::Min => Reducer::Min,
                    _ => Reducer::Sum,
                };
                (reducer, value, filter, None)
            }
            ReducerKind::Any | ReducerKind::All => {
                let value = self.require(value, &Type::Bool, body.result.span)?;
                let (reducer, body) = if reducer == ReducerKind::Any {
                    (Reducer::Any, self.graph.and(outcome.valid, value))
                } else {
                    let skipped = self.graph.unary(Unary::Not, outcome.valid);
                    (Reducer::All, self.graph.or(skipped, value))
                };
                (reducer, body, None, None)
            }
            ReducerKind::First | ReducerKind::Last => {
                if matches!(ty, Type::Array(_)) {
                    self.error(
                        body.result.span,
                        format!("`{reducer_name}` cannot produce an array"),
                    );
                    return None;
                }
                let default = match otherwise {
                    Some(block) => {
                        let value = self.tail_block(block, Mode::Value)?.value?;
                        self.require(value, &ty, block.result.span)?
                    }
                    None if ty == Type::Color => self.graph.color(Color::BLACK),
                    None => {
                        let message = format!(
                            "`{reducer_name}` of {} needs an `else` value",
                            describe(&ty)
                        );
                        self.error(span, message);
                        return None;
                    }
                };
                let reducer = if reducer == ReducerKind::First {
                    Reducer::First
                } else {
                    Reducer::Last
                };
                (reducer, value, filter, Some(default))
            }
        };
        if let (Some(around), Some(_)) = (around, stencil) {
            if let Err(error) = check_stencil(&self.graph, id, reducer, body, filter) {
                self.error(result_span, error.message());
                return None;
            }
            self.graph.set_around(id, around.edges);
        }
        Some(self.graph.finish_loop(id, reducer, body, filter, default))
    }

    /// Check that a neighborhood's offsets fit one stencil: at most a strip
    /// of them, reaching at most half a strip past a fixture's ends.
    fn neighborhood(
        &mut self,
        start: Node,
        end: Node,
        edges: Edges,
        span: TextSpan,
    ) -> Checked<()> {
        if self.standalone {
            return Some(());
        }
        let ranges = param_ranges(self.params.iter().map(|param| &param.value));
        let lengths = vec![None; self.params.len()];
        let bounds = Bounds {
            ranges: &ranges,
            lengths: &lengths,
        };
        let (first, past) = (
            interval(&self.graph, start, &bounds).min,
            interval(&self.graph, end, &bounds).max,
        );
        let reach = (STRIP / 2) as f64;
        if !(first.is_finite() && past.is_finite() && past - first <= STRIP as f64) {
            let message = format!(
                "cannot prove this `around` range spans at most {STRIP} offsets; bound it with literals or parameter ranges"
            );
            self.error(span, message);
            return None;
        }
        if edges != Edges::Skip && (first < -reach || past - 1.0 > reach) {
            let message = format!(
                "an extended or mirrored `around` reaches at most {} pixels each way",
                STRIP / 2
            );
            self.error(span, message);
            return None;
        }
        Some(())
    }

    /// A scan: `previous * decay + light * weight`, with the decay fixed for
    /// the frame and the weight depending on the light alone.
    fn scan(&mut self, scan: &Scan) -> Checked<Node> {
        let Scan {
            direction,
            input,
            light,
            previous,
            body,
        } = scan;
        let Some(index) = self.signal(input) else {
            let message = format!(
                "`scan` reads an operator input or `previous`; `{}` is neither",
                input.name.as_str()
            );
            self.error(input.span, message);
            return None;
        };
        let sample = self.graph.add(Op::Sample {
            input: index,
            pixel: SignalPixel::Current,
        });
        let before = self.graph.add(Op::Previous);
        let depth = self.scopes.len();
        for (name, node) in [(light, sample), (previous, before)] {
            self.diagnostics
                .extend(name_diagnostic(NameKind::Value, name));
            self.bind(name, SymbolKind::Let, node);
        }
        let value = self.tail_block(body, Mode::Value);
        self.scopes.truncate(depth);
        let value = value?.value?;
        let value = self.require(value, &Type::Color, body.result.span)?;
        if !reads(&self.graph, value, before) {
            return Some(value);
        }
        let Some((decay, weight)) = self.trail(value, sample, before) else {
            self.error(
                body.result.span,
                "a scan produces `previous * decay + light * weight`, with the decay fixed for the frame and the weight depending on `light`, parameters and time alone",
            );
            return None;
        };
        Some(self.graph.add(Op::Scan {
            direction: *direction,
            light: sample,
            decay,
            weight,
        }))
    }

    /// The decay and weight of `previous * decay + light * weight`.
    fn trail(&mut self, value: Node, light: Node, previous: Node) -> Option<(Node, Option<Node>)> {
        let Op::Binary(Binary::ColorAdd, a, b) = *self.graph.op(value) else {
            return None;
        };
        let decay_of = |graph: &mut Graph, node: Node| match *graph.op(node) {
            _ if node == previous => Some(graph.float(1.0)),
            Op::Binary(Binary::ColorScale, color, decay) if color == previous => Some(decay),
            _ => None,
        };
        let (decay, term) = match (decay_of(&mut self.graph, a), decay_of(&mut self.graph, b)) {
            (Some(decay), None) => (decay, b),
            (None, Some(decay)) => (decay, a),
            _ => return None,
        };
        let fixed = self.graph.loops_of(decay).is_empty()
            && !self.graph.domain(decay).intersects(
                super::ir::Domain::PIXEL
                    .union(super::ir::Domain::SIGNAL)
                    .union(super::ir::Domain::TARGET),
            );
        let weight = match *self.graph.op(term) {
            _ if term == light => None,
            Op::Binary(Binary::ColorScale, color, weight) if color == light => Some(weight),
            _ => return None,
        };
        let weighted = weight.is_none_or(|weight| {
            !reads(&self.graph, weight, previous) && source_only(&self.graph, weight, light)
        });
        (fixed && weighted).then_some((decay, weight))
    }

    /// The input `around` names, read `offset` pixels along the fixture.
    fn neighbor(&mut self, around: &Around, offset: Node) -> Checked<Node> {
        let name = &around.input;
        let Some(input) = self.signal(name) else {
            let message = format!(
                "`around` reads an operator input or `previous`; `{}` is neither",
                name.name.as_str()
            );
            self.error(name.span, message);
            return None;
        };
        Some(self.graph.add(Op::Sample {
            input,
            pixel: SignalPixel::Shifted(offset, around.edges),
        }))
    }

    /// Whether the pixel `offset` pixels along the fixture exists.
    fn present(&mut self, offset: Node) -> Node {
        let index = self.graph.add(Op::Context(Context::PixelIndex));
        let at = self.graph.binary(Binary::IntAdd, index, offset);
        let zero = self.graph.int(0);
        let count = self.graph.add(Op::Context(Context::TargetCount));
        let low = self.graph.binary(Binary::IntGreaterEqual, at, zero);
        let high = self.graph.binary(Binary::IntLess, at, count);
        self.graph.and(low, high)
    }

    /// A reduction bound, which counts whole iterations.
    fn bound_value(&mut self, expr: &Expr) -> Checked<Node> {
        let node = self.value(expr)?;
        if *self.ty(node) == Type::Float {
            self.error(
                expr.span,
                "reduction bounds are ints; this is a float, so convert it with `int(...)` or divide with `//`",
            );
            return None;
        }
        self.require(node, &Type::Int, expr.span)
    }

    /// Prove that `count` stays within the iteration limit, or defer the proof
    /// to binding when it depends on parameter lengths.
    fn bound(&mut self, count: Node, span: TextSpan, what: &str) -> Checked<()> {
        if self.standalone {
            return Some(());
        }
        let ranges = param_ranges(self.params.iter().map(|param| &param.value));
        let lengths = vec![None; self.params.len()];
        let range = interval(
            &self.graph,
            count,
            &Bounds {
                ranges: &ranges,
                lengths: &lengths,
            },
        );
        if range.max <= MAX_ITERATIONS as f64 && !range.max.is_nan() {
            return Some(());
        }
        if depends_on_length(&self.graph, count) {
            self.length_bounds.push(count);
            return Some(());
        }
        self.error(
            span,
            format!(
                "cannot prove this {what} runs at most {MAX_ITERATIONS} times; bound it with literals, parameter ranges or `len()`"
            ),
        );
        None
    }

    fn call(&mut self, name: &Name, args: &[Expr], span: TextSpan) -> Checked<Node> {
        let Some(builtin) = builtin(name.name.as_str()) else {
            return match self
                .functions
                .iter()
                .position(|function| function.name.name == name.name)
            {
                Some(index) => {
                    self.refer_symbol(name.span, self.function_symbols[index]);
                    self.inline(index, args, span)
                }
                None => {
                    let message = format!("unknown function `{}`", name.name.as_str());
                    self.error(name.span, message);
                    None
                }
            };
        };
        self.refer(name.span, ScriptTarget::Builtin(builtin));
        let (function, arity) = (builtin.name, builtin.arity());
        if args.len() != arity {
            let message = format!(
                "`{function}` takes {arity} argument{}, found {}",
                if arity == 1 { "" } else { "s" },
                args.len()
            );
            self.error(span, message);
            return None;
        }
        if let [signature] = builtin.signatures
            && let Lowering::Integral = signature.lowering
        {
            return self.integral(&args[0]);
        }
        // Each argument narrows the signatures it fits. Once every signature
        // left converts an argument alike, it is converted; an int fitting
        // several signatures waits for the arguments after it.
        let mut candidates = builtin.signatures.iter().collect::<Vec<_>>();
        let mut nodes = Vec::with_capacity(args.len());
        let mut converted = 0;
        for (position, arg) in args.iter().enumerate() {
            let node = self.value(arg)?;
            let ty = self.ty(node).clone();
            let fitting = candidates
                .iter()
                .copied()
                .filter(|signature| signature.args[position].1.fit(&ty).is_some())
                .collect::<Vec<_>>();
            if fitting.is_empty() {
                let mut expected = Vec::new();
                for signature in &candidates {
                    let described = signature.args[position].1.describe();
                    if !expected.contains(&described) {
                        expected.push(described);
                    }
                }
                let expected = match expected.split_last() {
                    Some((last, [])) => last.clone(),
                    Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
                    None => unreachable!("a builtin has a signature"),
                };
                self.error(
                    arg.span,
                    format!("expected {expected}, found {}", describe(&ty)),
                );
                return None;
            }
            candidates = fitting;
            nodes.push(node);
            while converted < nodes.len() {
                let fit = |signature: &&super::builtins::Signature, graph: &Graph| {
                    signature.args[converted].1.fit(graph.ty(nodes[converted]))
                };
                let first = fit(&candidates[0], &self.graph);
                if candidates
                    .iter()
                    .any(|candidate| fit(candidate, &self.graph) != first)
                {
                    break;
                }
                if first == Some(Fit::Widened) {
                    nodes[converted] = self.graph.unary(Unary::IntToFloat, nodes[converted]);
                }
                converted += 1;
            }
        }
        // A signature every argument fits exactly, else one they fit widened.
        let exact = |signature: &&super::builtins::Signature, graph: &Graph| {
            nodes.iter().enumerate().all(|(position, node)| {
                signature.args[position].1.fit(graph.ty(*node)) == Some(Fit::Exact)
            })
        };
        let signature = candidates
            .iter()
            .copied()
            .find(|signature| exact(signature, &self.graph))
            .unwrap_or(candidates[0]);
        for (position, node) in nodes.iter_mut().enumerate().skip(converted) {
            if signature.args[position].1.fit(self.graph.ty(*node)) == Some(Fit::Widened) {
                *node = self.graph.unary(Unary::IntToFloat, *node);
            }
        }
        Some(match signature.lowering {
            Lowering::Unary(op) => self.graph.unary(op, nodes[0]),
            Lowering::Binary(op) => self.graph.binary(op, nodes[0], nodes[1]),
            Lowering::Ternary(op) => self.graph.ternary(op, nodes[0], nodes[1], nodes[2]),
            Lowering::Expand(expand) => expand(self, &nodes),
            Lowering::Integral => unreachable!("`integral` takes a parameter name"),
        })
    }

    /// `integral(parameter)`: the argument names a float parameter.
    fn integral(&mut self, arg: &Expr) -> Checked<Node> {
        let parameter = match &arg.kind {
            ExprKind::Name(name)
                if self.frames.is_empty()
                    && !self.scopes.iter().any(|(bound, ..)| bound == name) =>
            {
                self.params
                    .iter()
                    .position(|param| &param.value.name == name && param.value.ty == Type::Float)
            }
            _ => None,
        };
        let Some(index) = parameter else {
            self.error(arg.span, "`integral` needs the name of a float parameter");
            return None;
        };
        self.refer_symbol(arg.span, self.params[index].symbol);
        Some(self.graph.add(Op::ParamIntegral(index as u32)))
    }

    /// -1, 0 or 1 by sign; zeros and NaN pass through.
    pub(crate) fn float_sign(&mut self, value: Node) -> Node {
        let zero = self.graph.float(0.0);
        let positive = self.graph.binary(Binary::Greater, value, zero);
        let negative = self.graph.binary(Binary::Less, value, zero);
        let (one, minus_one) = (self.graph.float(1.0), self.graph.float(-1.0));
        let low = self.graph.select(negative, minus_one, value);
        self.graph.select(positive, one, low)
    }

    /// Whether an int is provably a small non-negative count.
    pub(crate) fn counts(&self, node: Node) -> bool {
        if self.standalone {
            return false;
        }
        let ranges = param_ranges(self.params.iter().map(|param| &param.value));
        let lengths = vec![None; self.params.len()];
        let range = interval(
            &self.graph,
            node,
            &Bounds {
                ranges: &ranges,
                lengths: &lengths,
            },
        );
        range.min >= 0.0 && range.max <= MAX_ITERATIONS as f64 && !range.max.is_nan()
    }

    /// Inline a call of a user function: its arguments are checked in the
    /// caller, its body sees only them and the context values.
    fn inline(&mut self, index: usize, args: &[Expr], span: TextSpan) -> Checked<Node> {
        let functions = self.functions.clone();
        let function = &functions[index];
        let name = function.name.name.as_str();
        if self.frames.contains(&index) {
            self.error(
                span,
                format!("`{name}` calls itself; functions cannot recurse"),
            );
            return None;
        }
        if args.len() != function.args.len() {
            let arity = function.args.len();
            let message = format!(
                "`{name}` takes {arity} argument{}, found {}",
                if arity == 1 { "" } else { "s" },
                args.len()
            );
            self.error(span, message);
            return None;
        }
        let mut bound = Vec::new();
        for ((arg, ty), expr) in function.args.iter().zip(args) {
            let ty = resolve_type(ty).ok()?;
            bound.push((arg.name.clone(), self.typed(expr, &ty)));
        }
        let bound = bound
            .into_iter()
            .map(|(name, node)| Some((name, node?, None)))
            .collect::<Option<Vec<_>>>()?;
        let result = resolve_type(&function.result).ok()?;
        let caller = core::mem::replace(&mut self.scopes, bound);
        self.frames.push(index);
        let value = self
            .tail_block(&function.body, Mode::Value)
            .and_then(|outcome| outcome.value);
        self.frames.pop();
        self.scopes = caller;
        self.require(value?, &result, function.body.result.span)
    }
}

fn param_ranges<'a>(params: impl Iterator<Item = &'a ParamDecl>) -> Vec<Option<(f64, f64)>> {
    params
        .map(|param| match (&param.ty, param.range) {
            (Type::Int, Some(ParamRange::Int { min, max })) => {
                Some((f64::from(min), f64::from(max)))
            }
            (Type::Float, Some(ParamRange::Float { min, max })) => {
                Some((f64::from(min), f64::from(max)))
            }
            _ => None,
        })
        .collect()
}

/// Whether `node` reads the length of an array or marks parameter.
fn depends_on_length(graph: &Graph, node: Node) -> bool {
    let mut pending = vec![node];
    let mut seen = std::collections::HashSet::new();
    while let Some(node) = pending.pop() {
        if !seen.insert(node) {
            continue;
        }
        match graph.op(node) {
            Op::Unary(Unary::Len | Unary::MarkCount, operand)
            | Op::Binary(Binary::MarkLastIndex, operand, _)
                if matches!(graph.op(*operand), Op::Param(_)) =>
            {
                return true;
            }
            Op::LoopIndex(id) => {
                let data = graph.loop_(*id);
                pending.extend([data.start, data.end]);
            }
            op => pending.extend(op.operands()),
        }
    }
    false
}

/// Check one parameter declaration: its type, range and default.
fn check_param(param: &super::syntax::ast::Param) -> Result<ParamDecl, Diagnostic> {
    let ty = resolve_type(&param.ty)?;
    let span = param.name.span;
    let name = param.name.name.as_str();
    let ranged = match &ty {
        Type::Int | Type::Float | Type::Curve => true,
        Type::Array(item) => **item == Type::Curve,
        _ => false,
    };
    let range = match (&param.range, ranged) {
        (None, true) => {
            return Err(Diagnostic::new(
                span,
                format!("`{name}` must declare its range, like `in 0.0..1.0`"),
            ));
        }
        (Some((min, _)), false) => {
            return Err(Diagnostic::new(
                min.span,
                "only int, float, curve and array<curve> parameters take a range",
            ));
        }
        (None, false) => None,
        (Some((min, max)), true) => {
            let range = match (&ty, &min.kind, &max.kind) {
                (Type::Int, LiteralKind::Int(min), LiteralKind::Int(max)) => {
                    match (i32::try_from(*min), i32::try_from(*max)) {
                        (Ok(min), Ok(max)) => Some(ParamRange::Int { min, max }),
                        _ => None,
                    }
                }
                (Type::Float | Type::Curve | Type::Array(_), min, max) => number(min)
                    .zip(number(max))
                    .map(|(min, max)| ParamRange::Float { min, max }),
                _ => None,
            };
            match range {
                Some(range) if range.fits(&ty) => Some(range),
                _ => {
                    return Err(Diagnostic::new(
                        min.span.to(max.span),
                        "a range is two finite literals of the parameter's type, minimum first",
                    ));
                }
            }
        }
    };
    let default = match &param.default {
        None => None,
        Some(literal) => {
            let value = match (&ty, &literal.kind) {
                (Type::Int, LiteralKind::Int(value)) => i32::try_from(*value).ok().map(Value::Int),
                (Type::Float, kind) => number(kind).map(Value::Float),
                (Type::Bool, LiteralKind::Bool(value)) => Some(Value::Bool(*value)),
                (Type::Color, LiteralKind::Color(value)) => Some(Value::Color(*value)),
                (Type::Enum(options), LiteralKind::Name(value)) if options.contains(value) => {
                    Some(Value::Enum(value.clone()))
                }
                _ => None,
            };
            let Some(value) = value else {
                let message = match ty {
                    Type::Int | Type::Float | Type::Bool | Type::Color | Type::Enum(_) => {
                        format!("the default of `{name}` must be {}", describe(&ty))
                    }
                    _ => format!(
                        "{} has no literal, so `{name}` has no default",
                        describe(&ty)
                    ),
                };
                return Err(Diagnostic::new(literal.span, message));
            };
            Some(value)
        }
    };
    let declaration = ParamDecl {
        name: param.name.name.clone(),
        ty,
        range,
        default,
        description: param.description.clone(),
    };
    if let Some(default) = &declaration.default
        && !declaration.accepts_value(default)
    {
        return Err(Diagnostic::new(
            span,
            format!("the default of `{name}` is outside its range"),
        ));
    }
    Ok(declaration)
}

fn number(kind: &LiteralKind) -> Option<f32> {
    match *kind {
        LiteralKind::Int(value) => i32::try_from(value).ok().map(|value| value as f32),
        LiteralKind::Float(value) => Some(value),
        _ => None,
    }
}

pub(crate) fn resolve_type(ty: &TypeExpr) -> Result<Type, Diagnostic> {
    Ok(match &ty.kind {
        TypeKind::Named(name) => match NAMED_TYPES
            .iter()
            .find(|(candidate, _)| *candidate == name.as_str())
        {
            Some((_, ty)) => ty.clone(),
            None => {
                return Err(Diagnostic::new(
                    ty.span,
                    format!("unknown type `{}`", name.as_str()),
                ));
            }
        },
        TypeKind::Enum(options) => {
            let mut names = Vec::new();
            for option in options {
                if let Some(diagnostic) = name_diagnostic(NameKind::EnumOption, option) {
                    return Err(diagnostic);
                }
                if names.contains(&option.name) {
                    return Err(Diagnostic::new(
                        option.span,
                        format!("option `{}` is listed twice", option.name.as_str()),
                    ));
                }
                names.push(option.name.clone());
            }
            Type::Enum(names)
        }
        TypeKind::Array(item) => match resolve_type(item)? {
            Type::Array(_) => {
                return Err(Diagnostic::new(
                    item.span,
                    "an array's items cannot be arrays",
                ));
            }
            item => Type::array(item),
        },
    })
}

fn operator_text(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::FloorDivide => "//",
        BinaryOp::Remainder => "%",
        BinaryOp::Less => "<",
        BinaryOp::LessEqual => "<=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

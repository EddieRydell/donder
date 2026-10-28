//! The Donder language server, independent of how messages travel: a host
//! passes each JSON-RPC message to [`Server::handle`], sends what it returns,
//! and calls [`Server::idle`] when messages pause, which takes the latest
//! project check and publishes its diagnostics. Requests answer at once from
//! the current texts and the last check; they never wait for a new one. Hosts: `donder lsp` over stdio, the desktop over
//! Tauri, and the website in a worker.
mod data;
mod document;
mod navigation;
mod text;
mod tokens;
mod workspace;

use std::collections::HashMap;

use std::sync::Arc;

use donder_language::analysis::{
    Completion, CompletionKind, SymbolKind, script_completions, script_signature,
};
use donder_project_io::{IoFix, PROJECT_ROOT_FILE, include_document};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CompletionItem,
    CompletionItemKind, CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    InitializeParams, InsertTextFormat, Location, MarkupContent, MarkupKind, ParameterInformation,
    ParameterLabel, PrepareRenameResponse, PublishDiagnosticsParams, Range, ReferenceParams,
    RenameParams, SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult, ServerCapabilities, SignatureHelp as LspSignatureHelp,
    SignatureHelpParams, SignatureInformation, SymbolKind as Kind, TextDocumentPositionParams,
    TextEdit, Uri, WorkspaceEdit,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::document::Document;
use crate::text::LineIndex;
pub use crate::text::{file_uri, uri_path};
use crate::workspace::{Checker, Workspace, kind};
pub use crate::workspace::{DocumentSource, ProjectCheck};

/// How long hosts wait after the last message before calling [`Server::idle`].
pub const IDLE_DELAY_MS: u64 = 150;

/// One language server session.
#[derive(Default)]
pub struct Server {
    workspace: Workspace,
    /// Kept across sessions: each `initialize` starts a fresh workspace.
    source: Option<Arc<dyn DocumentSource + Sync>>,
    shutdown: bool,
}

const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;
const REQUEST_FAILED: i32 = -32803;

type Reply = Result<Value, (i32, String)>;

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, (i32, String)> {
    serde_json::from_value(value).map_err(|error| (INVALID_PARAMS, error.to_string()))
}

fn to_value<T: Serialize>(value: T) -> Reply {
    serde_json::to_value(value).map_err(|error| (REQUEST_FAILED, error.to_string()))
}

fn notification(method: &str, params: impl Serialize) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

/// A diagnostic's fix as the client returns it in code action requests. An
/// inclusion is looked up again in the project check when it is applied.
fn fix_data(fix: Option<IoFix>) -> Option<Value> {
    match fix? {
        IoFix::Replace(text) => Some(json!({ "fix": text })),
        IoFix::Include(_) => Some(json!({ "include": true })),
    }
}

fn uri(text: &str) -> Option<Uri> {
    text.parse().ok()
}

impl Server {
    pub fn new() -> Self {
        Self::default()
    }

    /// A server for a host that keeps its own copies of documents and checks
    /// the project itself.
    pub fn with_source(source: Arc<dyn DocumentSource + Sync>) -> Self {
        Self {
            workspace: Workspace::with_checker(Checker::Host(Arc::clone(&source))),
            source: Some(source),
            shutdown: false,
        }
    }

    /// Whether the client asked to exit.
    pub fn exited(&self) -> bool {
        self.shutdown
    }

    /// Handle one message; returns the messages to send.
    pub fn handle(&mut self, mut message: Value) -> Vec<Value> {
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let params = message
            .get_mut("params")
            .map(Value::take)
            .unwrap_or(Value::Null);
        match message.get("id").cloned() {
            Some(id) if !method.is_empty() => {
                // Following names across documents needs the index of the
                // current texts.
                let checked = matches!(
                    method.as_str(),
                    "textDocument/definition"
                        | "textDocument/references"
                        | "textDocument/prepareRename"
                        | "textDocument/rename"
                ) && self.workspace.refresh();
                let reply = self.request(&method, params);
                let mut out = vec![match reply {
                    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Err((code, message)) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": code, "message": message },
                    }),
                }];
                if checked {
                    out.extend(self.publish());
                }
                out
            }
            Some(_) => Vec::new(),
            None => self.notification(&method, params),
        }
    }

    /// Take the latest check and publish diagnostics. Hosts call this when
    /// messages pause, so typing does not recheck on every keystroke.
    pub fn idle(&mut self) -> Vec<Value> {
        let checked = self.workspace.refresh();
        let edited = std::mem::take(&mut self.workspace.stale);
        if !checked && !edited {
            return Vec::new();
        }
        self.publish()
    }

    fn notification(&mut self, method: &str, params: Value) -> Vec<Value> {
        match method {
            "textDocument/didOpen" => {
                if let Ok(open) = decode::<DidOpenTextDocumentParams>(params) {
                    let uri = open.text_document.uri.as_str().to_string();
                    if self.workspace.root.is_none()
                        && let Some(path) = uri_path(&uri)
                    {
                        self.workspace.root = Workspace::find_root(&path);
                    }
                    let document = Document::new(kind(&uri), open.text_document.text);
                    self.workspace.open.insert(uri, Arc::new(document));
                    self.workspace.stale = true;
                }
                Vec::new()
            }
            "textDocument/didChange" => {
                if let Ok(change) = decode::<DidChangeTextDocumentParams>(params) {
                    let uri = change.text_document.uri.as_str().to_string();
                    if let Some(previous) = self.workspace.open.get(&uri) {
                        let text = changed_text(previous, change.content_changes);
                        let document = Document::new(kind(&uri), text);
                        self.workspace.open.insert(uri, Arc::new(document));
                        self.workspace.stale = true;
                    }
                }
                Vec::new()
            }
            "textDocument/didClose" => {
                if let Ok(close) = decode::<DidCloseTextDocumentParams>(params) {
                    self.workspace
                        .open
                        .shift_remove(close.text_document.uri.as_str());
                    self.workspace.stale = true;
                }
                Vec::new()
            }
            "textDocument/didSave" | "workspace/didChangeWatchedFiles" => {
                self.workspace.files_changed();
                Vec::new()
            }
            "exit" => {
                self.shutdown = true;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Reply {
        match method {
            "initialize" => {
                // A client starts each session afresh, as for another project.
                self.workspace = Workspace::with_checker(match &self.source {
                    Some(source) => Checker::Host(Arc::clone(source)),
                    None => Checker::Own(Default::default()),
                });
                let initialize = decode::<InitializeParams>(params)?;
                #[expect(deprecated, reason = "older clients send only `rootUri`")]
                let root = initialize
                    .workspace_folders
                    .and_then(|folders| folders.into_iter().next().map(|folder| folder.uri))
                    .or(initialize.root_uri);
                if let Some(root) = root.and_then(|root| uri_path(root.as_str())) {
                    self.workspace.root = Some(root);
                    self.workspace.stale = true;
                }
                to_value(json!({
                    "capabilities": capabilities(),
                    "serverInfo": { "name": "donder", "version": env!("CARGO_PKG_VERSION") },
                }))
            }
            "shutdown" => Ok(Value::Null),
            "textDocument/hover" => self.hover(decode(params)?),
            "textDocument/completion" => self.completion(decode(params)?),
            "textDocument/signatureHelp" => self.signature_help(decode(params)?),
            "textDocument/definition" => self.definition(decode(params)?),
            "textDocument/references" => self.references(decode(params)?),
            "textDocument/prepareRename" => self.prepare_rename(decode(params)?),
            "textDocument/rename" => self.rename(decode(params)?),
            "textDocument/formatting" => self.formatting(decode(params)?),
            "textDocument/semanticTokens/full" => self.semantic_tokens(decode(params)?),
            "textDocument/semanticTokens/range" => self.semantic_tokens_range(decode(params)?),
            "textDocument/codeAction" => self.code_actions(decode(params)?),
            "textDocument/documentSymbol" => self.document_symbols(decode(params)?),
            _ => Err((METHOD_NOT_FOUND, format!("`{method}` is not supported"))),
        }
    }

    fn publish(&mut self) -> Vec<Value> {
        let mut by_uri = HashMap::<String, Vec<Diagnostic>>::new();
        for (uri, diagnostics) in self.workspace.project_diagnostics() {
            let Some(document) = self.workspace.document(&uri) else {
                continue;
            };
            let (text, lines) = (document.text(), document.lines());
            by_uri
                .entry(uri)
                .or_default()
                .extend(diagnostics.into_iter().map(|diagnostic| {
                    let range = diagnostic
                        .range
                        .as_ref()
                        .map_or_else(Range::default, |range| {
                            Range::new(
                                lines.character_position(
                                    text,
                                    range.start.line,
                                    range.start.character,
                                ),
                                lines.character_position(text, range.end.line, range.end.character),
                            )
                        });
                    Diagnostic {
                        range,
                        severity: Some(match diagnostic.severity {
                            donder_project_io::IoDiagnosticSeverity::Error => {
                                DiagnosticSeverity::ERROR
                            }
                            donder_project_io::IoDiagnosticSeverity::Warning => {
                                DiagnosticSeverity::WARNING
                            }
                        }),
                        // The code's one spelling is its serialized form.
                        code: serde_json::to_value(diagnostic.code)
                            .and_then(serde_json::from_value)
                            .ok(),
                        source: Some("donder".into()),
                        message: diagnostic.message,
                        data: fix_data(diagnostic.fix),
                        ..Diagnostic::default()
                    }
                }));
        }
        // Open documents outside a project are checked on their own.
        for (uri, document) in &self.workspace.open {
            if self.workspace.in_project(uri) && self.workspace.check.is_some() {
                continue;
            }
            let diagnostics = match document.kind() {
                workspace::Kind::Script => document.script().diagnostics.clone(),
                workspace::Kind::Data => document.parsed().1.clone(),
                workspace::Kind::Other => Vec::new(),
            };
            by_uri
                .entry(uri.clone())
                .or_default()
                .extend(diagnostics.into_iter().map(|diagnostic| Diagnostic {
                    range: document.range(diagnostic.span),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("donder".into()),
                    message: diagnostic.message,
                    data: diagnostic.fix.map(|fix| json!({ "fix": fix })),
                    ..Diagnostic::default()
                }));
        }
        let mut out = Vec::new();
        let previous = std::mem::take(&mut self.workspace.published);
        for stale in previous.iter().filter(|stale| !by_uri.contains_key(*stale)) {
            if let Some(uri) = uri(stale) {
                out.push(notification(
                    "textDocument/publishDiagnostics",
                    PublishDiagnosticsParams::new(uri, Vec::new(), None),
                ));
            }
        }
        for (key, diagnostics) in by_uri {
            if let Some(uri) = uri(&key) {
                out.push(notification(
                    "textDocument/publishDiagnostics",
                    PublishDiagnosticsParams::new(uri, diagnostics, None),
                ));
                self.workspace.published.insert(key);
            }
        }
        out
    }

    /// The document and byte offset of a position.
    fn position(
        &self,
        position: &TextDocumentPositionParams,
    ) -> Option<(String, Arc<Document>, usize)> {
        let uri = position.text_document.uri.as_str().to_string();
        let document = self.workspace.document(&uri)?;
        let offset = document.offset(position.position);
        Some((uri, document, offset))
    }

    fn location(&self, uri: &str, span: donder_language::compiler::TextSpan) -> Option<Location> {
        let document = self.workspace.document(uri)?;
        Some(Location::new(self::uri(uri)?, document.range(span)))
    }

    fn hover(&self, hover: HoverParams) -> Reply {
        let Some((uri, document, offset)) = self.position(&hover.text_document_position_params)
        else {
            return Ok(Value::Null);
        };
        let found = match document.kind() {
            workspace::Kind::Script => {
                let analysis = document.script();
                analysis
                    .target_at(offset)
                    .map(|(span, target)| (span, analysis.hover(target)))
            }
            workspace::Kind::Data => {
                let linked = navigation::target_at(&self.workspace, &uri, &document, offset)
                    .and_then(|target| {
                        let (target_uri, span) = navigation::definition(&self.workspace, &target)?;
                        let target = self.workspace.document(&target_uri)?;
                        let markdown = match target.kind() {
                            workspace::Kind::Script => {
                                let analysis = target.script();
                                let symbol = analysis
                                    .symbols
                                    .iter()
                                    .position(|symbol| symbol.span == span)?;
                                analysis
                                    .hover(donder_language::analysis::ScriptTarget::Symbol(symbol))
                            }
                            _ => data::describe(&target, span)?,
                        };
                        Some((document.token_at(offset)?.span, markdown))
                    });
                linked.or_else(|| {
                    let markdown = data::schema_hover(&document, offset)?;
                    Some((document.token_at(offset)?.span, markdown))
                })
            }
            workspace::Kind::Other => None,
        };
        let Some((span, markdown)) = found else {
            return Ok(Value::Null);
        };
        to_value(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: Some(document.range(span)),
        })
    }

    fn completion(&self, completion: CompletionParams) -> Reply {
        let Some((uri, document, offset)) = self.position(&completion.text_document_position)
        else {
            return Ok(Value::Null);
        };
        let items = match document.kind() {
            workspace::Kind::Script => script_completions(document.text(), offset),
            workspace::Kind::Data => data::completions(&self.workspace, &uri, &document, offset),
            workspace::Kind::Other => Vec::new(),
        };
        to_value(CompletionResponse::Array(
            items.into_iter().enumerate().map(completion_item).collect(),
        ))
    }

    fn signature_help(&self, help: SignatureHelpParams) -> Reply {
        let Some((_, document, offset)) = self.position(&help.text_document_position_params) else {
            return Ok(Value::Null);
        };
        if document.kind() != workspace::Kind::Script {
            return Ok(Value::Null);
        }
        let Some(help) = script_signature(document.text(), offset) else {
            return Ok(Value::Null);
        };
        to_value(LspSignatureHelp {
            signatures: help
                .signatures
                .into_iter()
                .map(|signature| SignatureInformation {
                    label: signature.label,
                    documentation: signature
                        .documentation
                        .map(lsp_types::Documentation::String),
                    parameters: Some(
                        signature
                            .parameters
                            .into_iter()
                            .map(|label| ParameterInformation {
                                label: ParameterLabel::Simple(label),
                                documentation: None,
                            })
                            .collect(),
                    ),
                    active_parameter: None,
                })
                .collect(),
            active_signature: Some(0),
            active_parameter: Some(help.active_parameter as u32),
        })
    }

    fn definition(&self, definition: GotoDefinitionParams) -> Reply {
        let Some((uri, document, offset)) =
            self.position(&definition.text_document_position_params)
        else {
            return Ok(Value::Null);
        };
        let Some(target) = navigation::target_at(&self.workspace, &uri, &document, offset) else {
            return Ok(Value::Null);
        };
        let Some(location) = navigation::definition(&self.workspace, &target)
            .and_then(|(uri, span)| self.location(&uri, span))
        else {
            return Ok(Value::Null);
        };
        to_value(GotoDefinitionResponse::Scalar(location))
    }

    fn references(&self, references: ReferenceParams) -> Reply {
        let Some((uri, document, offset)) = self.position(&references.text_document_position)
        else {
            return Ok(Value::Null);
        };
        let Some(target) = navigation::target_at(&self.workspace, &uri, &document, offset) else {
            return Ok(Value::Null);
        };
        let declaration = navigation::definition(&self.workspace, &target);
        let locations = navigation::occurrences(&self.workspace, &target)
            .into_iter()
            .filter(|found| {
                references.context.include_declaration || Some(found) != declaration.as_ref()
            })
            .filter_map(|(uri, span)| self.location(&uri, span))
            .collect::<Vec<_>>();
        to_value(locations)
    }

    fn prepare_rename(&self, position: TextDocumentPositionParams) -> Reply {
        let Some((uri, document, offset)) = self.position(&position) else {
            return Ok(Value::Null);
        };
        let Some(target) = navigation::target_at(&self.workspace, &uri, &document, offset) else {
            return Err((REQUEST_FAILED, "nothing to rename here".into()));
        };
        let span = navigation::occurrences(&self.workspace, &target)
            .into_iter()
            .find(|(found, span)| found == &uri && span.start <= offset && offset <= span.end)
            .map(|(_, span)| span);
        match span {
            Some(span) => to_value(PrepareRenameResponse::Range(document.range(span))),
            None => Err((REQUEST_FAILED, "nothing to rename here".into())),
        }
    }

    fn rename(&self, rename: RenameParams) -> Reply {
        let Some((uri, document, offset)) = self.position(&rename.text_document_position) else {
            return Ok(Value::Null);
        };
        let Some(target) = navigation::target_at(&self.workspace, &uri, &document, offset) else {
            return Err((REQUEST_FAILED, "nothing to rename here".into()));
        };
        navigation::valid_name(&target, &rename.new_name)
            .map_err(|error| (REQUEST_FAILED, error))?;
        // `WorkspaceEdit` keys its changes by `Uri`, whose parsed form clippy
        // cannot see is never mutated.
        #[allow(clippy::mutable_key_type)]
        let mut changes = HashMap::<Uri, Vec<TextEdit>>::new();
        for (found, span) in navigation::occurrences(&self.workspace, &target) {
            if let Some(location) = self.location(&found, span) {
                changes
                    .entry(location.uri)
                    .or_default()
                    .push(TextEdit::new(location.range, rename.new_name.clone()));
            }
        }
        to_value(WorkspaceEdit::new(changes))
    }

    fn formatting(&self, formatting: DocumentFormattingParams) -> Reply {
        let uri = formatting.text_document.uri.as_str().to_string();
        if kind(&uri) != workspace::Kind::Data {
            return to_value(Vec::<TextEdit>::new());
        }
        let Some(document) = self.workspace.document(&uri) else {
            return Ok(Value::Null);
        };
        let Some(formatted) = data::format(&document) else {
            return Ok(Value::Null);
        };
        if formatted == document.text() {
            return to_value(Vec::<TextEdit>::new());
        }
        let whole = Range::new(
            document.position(0),
            document.position(document.text().len()),
        );
        to_value(vec![TextEdit::new(whole, formatted)])
    }

    fn semantic_tokens(&self, tokens: SemanticTokensParams) -> Reply {
        let uri = tokens.text_document.uri.as_str().to_string();
        let Some(document) = self.workspace.document(&uri) else {
            return Ok(Value::Null);
        };
        to_value(SemanticTokensResult::Tokens(tokens::encode(
            &document,
            document.tokens(),
        )))
    }

    fn semantic_tokens_range(&self, tokens: SemanticTokensRangeParams) -> Reply {
        let uri = tokens.text_document.uri.as_str().to_string();
        let Some(document) = self.workspace.document(&uri) else {
            return Ok(Value::Null);
        };
        let (start, end) = (
            document.offset(tokens.range.start),
            document.offset(tokens.range.end),
        );
        let all = document.tokens();
        let first = all.partition_point(|token| token.span.end < start);
        let last = all
            .partition_point(|token| token.span.start < end)
            .max(first);
        to_value(SemanticTokensRangeResult::Tokens(tokens::encode(
            &document,
            &all[first..last],
        )))
    }

    fn code_actions(&self, actions: CodeActionParams) -> Reply {
        let uri = actions.text_document.uri;
        let fixes = actions
            .context
            .diagnostics
            .into_iter()
            .filter_map(|diagnostic| {
                if diagnostic.data.as_ref()?.get("include").is_some() {
                    return self.include_action(&uri, diagnostic);
                }
                let fix = diagnostic.data.as_ref()?.get("fix")?.as_str()?.to_string();
                Some(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("Write `{fix}`"),
                    kind: Some(CodeActionKind::QUICKFIX),
                    edit: Some(WorkspaceEdit::new(HashMap::from([(
                        uri.clone(),
                        vec![TextEdit::new(diagnostic.range, fix)],
                    )]))),
                    is_preferred: Some(true),
                    diagnostics: Some(vec![diagnostic]),
                    ..CodeAction::default()
                }))
            })
            .collect::<Vec<_>>();
        to_value(fixes)
    }

    /// Import an unreferenced document from the project root, by rewriting
    /// the root's text.
    fn include_action(
        &self,
        document: &Uri,
        diagnostic: Diagnostic,
    ) -> Option<CodeActionOrCommand> {
        let relative = self.workspace.relative(document.as_str())?;
        let inclusion = self.workspace.check.as_ref()?.diagnostics.iter().find_map(
            |candidate| match &candidate.fix {
                Some(IoFix::Include(inclusion)) if candidate.path == relative => Some(inclusion),
                _ => None,
            },
        )?;
        let root_uri = self
            .workspace
            .uri(camino::Utf8Path::new(PROJECT_ROOT_FILE))?;
        let root = self.workspace.document(&root_uri)?;
        let included = include_document(root.text(), inclusion).ok()?;
        let range = Range::new(
            lsp_types::Position::new(0, 0),
            root.position(root.text().len()),
        );
        // `WorkspaceEdit` keys its changes by `Uri`, whose parsed form clippy
        // cannot see is never mutated.
        #[allow(clippy::mutable_key_type)]
        let changes = HashMap::from([(uri(&root_uri)?, vec![TextEdit::new(range, included)])]);
        Some(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Import it in {PROJECT_ROOT_FILE}"),
            kind: Some(CodeActionKind::QUICKFIX),
            edit: Some(WorkspaceEdit::new(changes)),
            is_preferred: Some(true),
            diagnostics: Some(vec![diagnostic]),
            ..CodeAction::default()
        }))
    }

    fn document_symbols(&self, symbols: DocumentSymbolParams) -> Reply {
        let uri = symbols.text_document.uri.as_str().to_string();
        let Some(document) = self.workspace.document(&uri) else {
            return Ok(Value::Null);
        };
        #[expect(
            deprecated,
            reason = "`deprecated` is a required field of `DocumentSymbol`"
        )]
        let symbol = |name: String, detail: Option<String>, kind: Kind, span, extent, children| {
            DocumentSymbol {
                name,
                detail,
                kind,
                tags: None,
                deprecated: None,
                range: document.range(extent),
                selection_range: document.range(span),
                children: Some(children),
            }
        };
        let result = match document.kind() {
            workspace::Kind::Script => {
                let analysis = document.script();
                fn build(
                    analysis: &donder_language::analysis::ScriptAnalysis,
                    parent: Option<usize>,
                    symbol: &dyn Fn(
                        String,
                        Option<String>,
                        Kind,
                        donder_language::compiler::TextSpan,
                        donder_language::compiler::TextSpan,
                        Vec<DocumentSymbol>,
                    ) -> DocumentSymbol,
                ) -> Vec<DocumentSymbol> {
                    analysis
                        .symbols
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| entry.parent == parent)
                        .filter_map(|(index, entry)| {
                            let kind = match entry.kind {
                                SymbolKind::Effect | SymbolKind::Operator => Kind::CLASS,
                                SymbolKind::Function => Kind::FUNCTION,
                                SymbolKind::Param => Kind::PROPERTY,
                                SymbolKind::Input => Kind::FIELD,
                                SymbolKind::EnumOption => Kind::ENUM_MEMBER,
                                _ => return None,
                            };
                            Some(symbol(
                                entry.name.clone(),
                                Some(entry.detail.clone()),
                                kind,
                                entry.span,
                                entry.extent,
                                build(analysis, Some(index), symbol),
                            ))
                        })
                        .collect()
                }
                build(analysis, None, &symbol)
            }
            workspace::Kind::Data => {
                fn build(
                    outline: Vec<data::Outline>,
                    symbol: &dyn Fn(
                        String,
                        Option<String>,
                        Kind,
                        donder_language::compiler::TextSpan,
                        donder_language::compiler::TextSpan,
                        Vec<DocumentSymbol>,
                    ) -> DocumentSymbol,
                ) -> Vec<DocumentSymbol> {
                    outline
                        .into_iter()
                        .map(|entry| {
                            let children = build(entry.children, symbol);
                            symbol(
                                entry.name,
                                Some(entry.ty),
                                Kind::OBJECT,
                                entry.span,
                                entry.extent,
                                children,
                            )
                        })
                        .collect()
                }
                build(data::outline(&document), &symbol)
            }
            workspace::Kind::Other => Vec::new(),
        };
        to_value(DocumentSymbolResponse::Nested(result))
    }
}

fn completion_item((order, completion): (usize, Completion)) -> CompletionItem {
    let snippet = completion
        .insert
        .as_ref()
        .is_some_and(|insert| insert.contains('$'));
    CompletionItem {
        label: completion.label,
        kind: Some(match completion.kind {
            CompletionKind::Keyword => CompletionItemKind::KEYWORD,
            CompletionKind::Type => CompletionItemKind::CLASS,
            CompletionKind::Function => CompletionItemKind::FUNCTION,
            CompletionKind::Variable => CompletionItemKind::VARIABLE,
            CompletionKind::Field => CompletionItemKind::FIELD,
            CompletionKind::EnumMember => CompletionItemKind::ENUM_MEMBER,
            CompletionKind::Reference => CompletionItemKind::REFERENCE,
            CompletionKind::Snippet => CompletionItemKind::SNIPPET,
        }),
        detail: completion.detail,
        documentation: completion.documentation.map(|text| {
            lsp_types::Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: text,
            })
        }),
        // Keep the order completion produced: next fields first.
        sort_text: Some(format!("{order:04}")),
        insert_text: completion.insert,
        insert_text_format: Some(if snippet {
            InsertTextFormat::SNIPPET
        } else {
            InsertTextFormat::PLAIN_TEXT
        }),
        ..CompletionItem::default()
    }
}

fn capabilities() -> ServerCapabilities {
    use lsp_types::*;
    ServerCapabilities {
        position_encoding: Some(PositionEncodingKind::UTF16),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                ..TextDocumentSyncOptions::default()
            },
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), ":".into(), "{".into(), " ".into()]),
            ..CompletionOptions::default()
        }),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            ..SignatureHelpOptions::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: tokens::legend(),
                full: Some(SemanticTokensFullOptions::Bool(true)),
                range: Some(true),
                work_done_progress_options: WorkDoneProgressOptions::default(),
            },
        )),
        ..ServerCapabilities::default()
    }
}

/// A document's text after a `didChange`'s changes, applied in order. A change
/// without a range replaces the whole text.
fn changed_text(
    previous: &Document,
    changes: Vec<lsp_types::TextDocumentContentChangeEvent>,
) -> String {
    let mut text = previous.text().to_string();
    // The previous document's line index holds until the first change.
    let mut lines = Some(previous.lines());
    for change in changes {
        match change.range {
            Some(range) => {
                let fresh;
                let index = match lines.take() {
                    Some(index) => index,
                    None => {
                        fresh = LineIndex::new(&text);
                        &fresh
                    }
                };
                let start = index.offset(&text, range.start);
                let end = index.offset(&text, range.end).max(start);
                text.replace_range(start..end, &change.text);
            }
            None => {
                text = change.text;
                lines = None;
            }
        }
    }
    text
}

/// `text` with LSP edits applied; positions count UTF-16 code units.
pub fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
    let lines = LineIndex::new(text);
    let mut spans = edits
        .iter()
        .map(|edit| {
            (
                lines.offset(text, edit.range.start),
                lines.offset(text, edit.range.end),
                edit.new_text.as_str(),
            )
        })
        .collect::<Vec<_>>();
    spans.sort_by_key(|(start, ..)| std::cmp::Reverse(*start));
    let mut result = text.to_string();
    for (start, end, replacement) in spans {
        result.replace_range(start..end.max(start), replacement);
    }
    result
}

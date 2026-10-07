//! The Donder language server, independent of how messages travel: a host
//! passes each JSON-RPC message to [`Server::handle`], sends what it returns,
//! and calls [`Server::idle`] when messages pause, which rechecks the project
//! and publishes diagnostics. Hosts: `donder lsp` over stdio, the desktop over
//! Tauri, and the website in a worker.
mod data;
mod navigation;
mod text;
mod tokens;
mod workspace;

use std::collections::HashMap;

use donder_language::analysis::{
    Completion, CompletionKind, SymbolKind, analyze_script, data_tokens, script_completions,
    script_signature,
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
    RenameParams, SemanticTokensParams, SemanticTokensResult, ServerCapabilities,
    SignatureHelp as LspSignatureHelp, SignatureHelpParams, SignatureInformation,
    SymbolKind as Kind, TextDocumentPositionParams, TextEdit, Uri, WorkspaceEdit,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::text::LineIndex;
pub use crate::text::{file_uri, uri_path};
pub use crate::workspace::DocumentSource;
use crate::workspace::{Workspace, kind};

/// How long hosts wait after the last message before calling [`Server::idle`].
pub const IDLE_DELAY_MS: u64 = 150;

/// One language server session.
#[derive(Default)]
pub struct Server {
    workspace: Workspace,
    /// Kept across sessions: each `initialize` starts a fresh workspace.
    source: Option<std::sync::Arc<dyn DocumentSource + Sync>>,
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

    /// A server that reads the host's copies of documents before disk.
    pub fn with_source(source: std::sync::Arc<dyn DocumentSource + Sync>) -> Self {
        Self {
            source: Some(source),
            ..Self::default()
        }
    }

    /// Whether the client asked to exit.
    pub fn exited(&self) -> bool {
        self.shutdown
    }

    /// Handle one message; returns the messages to send.
    pub fn handle(&mut self, message: Value) -> Vec<Value> {
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match message.get("id").cloned() {
            Some(id) if !method.is_empty() => {
                let reply = self.request(&method, params);
                let mut out = vec![match reply {
                    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Err((code, message)) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": code, "message": message },
                    }),
                }];
                // The first request after edits sees fresh diagnostics too.
                out.extend(self.idle());
                out
            }
            Some(_) => Vec::new(),
            None => self.notification(&method, params),
        }
    }

    /// Recheck after edits and publish diagnostics. Hosts call this when
    /// messages pause, so typing does not recheck on every keystroke.
    pub fn idle(&mut self) -> Vec<Value> {
        if !self.workspace.stale {
            return Vec::new();
        }
        self.workspace.refresh();
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
                    self.workspace.open.insert(uri, open.text_document.text);
                    self.workspace.stale = true;
                }
                Vec::new()
            }
            "textDocument/didChange" => {
                if let Ok(change) = decode::<DidChangeTextDocumentParams>(params)
                    && let Some(last) = change.content_changes.into_iter().last()
                {
                    // Full document sync: each change is the whole text.
                    self.workspace
                        .open
                        .insert(change.text_document.uri.as_str().to_string(), last.text);
                    self.workspace.stale = true;
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
                self.workspace.stale = true;
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
                self.workspace = Workspace {
                    source: self.source.clone(),
                    ..Workspace::default()
                };
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
            "textDocument/codeAction" => self.code_actions(decode(params)?),
            "textDocument/documentSymbol" => self.document_symbols(decode(params)?),
            _ => Err((METHOD_NOT_FOUND, format!("`{method}` is not supported"))),
        }
    }

    fn publish(&mut self) -> Vec<Value> {
        let mut by_uri = HashMap::<String, Vec<Diagnostic>>::new();
        for (uri, diagnostics) in self.workspace.project_diagnostics() {
            let Some(text) = self.workspace.text(&uri) else {
                continue;
            };
            let lines = LineIndex::new(&text);
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
                                    &text,
                                    range.start.line,
                                    range.start.character,
                                ),
                                lines.character_position(
                                    &text,
                                    range.end.line,
                                    range.end.character,
                                ),
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
                        code: Some(lsp_types::NumberOrString::String(
                            diagnostic.code.as_str().to_string(),
                        )),
                        source: Some("donder".into()),
                        message: diagnostic.message,
                        data: fix_data(diagnostic.fix),
                        ..Diagnostic::default()
                    }
                }));
        }
        // Open documents outside a project are checked on their own.
        for (uri, text) in &self.workspace.open {
            if self.workspace.in_project(uri) && self.workspace.check.is_some() {
                continue;
            }
            let lines = LineIndex::new(text);
            let diagnostics = match kind(uri) {
                workspace::Kind::Script => analyze_script(text).diagnostics,
                workspace::Kind::Data => donder_language::data::parse(text).1,
                workspace::Kind::Other => Vec::new(),
            };
            by_uri
                .entry(uri.clone())
                .or_default()
                .extend(diagnostics.into_iter().map(|diagnostic| Diagnostic {
                    range: lines.range(text, diagnostic.span),
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

    /// The text, line index and byte offset of a position.
    fn position(
        &self,
        position: &TextDocumentPositionParams,
    ) -> Option<(String, String, LineIndex, usize)> {
        let uri = position.text_document.uri.as_str().to_string();
        let text = self.workspace.text(&uri)?;
        let lines = LineIndex::new(&text);
        let offset = lines.offset(&text, position.position);
        Some((uri, text, lines, offset))
    }

    fn location(&self, uri: &str, span: donder_language::dsl::TextSpan) -> Option<Location> {
        let text = self.workspace.text(uri)?;
        Some(Location::new(
            self::uri(uri)?,
            LineIndex::new(&text).range(&text, span),
        ))
    }

    fn hover(&self, hover: HoverParams) -> Reply {
        let Some((uri, text, lines, offset)) = self.position(&hover.text_document_position_params)
        else {
            return Ok(Value::Null);
        };
        let found = match kind(&uri) {
            workspace::Kind::Script => {
                let analysis = analyze_script(&text);
                analysis
                    .target_at(offset)
                    .map(|(span, target)| (span, analysis.hover(target)))
            }
            workspace::Kind::Data => {
                let linked = navigation::target_at(&self.workspace, &uri, &text, offset).and_then(
                    |target| {
                        let (target_uri, span) = navigation::definition(&self.workspace, &target)?;
                        let target_text = self.workspace.text(&target_uri)?;
                        let markdown = match kind(&target_uri) {
                            workspace::Kind::Script => {
                                let analysis = analyze_script(&target_text);
                                let symbol = analysis
                                    .symbols
                                    .iter()
                                    .position(|symbol| symbol.span == span)?;
                                analysis
                                    .hover(donder_language::analysis::ScriptTarget::Symbol(symbol))
                            }
                            _ => data::describe(&target_text, span)?,
                        };
                        let token = data_tokens(&text)
                            .into_iter()
                            .find(|token| token.span.start <= offset && offset <= token.span.end)?;
                        Some((token.span, markdown))
                    },
                );
                linked.or_else(|| {
                    let markdown = data::schema_hover(&text, offset)?;
                    let token = data_tokens(&text)
                        .into_iter()
                        .find(|token| token.span.start <= offset && offset <= token.span.end)?;
                    Some((token.span, markdown))
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
            range: Some(lines.range(&text, span)),
        })
    }

    fn completion(&self, completion: CompletionParams) -> Reply {
        let Some((uri, text, _, offset)) = self.position(&completion.text_document_position) else {
            return Ok(Value::Null);
        };
        let items = match kind(&uri) {
            workspace::Kind::Script => script_completions(&text, offset),
            workspace::Kind::Data => data::completions(&self.workspace, &uri, &text, offset),
            workspace::Kind::Other => Vec::new(),
        };
        to_value(CompletionResponse::Array(
            items.into_iter().enumerate().map(completion_item).collect(),
        ))
    }

    fn signature_help(&self, help: SignatureHelpParams) -> Reply {
        let Some((uri, text, _, offset)) = self.position(&help.text_document_position_params)
        else {
            return Ok(Value::Null);
        };
        if kind(&uri) != workspace::Kind::Script {
            return Ok(Value::Null);
        }
        let Some(help) = script_signature(&text, offset) else {
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
        let Some((uri, text, _, offset)) = self.position(&definition.text_document_position_params)
        else {
            return Ok(Value::Null);
        };
        let Some(target) = navigation::target_at(&self.workspace, &uri, &text, offset) else {
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
        let Some((uri, text, _, offset)) = self.position(&references.text_document_position) else {
            return Ok(Value::Null);
        };
        let Some(target) = navigation::target_at(&self.workspace, &uri, &text, offset) else {
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
        let Some((uri, text, lines, offset)) = self.position(&position) else {
            return Ok(Value::Null);
        };
        let Some(target) = navigation::target_at(&self.workspace, &uri, &text, offset) else {
            return Err((REQUEST_FAILED, "nothing to rename here".into()));
        };
        let span = navigation::occurrences(&self.workspace, &target)
            .into_iter()
            .find(|(found, span)| found == &uri && span.start <= offset && offset <= span.end)
            .map(|(_, span)| span);
        match span {
            Some(span) => to_value(PrepareRenameResponse::Range(lines.range(&text, span))),
            None => Err((REQUEST_FAILED, "nothing to rename here".into())),
        }
    }

    fn rename(&self, rename: RenameParams) -> Reply {
        let Some((uri, text, _, offset)) = self.position(&rename.text_document_position) else {
            return Ok(Value::Null);
        };
        let Some(target) = navigation::target_at(&self.workspace, &uri, &text, offset) else {
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
        let Some(text) = self.workspace.text(&uri) else {
            return Ok(Value::Null);
        };
        let Some(formatted) = data::format(&text) else {
            return Ok(Value::Null);
        };
        if formatted == text {
            return to_value(Vec::<TextEdit>::new());
        }
        let lines = LineIndex::new(&text);
        let whole = Range::new(lines.position(&text, 0), lines.position(&text, text.len()));
        to_value(vec![TextEdit::new(whole, formatted)])
    }

    fn semantic_tokens(&self, tokens: SemanticTokensParams) -> Reply {
        let uri = tokens.text_document.uri.as_str().to_string();
        let Some(text) = self.workspace.text(&uri) else {
            return Ok(Value::Null);
        };
        let found = match kind(&uri) {
            workspace::Kind::Script => analyze_script(&text).tokens,
            workspace::Kind::Data => data_tokens(&text),
            workspace::Kind::Other => Vec::new(),
        };
        to_value(SemanticTokensResult::Tokens(tokens::encode(&text, &found)))
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
        let inclusion = self
            .workspace
            .check
            .as_ref()?
            .report
            .diagnostics
            .iter()
            .find_map(|candidate| match &candidate.fix {
                Some(IoFix::Include(inclusion)) if candidate.path == relative => Some(inclusion),
                _ => None,
            })?;
        let root_uri = self
            .workspace
            .uri(camino::Utf8Path::new(PROJECT_ROOT_FILE))?;
        let text = self.workspace.text(&root_uri)?;
        let included = include_document(&text, inclusion).ok()?;
        let range = Range::new(
            lsp_types::Position::new(0, 0),
            LineIndex::new(&text).position(&text, text.len()),
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
        let Some(text) = self.workspace.text(&uri) else {
            return Ok(Value::Null);
        };
        let lines = LineIndex::new(&text);
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
                range: lines.range(&text, extent),
                selection_range: lines.range(&text, span),
                children: Some(children),
            }
        };
        let result = match kind(&uri) {
            workspace::Kind::Script => {
                let analysis = analyze_script(&text);
                fn build(
                    analysis: &donder_language::analysis::ScriptAnalysis,
                    parent: Option<usize>,
                    symbol: &dyn Fn(
                        String,
                        Option<String>,
                        Kind,
                        donder_language::dsl::TextSpan,
                        donder_language::dsl::TextSpan,
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
                build(&analysis, None, &symbol)
            }
            workspace::Kind::Data => {
                fn build(
                    outline: Vec<data::Outline>,
                    symbol: &dyn Fn(
                        String,
                        Option<String>,
                        Kind,
                        donder_language::dsl::TextSpan,
                        donder_language::dsl::TextSpan,
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
                build(data::outline(&text), &symbol)
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
                change: Some(TextDocumentSyncKind::FULL),
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
                range: None,
                work_done_progress_options: WorkDoneProgressOptions::default(),
            },
        )),
        ..ServerCapabilities::default()
    }
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

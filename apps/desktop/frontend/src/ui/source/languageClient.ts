// A thin language server client for Monaco: it syncs documents and maps each
// LSP feature onto Monaco's providers. The host supplies the transport and
// applies edits to documents Monaco has not opened.
import { DATA_LANGUAGE, SCRIPT_LANGUAGE, monaco } from "./monaco";

/** How messages travel: Tauri on the desktop, a worker on the website. */
export type LanguageTransport = {
  send: (message: string) => void;
  /** Receive every message; returns a function that stops receiving. */
  listen: (listener: (message: string) => void) => () => void;
};

type Position = { line: number; character: number };
type Range = { start: Position; end: Position };
type TextEdit = { range: Range; newText: string };
type Location = { uri: string; range: Range };
type Diagnostic = { range: Range; severity?: number; code?: string | number; message: string; data?: unknown };
type MarkupContent = { kind: string; value: string } | string;
type DocumentSymbol = { name: string; detail?: string; kind: number; range: Range; selectionRange: Range; children?: DocumentSymbol[] };

/** Edits to one document Monaco has no model for, such as a rename's. */
export type ExternalEdits = { uri: string; edits: TextEdit[] };

export type LanguageClientOptions = {
  rootUri: string | null;
  /** Apply edits to documents without a Monaco model. */
  applyExternalEdits: (edits: ExternalEdits[]) => Promise<void>;
  /** Show the server's diagnostics as markers; hosts with their own diagnostics turn this off. */
  diagnostics: boolean;
};

const LANGUAGES = [DATA_LANGUAGE, SCRIPT_LANGUAGE];
const MARKER_OWNER = "donder";

function fromPosition(position: monaco.IPosition): Position {
  return { line: position.lineNumber - 1, character: position.column - 1 };
}

function toRange(range: Range): monaco.IRange {
  return {
    startLineNumber: range.start.line + 1,
    startColumn: range.start.character + 1,
    endLineNumber: range.end.line + 1,
    endColumn: range.end.character + 1
  };
}

function fromRange(range: monaco.IRange): Range {
  return {
    start: { line: range.startLineNumber - 1, character: range.startColumn - 1 },
    end: { line: range.endLineNumber - 1, character: range.endColumn - 1 }
  };
}

function markdown(content: MarkupContent): monaco.IMarkdownString {
  return { value: typeof content === "string" ? content : content.value };
}

/** URIs as Monaco spells them, so the server's and the editor's compare equal. */
function canonical(uri: string): string {
  return monaco.Uri.parse(uri).toString();
}

const COMPLETION_KINDS: Record<number, monaco.languages.CompletionItemKind> = {
  3: monaco.languages.CompletionItemKind.Function,
  5: monaco.languages.CompletionItemKind.Field,
  6: monaco.languages.CompletionItemKind.Variable,
  7: monaco.languages.CompletionItemKind.Class,
  14: monaco.languages.CompletionItemKind.Keyword,
  15: monaco.languages.CompletionItemKind.Snippet,
  18: monaco.languages.CompletionItemKind.Reference,
  20: monaco.languages.CompletionItemKind.EnumMember
};

export class LanguageClient {
  private nextId = 1;
  private readonly pending = new Map<number, { resolve: (result: unknown) => void; reject: (error: Error) => void }>();
  private readonly diagnostics = new Map<string, Diagnostic[]>();
  private readonly attached = new Map<string, monaco.IDisposable>();
  private readonly providers: monaco.IDisposable[] = [];
  private readonly stopListening: () => void;
  private readonly ready: Promise<void>;

  constructor(private readonly transport: LanguageTransport, private readonly options: LanguageClientOptions) {
    this.stopListening = transport.listen((text) => { this.receive(text); });
    this.ready = this.initialize();
  }

  dispose() {
    this.stopListening();
    for (const provider of this.providers) provider.dispose();
    for (const attachment of this.attached.values()) attachment.dispose();
    this.attached.clear();
    for (const model of monaco.editor.getModels()) monaco.editor.setModelMarkers(model, MARKER_OWNER, []);
    for (const { reject } of this.pending.values()) reject(new Error("The language server session ended."));
    this.pending.clear();
  }

  /** Keep the server's copy of `model` current while it lives. */
  attach(model: monaco.editor.ITextModel) {
    const uri = model.uri.toString();
    if (this.attached.has(uri)) return;
    const open = () => {
      this.notify("textDocument/didOpen", {
        textDocument: { uri, languageId: model.getLanguageId(), version: model.getVersionId(), text: model.getValue() }
      });
      this.showDiagnostics(uri);
    };
    void this.ready.then(open);
    const changes = model.onDidChangeContent(() => {
      void this.ready.then(() => {
        this.notify("textDocument/didChange", {
          textDocument: { uri, version: model.getVersionId() },
          contentChanges: [{ text: model.getValue() }]
        });
      });
    });
    const disposal = model.onWillDispose(() => { this.detach(uri); });
    this.attached.set(uri, {
      dispose: () => {
        changes.dispose();
        disposal.dispose();
        void this.ready.then(() => { this.notify("textDocument/didClose", { textDocument: { uri } }); });
      }
    });
  }

  detach(uri: string) {
    this.attached.get(uri)?.dispose();
    this.attached.delete(uri);
  }

  /** Files changed outside the editor, such as a save or a GUI edit. */
  filesChanged() {
    void this.ready.then(() => { this.notify("workspace/didChangeWatchedFiles", { changes: [] }); });
  }

  private send(message: object) {
    this.transport.send(JSON.stringify({ jsonrpc: "2.0", ...message }));
  }

  private notify(method: string, params: unknown) {
    this.send({ method, params });
  }

  private request<T>(method: string, params: unknown): Promise<T> {
    const id = this.nextId;
    this.nextId += 1;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: (result) => { resolve(result as T); }, reject });
      this.send({ id, method, params });
    });
  }

  private receive(text: string) {
    const message = JSON.parse(text) as { id?: number; method?: string; params?: unknown; result?: unknown; error?: { message: string } };
    if (message.method === undefined && message.id !== undefined) {
      const pending = this.pending.get(message.id);
      if (pending === undefined) return;
      this.pending.delete(message.id);
      if (message.error !== undefined) pending.reject(new Error(message.error.message));
      else pending.resolve(message.result ?? null);
      return;
    }
    if (message.method === "textDocument/publishDiagnostics" && this.options.diagnostics) {
      const params = message.params as { uri: string; diagnostics: Diagnostic[] };
      const uri = canonical(params.uri);
      this.diagnostics.set(uri, params.diagnostics);
      this.showDiagnostics(uri);
    }
  }

  private showDiagnostics(uri: string) {
    const model = monaco.editor.getModel(monaco.Uri.parse(uri));
    if (model === null || !this.options.diagnostics) return;
    monaco.editor.setModelMarkers(model, MARKER_OWNER, (this.diagnostics.get(uri) ?? []).map((diagnostic) => ({
      ...toRange(diagnostic.range),
      severity: diagnostic.severity === 2 ? monaco.MarkerSeverity.Warning : monaco.MarkerSeverity.Error,
      message: diagnostic.message,
      ...(diagnostic.code === undefined ? {} : { code: String(diagnostic.code) })
    })));
  }

  private async initialize() {
    const result = await this.request<{ capabilities: { semanticTokensProvider?: { legend: monaco.languages.SemanticTokensLegend } } }>("initialize", {
      processId: null,
      rootUri: this.options.rootUri,
      capabilities: {}
    });
    this.notify("initialized", {});
    const legend = result.capabilities.semanticTokensProvider?.legend ?? { tokenTypes: [], tokenModifiers: [] };
    for (const language of LANGUAGES) this.register(language, legend);
  }

  /** Apply a workspace edit: open documents in Monaco, the rest through the host. */
  private async workspaceEdits(changes: Record<string, TextEdit[]>): Promise<monaco.languages.IWorkspaceTextEdit[]> {
    const local: monaco.languages.IWorkspaceTextEdit[] = [];
    const external: ExternalEdits[] = [];
    for (const [uri, edits] of Object.entries(changes)) {
      const resource = monaco.Uri.parse(uri);
      if (monaco.editor.getModel(resource) !== null) {
        for (const edit of edits) local.push({ resource, textEdit: { range: toRange(edit.range), text: edit.newText }, versionId: undefined });
      } else {
        external.push({ uri, edits });
      }
    }
    if (external.length > 0) await this.options.applyExternalEdits(external);
    return local;
  }

  private register(language: string, legend: monaco.languages.SemanticTokensLegend) {
    const document = (model: monaco.editor.ITextModel) => ({ uri: model.uri.toString() });
    const at = (model: monaco.editor.ITextModel, position: monaco.Position) => ({ textDocument: document(model), position: fromPosition(position) });
    const languages = monaco.languages;
    this.providers.push(
      languages.registerHoverProvider(language, {
        provideHover: async (model, position) => {
          const hover = await this.request<{ contents: MarkupContent; range?: Range } | null>("textDocument/hover", at(model, position));
          if (hover === null) return null;
          return { contents: [markdown(hover.contents)], ...(hover.range === undefined ? {} : { range: toRange(hover.range) }) };
        }
      }),
      languages.registerCompletionItemProvider(language, {
        triggerCharacters: [".", ":", "{", " "],
        provideCompletionItems: async (model, position) => {
          const items = await this.request<{ label: string; kind?: number; detail?: string; documentation?: MarkupContent; sortText?: string; insertText?: string; insertTextFormat?: number }[] | null>(
            "textDocument/completion", at(model, position));
          const word = model.getWordUntilPosition(position);
          const range = { startLineNumber: position.lineNumber, endLineNumber: position.lineNumber, startColumn: word.startColumn, endColumn: word.endColumn };
          return {
            suggestions: (items ?? []).map((item) => ({
              label: item.label,
              kind: COMPLETION_KINDS[item.kind ?? 0] ?? monaco.languages.CompletionItemKind.Text,
              insertText: item.insertText ?? item.label,
              ...(item.insertTextFormat === 2 ? { insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet } : {}),
              ...(item.detail === undefined ? {} : { detail: item.detail }),
              ...(item.documentation === undefined ? {} : { documentation: markdown(item.documentation) }),
              ...(item.sortText === undefined ? {} : { sortText: item.sortText }),
              range
            }))
          };
        }
      }),
      languages.registerSignatureHelpProvider(language, {
        signatureHelpTriggerCharacters: ["(", ","],
        provideSignatureHelp: async (model, position) => {
          const help = await this.request<{ signatures: { label: string; documentation?: MarkupContent; parameters?: { label: string }[] }[]; activeSignature?: number; activeParameter?: number } | null>(
            "textDocument/signatureHelp", at(model, position));
          if (help === null) return null;
          return {
            value: {
              signatures: help.signatures.map((signature) => ({
                label: signature.label,
                ...(signature.documentation === undefined ? {} : { documentation: markdown(signature.documentation) }),
                parameters: (signature.parameters ?? []).map((parameter) => ({ label: parameter.label }))
              })),
              activeSignature: help.activeSignature ?? 0,
              activeParameter: help.activeParameter ?? 0
            },
            dispose: () => {}
          };
        }
      }),
      languages.registerDefinitionProvider(language, {
        provideDefinition: async (model, position) => {
          const location = await this.request<Location | null>("textDocument/definition", at(model, position));
          return location === null ? null : { uri: monaco.Uri.parse(location.uri), range: toRange(location.range) };
        }
      }),
      languages.registerReferenceProvider(language, {
        provideReferences: async (model, position, context) => {
          const locations = await this.request<Location[] | null>("textDocument/references", { ...at(model, position), context });
          return (locations ?? []).map((location) => ({ uri: monaco.Uri.parse(location.uri), range: toRange(location.range) }));
        }
      }),
      languages.registerRenameProvider(language, {
        resolveRenameLocation: async (model, position) => {
          try {
            const range = await this.request<Range>("textDocument/prepareRename", at(model, position));
            const resolved = toRange(range);
            return { range: resolved, text: model.getValueInRange(resolved) };
          } catch (error) {
            return { range: new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column), text: "", rejectReason: String(error instanceof Error ? error.message : error) };
          }
        },
        provideRenameEdits: async (model, position, newName) => {
          const edit = await this.request<{ changes?: Record<string, TextEdit[]> } | null>("textDocument/rename", { ...at(model, position), newName });
          return { edits: await this.workspaceEdits(edit?.changes ?? {}) };
        }
      }),
      languages.registerDocumentFormattingEditProvider(language, {
        provideDocumentFormattingEdits: async (model, options) => {
          const edits = await this.request<TextEdit[] | null>("textDocument/formatting", {
            textDocument: document(model),
            options: { tabSize: options.tabSize, insertSpaces: options.insertSpaces }
          });
          return (edits ?? []).map((edit) => ({ range: toRange(edit.range), text: edit.newText }));
        }
      }),
      languages.registerCodeActionProvider(language, {
        provideCodeActions: async (model, range) => {
          const uri = model.uri.toString();
          const diagnostics = (this.diagnostics.get(uri) ?? []).filter((diagnostic) =>
            monaco.Range.areIntersectingOrTouching(toRange(diagnostic.range), range));
          const actions = await this.request<{ title: string; kind?: string; isPreferred?: boolean; edit?: { changes?: Record<string, TextEdit[]> } }[] | null>(
            "textDocument/codeAction", { textDocument: { uri }, range: fromRange(range), context: { diagnostics } });
          const result: monaco.languages.CodeAction[] = [];
          for (const action of actions ?? []) {
            result.push({
              title: action.title,
              ...(action.kind === undefined ? {} : { kind: action.kind }),
              ...(action.isPreferred === undefined ? {} : { isPreferred: action.isPreferred }),
              edit: { edits: await this.workspaceEdits(action.edit?.changes ?? {}) }
            });
          }
          return { actions: result, dispose: () => {} };
        }
      }),
      languages.registerDocumentSymbolProvider(language, {
        provideDocumentSymbols: async (model) => {
          const symbols = await this.request<DocumentSymbol[] | null>("textDocument/documentSymbol", { textDocument: document(model) });
          const convert = (symbol: DocumentSymbol): monaco.languages.DocumentSymbol => ({
            name: symbol.name,
            detail: symbol.detail ?? "",
            kind: symbol.kind - 1,
            tags: [],
            range: toRange(symbol.range),
            selectionRange: toRange(symbol.selectionRange),
            children: (symbol.children ?? []).map(convert)
          });
          return (symbols ?? []).map(convert);
        }
      }),
      languages.registerDocumentSemanticTokensProvider(language, {
        getLegend: () => legend,
        provideDocumentSemanticTokens: async (model) => {
          const tokens = await this.request<{ data: number[] } | null>("textDocument/semanticTokens/full", { textDocument: document(model) });
          const data = new Uint32Array(tokens?.data ?? []);
          this.tokens.set(model.uri.toString(), { legend, data });
          for (const listener of this.tokenListeners) listener(model.uri.toString());
          return { data };
        },
        releaseDocumentSemanticTokens: () => {}
      })
    );
  }

  /** The last semantic tokens of each document, for hosts that read colors. */
  readonly tokens = new Map<string, { legend: monaco.languages.SemanticTokensLegend; data: Uint32Array }>();
  private readonly tokenListeners = new Set<(uri: string) => void>();

  /** Called with a document's URI whenever its tokens change. */
  onTokens(listener: (uri: string) => void): monaco.IDisposable {
    this.tokenListeners.add(listener);
    return { dispose: () => { this.tokenListeners.delete(listener); } };
  }
}

// Monaco with the features Donder's editors use, the Donder languages, and a
// theme read from the CSS tokens. Highlighting comes only from the language
// server's semantic tokens.
import * as monaco from "monaco-editor/editor/editor.api.js";
import "monaco-editor/features/codeEditor/register.js";
import "monaco-editor/features/bracketMatching/register.js";
import "monaco-editor/features/clipboard/register.js";
import "monaco-editor/features/codeAction/register.js";
import "monaco-editor/features/comment/register.js";
import "monaco-editor/features/contextmenu/register.js";
import "monaco-editor/features/cursorUndo/register.js";
import "monaco-editor/features/documentSymbols/register.js";
import "monaco-editor/features/find/register.js";
import "monaco-editor/features/folding/register.js";
import "monaco-editor/features/fontZoom/register.js";
import "monaco-editor/features/format/register.js";
import "monaco-editor/features/gotoError/register.js";
import "monaco-editor/features/gotoLine/register.js";
import "monaco-editor/features/gotoSymbol/register.js";
import "monaco-editor/features/hover/register.js";
import "monaco-editor/features/indentation/register.js";
import "monaco-editor/features/linesOperations/register.js";
import "monaco-editor/features/multicursor/register.js";
import "monaco-editor/features/parameterHints/register.js";
import "monaco-editor/features/quickCommand/register.js";
import "monaco-editor/features/quickOutline/register.js";
import "monaco-editor/features/referenceSearch/register.js";
import "monaco-editor/features/rename/register.js";
import "monaco-editor/features/semanticTokens/register.js";
import "monaco-editor/features/smartSelect/register.js";
import "monaco-editor/features/snippet/register.js";
import "monaco-editor/features/suggest/register.js";
import "monaco-editor/features/wordHighlighter/register.js";
import "monaco-editor/features/wordOperations/register.js";
import "monaco-editor/features/wordPartOperations/register.js";
import "monaco-editor/editor/contrib/semanticTokens/browser/documentSemanticTokens.js";
import "monaco-editor/editor/contrib/gotoSymbol/browser/goToCommands.js";
import type { TextDocumentSyntax } from "../../editor/types";
import { THEME_CODE_EDITOR } from "../../theme";

export { monaco };

/** The editor whose text has keyboard focus; its find and rename inputs are ordinary text fields. */
export function focusedCodeEditor() {
  return monaco.editor.getEditors().find((editor) => editor.hasTextFocus());
}

/** Runs a Monaco action in the editor with keyboard focus, for app menus that receive its shortcuts first. */
export function runFocusedEditorAction(action: "undo" | "redo" | "editor.action.selectAll") {
  focusedCodeEditor()?.trigger("menu", action, null);
}

export const DATA_LANGUAGE = "donder-data";
export const SCRIPT_LANGUAGE = "donder";
const THEME = "donder";

export function languageForSyntax(syntax: TextDocumentSyntax): string {
  switch (syntax) {
    case "data":
      return DATA_LANGUAGE;
    case "script":
      return SCRIPT_LANGUAGE;
    case "plain":
      return "plaintext";
  }
}

/** Semantic token colors: Monaco's theme rules, and colors for hosts that read them. */
const TOKEN_RULES: { token: string; color: string; italic?: boolean }[] = [
  { token: "comment", color: THEME_CODE_EDITOR.muted, italic: true },
  { token: "keyword", color: THEME_CODE_EDITOR.keyword },
  { token: "type", color: THEME_CODE_EDITOR.type },
  { token: "enumMember", color: THEME_CODE_EDITOR.type },
  { token: "number", color: THEME_CODE_EDITOR.number },
  { token: "string", color: THEME_CODE_EDITOR.string },
  { token: "operator", color: THEME_CODE_EDITOR.muted },
  { token: "namespace", color: THEME_CODE_EDITOR.name },
  { token: "property", color: THEME_CODE_EDITOR.name },
  { token: "function", color: THEME_CODE_EDITOR.function },
  { token: "parameter", color: THEME_CODE_EDITOR.text },
  { token: "variable", color: THEME_CODE_EDITOR.text },
  { token: "variable.defaultLibrary", color: THEME_CODE_EDITOR.name }
];

/** The color of a semantic token, matched as Monaco matches theme rules. */
export function tokenColor(type: string, modifiers: string[]): string {
  for (let count = modifiers.length; count >= 0; count -= 1) {
    const scope = [type, ...modifiers.slice(0, count)].join(".");
    const rule = TOKEN_RULES.find((candidate) => candidate.token === scope);
    if (rule !== undefined) return rule.color;
  }
  return THEME_CODE_EDITOR.text;
}

/** The text color outside tokens. */
export const PLAIN_TEXT_COLOR = THEME_CODE_EDITOR.text;

let configured = false;

/**
 * Prepare Monaco once: the host supplies its editor worker, which each
 * bundler builds its own way.
 */
export function configureMonaco(createEditorWorker: () => Worker) {
  if (configured) return;
  configured = true;
  self.MonacoEnvironment = { getWorker: () => createEditorWorker() };
  monaco.languages.register({ id: DATA_LANGUAGE, extensions: [".data.donder"] });
  monaco.languages.register({ id: SCRIPT_LANGUAGE, extensions: [".donder"] });
  const brackets: monaco.languages.CharacterPair[] = [["{", "}"], ["[", "]"], ["(", ")"]];
  const pairs = [
    { open: "{", close: "}" },
    { open: "[", close: "]" },
    { open: "(", close: ")" },
    { open: '"', close: '"', notIn: ["string"] }
  ];
  monaco.languages.setLanguageConfiguration(DATA_LANGUAGE, {
    brackets,
    autoClosingPairs: [...pairs, { open: "<", close: ">" }],
    surroundingPairs: pairs
  });
  monaco.languages.setLanguageConfiguration(SCRIPT_LANGUAGE, {
    comments: { lineComment: "--" },
    brackets,
    autoClosingPairs: pairs,
    surroundingPairs: pairs
  });
  const code = THEME_CODE_EDITOR;
  monaco.editor.defineTheme(THEME, {
    base: "vs-dark",
    inherit: true,
    rules: TOKEN_RULES.map(({ token, color, italic }) => ({
      token,
      foreground: color.replace(/^#/, ""),
      ...(italic === true ? { fontStyle: "italic" } : {})
    })),
    colors: {
      "editor.background": code.background,
      "editor.foreground": code.text,
      "editorCursor.foreground": code.cursor,
      "editor.selectionBackground": code.selection,
      "editorLineNumber.foreground": code.muted,
      "editorLineNumber.activeForeground": code.text,
      "editorGutter.background": code.gutter,
      "editorError.foreground": code.error,
      "editorWarning.foreground": code.warning,
      "editorWidget.background": code.panel,
      "editorHoverWidget.background": code.panel,
      "editorSuggestWidget.background": code.panel
    }
  });
  monaco.editor.setTheme(THEME);
}

/** Options every Donder editor shares, as in rtl-lab. */
export function editorOptions(): monaco.editor.IStandaloneEditorConstructionOptions {
  const code = THEME_CODE_EDITOR;
  return {
    theme: THEME,
    automaticLayout: true,
    fontFamily: code.fontFamily,
    fontSize: code.fontSize,
    lineHeight: code.lineHeight,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    padding: { top: code.padding, bottom: code.padding },
    tabSize: 2,
    insertSpaces: true,
    bracketPairColorization: { enabled: true },
    guides: { indentation: true, bracketPairs: true },
    folding: true,
    renderWhitespace: "selection",
    wordWrap: "off",
    smoothScrolling: true,
    mouseWheelZoom: true,
    stickyScroll: { enabled: false },
    overviewRulerBorder: false,
    quickSuggestions: { other: true, comments: false, strings: false },
    suggest: { showWords: false },
    "semanticHighlighting.enabled": true
  };
}

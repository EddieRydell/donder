import { useEffect, useRef } from "react";
import type { BrowserCompileDiagnostic } from "../../editor/types";
import type { LanguageClient } from "./languageClient";
import { PLAIN_TEXT_COLOR, SCRIPT_LANGUAGE, editorOptions, monaco, tokenColor } from "./monaco";

/**
 * A visible character's document offset, its center in page coordinates (CSS
 * pixels), and its syntax color as a CSS color.
 */
export type DslSourceCharacter = { offset: number; x: number; y: number; color: string };
/** One source line; every character cell on it is `characterWidth` by `height` CSS pixels. */
export type DslSourceLine = { number: number; height: number; characterWidth: number; characters: DslSourceCharacter[] };

/**
 * Character-level access for hosts that treat source text as output, such as
 * the website's page lights. Geometry covers every line, rendered or not.
 */
export type DslSourceEditorHandle = {
  measure: () => DslSourceLine[];
  /** Called after the editor's layout or its token colors change. */
  onLayout: (listener: (() => void) | null) => void;
};

export type DslSourceEditorProps = {
  /** The document's URI, which identifies it to the language client. */
  uri: string;
  value: string;
  onChange: (value: string) => void;
  /** The host's diagnostics, which know whether its project accepts the source. */
  diagnostics: BrowserCompileDiagnostic[];
  language: LanguageClient | null;
  ariaLabel: string;
  className?: string;
  onHandle?: (handle: DslSourceEditorHandle | null) => void;
};

const MARKER_OWNER = "host";

/**
 * A controlled effect/operator script editor with the language server's
 * features. It grows to its content and leaves scrolling to the page.
 */
export function DslSourceEditor({ uri, value, onChange, diagnostics, language, ariaLabel, className, onHandle }: DslSourceEditorProps) {
  const parent = useRef<HTMLDivElement>(null);
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const applying = useRef(false);
  const latestOnChange = useRef(onChange);
  const initial = useRef({ uri, value, ariaLabel, onHandle, language });
  useEffect(() => {
    latestOnChange.current = onChange;
  }, [onChange]);

  useEffect(() => {
    if (!parent.current) return;
    const { onHandle, language } = initial.current;
    const resource = monaco.Uri.parse(initial.current.uri);
    const model = monaco.editor.getModel(resource) ?? monaco.editor.createModel(initial.current.value, SCRIPT_LANGUAGE, resource);
    const host = parent.current;
    const created = monaco.editor.create(host, {
      ...editorOptions(),
      model,
      ariaLabel: initial.current.ariaLabel,
      scrollbar: { vertical: "hidden", handleMouseWheel: false, alwaysConsumeMouseWheel: false }
    });
    const fit = () => { host.style.height = `${created.getContentHeight()}px`; };
    fit();
    editor.current = created;
    language?.attach(model);
    let layoutListener: (() => void) | null = null;
    const notify = () => { layoutListener?.(); };
    const subscriptions = [
      model.onDidChangeContent(() => {
        if (!applying.current) latestOnChange.current(model.getValue());
      }),
      created.onDidLayoutChange(notify),
      created.onDidContentSizeChange(() => {
        fit();
        notify();
      }),
      created.onDidChangeConfiguration(notify),
      ...(language === null ? [] : [language.onTokens((changed) => { if (changed === model.uri.toString()) notify(); })])
    ];
    onHandle?.({
      measure: () => measureCharacters(created, model, language),
      onLayout: (listener) => { layoutListener = listener; }
    });
    return () => {
      onHandle?.(null);
      for (const subscription of subscriptions) subscription.dispose();
      created.dispose();
      model.dispose();
      editor.current = null;
    };
  }, []);

  useEffect(() => {
    const model = editor.current?.getModel();
    if (model === null || model === undefined || model.getValue() === value) return;
    applying.current = true;
    model.pushEditOperations([], [{ range: model.getFullModelRange(), text: value }], () => null);
    applying.current = false;
  }, [value]);

  useEffect(() => {
    const model = editor.current?.getModel();
    if (model === null || model === undefined) return;
    const length = model.getValueLength();
    monaco.editor.setModelMarkers(model, MARKER_OWNER, diagnostics.map((diagnostic) => {
      const start = model.getPositionAt(Math.min(diagnostic.start, length));
      const end = model.getPositionAt(Math.min(Math.max(diagnostic.end, diagnostic.start + 1), length));
      return {
        severity: monaco.MarkerSeverity.Error,
        message: diagnostic.message,
        startLineNumber: start.lineNumber,
        startColumn: start.column,
        endLineNumber: end.lineNumber,
        endColumn: end.column
      };
    }));
  }, [diagnostics, value]);

  return <div ref={parent} className={className} />;
}

/** Each character's color from the document's semantic tokens. */
function characterColors(model: monaco.editor.ITextModel, language: LanguageClient | null): (offset: number) => string {
  const colors = new Map<number, string>();
  const tokens = language?.tokens.get(model.uri.toString());
  if (tokens !== undefined) {
    const { legend, data } = tokens;
    let line = 0;
    let character = 0;
    for (let index = 0; index + 4 < data.length + 1; index += 5) {
      const deltaLine = data[index] ?? 0;
      const deltaStart = data[index + 1] ?? 0;
      const length = data[index + 2] ?? 0;
      const type = legend.tokenTypes[data[index + 3] ?? 0] ?? "";
      const bits = data[index + 4] ?? 0;
      line += deltaLine;
      character = deltaLine === 0 ? character + deltaStart : deltaStart;
      const modifiers = legend.tokenModifiers.filter((_, bit) => (bits & (1 << bit)) !== 0);
      const color = tokenColor(type, modifiers);
      const start = model.getOffsetAt({ lineNumber: line + 1, column: character + 1 });
      for (let offset = start; offset < start + length; offset += 1) colors.set(offset, color);
    }
  }
  return (offset) => colors.get(offset) ?? PLAIN_TEXT_COLOR;
}

/** Positions come from Monaco's line tops and the monospace column, so unrendered lines are measured too. */
function measureCharacters(editor: monaco.editor.IStandaloneCodeEditor, model: monaco.editor.ITextModel, language: LanguageClient | null): DslSourceLine[] {
  const node = editor.getDomNode();
  if (node === null) return [];
  const color = characterColors(model, language);
  const rect = node.getBoundingClientRect();
  const layout = editor.getLayoutInfo();
  // Monaco types the font info option as `any`.
  const fontInfo = editor.getOption(monaco.editor.EditorOption.fontInfo) as monaco.editor.FontInfo;
  const width = fontInfo.typicalHalfwidthCharacterWidth;
  const height = editor.getOption(monaco.editor.EditorOption.lineHeight);
  const tabSize = model.getOptions().tabSize;
  const left = rect.left + window.scrollX + layout.contentLeft - editor.getScrollLeft();
  const top = rect.top + window.scrollY - editor.getScrollTop();
  const lines: DslSourceLine[] = [];
  for (let number = 1; number <= model.getLineCount(); number += 1) {
    const text = model.getLineContent(number);
    const y = top + editor.getTopForLineNumber(number) + height / 2;
    const lineStart = model.getOffsetAt({ lineNumber: number, column: 1 });
    const characters: DslSourceCharacter[] = [];
    let column = 0;
    for (let index = 0; index < text.length; index += 1) {
      const character = text[index];
      if (character === "\t") {
        column += tabSize - (column % tabSize);
        continue;
      }
      if (character !== undefined && character.trim() !== "") {
        const offset = lineStart + index;
        characters.push({ offset, x: left + (column + 0.5) * width, y, color: color(offset) });
      }
      column += 1;
    }
    lines.push({ number, height, characterWidth: width, characters });
  }
  return lines;
}

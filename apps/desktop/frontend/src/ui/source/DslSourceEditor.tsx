import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { ensureSyntaxTree, syntaxHighlighting } from "@codemirror/language";
import { linter, setDiagnostics } from "@codemirror/lint";
import { countColumn, EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { highlightTree } from "@lezer/highlight";
import { useEffect, useRef } from "react";
import type { BrowserCompileDiagnostic } from "../../editor/types";
import { donderHighlightStyle, languageForSyntax } from "./dslSyntax";

/**
 * A visible character's document offset, its center in page coordinates (CSS
 * pixels), and its syntax color as a computed CSS color.
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
};

export type DslSourceEditorProps = {
  value: string;
  onChange: (value: string) => void;
  diagnostics: BrowserCompileDiagnostic[];
  ariaLabel: string;
  className?: string;
  onHandle?: (handle: DslSourceEditorHandle | null) => void;
};

/** A controlled effect/operator DSL editor with Donder highlighting and inline diagnostics. */
export function DslSourceEditor({ value, onChange, diagnostics, ariaLabel, className, onHandle }: DslSourceEditorProps) {
  const parent = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const latestOnChange = useRef(onChange);
  const initial = useRef({ value, ariaLabel, onHandle });
  useEffect(() => {
    latestOnChange.current = onChange;
  }, [onChange]);

  useEffect(() => {
    if (!parent.current) return;
    const { onHandle } = initial.current;
    const created = new EditorView({
      parent: parent.current,
      state: EditorState.create({
        doc: initial.current.value,
        extensions: [
          languageForSyntax("effectDsl"),
          history(),
          syntaxHighlighting(donderHighlightStyle),
          linter(null, { autoPanel: false }),
          keymap.of([...historyKeymap, ...defaultKeymap]),
          EditorView.contentAttributes.of({ "aria-label": initial.current.ariaLabel }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) latestOnChange.current(update.state.doc.toString());
          })
        ]
      })
    });
    view.current = created;
    onHandle?.({
      measure: () => measureCharacters(created)
    });
    return () => {
      onHandle?.(null);
      created.destroy();
      view.current = null;
    };
  }, []);

  useEffect(() => {
    const current = view.current;
    if (!current) return;
    const text = current.state.doc.toString();
    if (text !== value) current.dispatch({ changes: { from: 0, to: text.length, insert: value } });
  }, [value]);

  useEffect(() => {
    const current = view.current;
    if (!current) return;
    const length = current.state.doc.length;
    current.dispatch(setDiagnostics(current.state, diagnostics.map((diagnostic) => {
      const from = Math.min(diagnostic.start, length);
      return { from, to: Math.min(Math.max(diagnostic.end, from + 1), length), severity: "error", message: diagnostic.message };
    })));
  }, [diagnostics, value]);

  return <div ref={parent} className={className} />;
}

function isVisible(character: string | undefined) {
  return character !== undefined && character.trim() !== "";
}

/** Syntax colors for every character, from the full syntax tree rather than the rendered DOM. */
function characterColors(view: EditorView): (offset: number) => string {
  const fallback = getComputedStyle(view.contentDOM).color;
  const classes = new Map<number, string>();
  const tree = ensureSyntaxTree(view.state, view.state.doc.length, SYNTAX_TREE_TIMEOUT_MS);
  if (tree) {
    highlightTree(tree, donderHighlightStyle, (from, to, names) => {
      for (let offset = from; offset < to; offset += 1) classes.set(offset, names);
    });
  }
  // Highlight classes are scoped to the editor, so probe inside it.
  const probe = document.createElement("span");
  view.dom.appendChild(probe);
  const colors = new Map<string, string>();
  for (const names of new Set(classes.values())) {
    probe.className = names;
    colors.set(names, getComputedStyle(probe).color);
  }
  probe.remove();
  return (offset) => colors.get(classes.get(offset) ?? "") ?? fallback;
}

const SYNTAX_TREE_TIMEOUT_MS = 50;

/** Positions come from CodeMirror's line layout and the monospace column, so unrendered lines are measured too. */
function measureCharacters(view: EditorView): DslSourceLine[] {
  const color = characterColors(view);
  const content = view.contentDOM.getBoundingClientRect();
  const rendered = view.coordsAtPos(view.viewport.from);
  const left = (rendered?.left ?? content.left) + window.scrollX;
  const top = view.documentTop + window.scrollY;
  const width = view.defaultCharacterWidth;
  const tabSize = view.state.tabSize;
  const doc = view.state.doc;
  const lines: DslSourceLine[] = [];
  for (let number = 1; number <= doc.lines; number += 1) {
    const line = doc.line(number);
    const block = view.lineBlockAt(line.from);
    const y = top + block.top + block.height / 2;
    const characters: DslSourceCharacter[] = [];
    for (let index = 0; index < line.text.length; index += 1) {
      if (!isVisible(line.text[index])) continue;
      const offset = line.from + index;
      characters.push({ offset, x: left + (countColumn(line.text, tabSize, index) + 0.5) * width, y, color: color(offset) });
    }
    lines.push({ number, height: block.height, characterWidth: width, characters });
  }
  return lines;
}

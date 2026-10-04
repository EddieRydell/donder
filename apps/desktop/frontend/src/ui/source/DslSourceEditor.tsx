import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { syntaxHighlighting } from "@codemirror/language";
import { linter, setDiagnostics } from "@codemirror/lint";
import { EditorState, RangeSetBuilder, type Extension } from "@codemirror/state";
import { Decoration, EditorView, keymap, ViewPlugin, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { useEffect, useRef } from "react";
import type { BrowserCompileDiagnostic } from "../../editor/types";
import { donderHighlightStyle, languageForSyntax } from "./dslSyntax";

/** DOM attributes added to every line and every non-whitespace character. */
export type DslSourceMarkup = { line: Record<string, string>; character: Record<string, string> };

export type DslSourceEditorProps = {
  value: string;
  onChange: (value: string) => void;
  diagnostics: BrowserCompileDiagnostic[];
  markup?: DslSourceMarkup;
  ariaLabel: string;
  className?: string;
};

/** A controlled effect/operator DSL editor with Donder highlighting and inline diagnostics. */
export function DslSourceEditor({ value, onChange, diagnostics, markup, ariaLabel, className }: DslSourceEditorProps) {
  const parent = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const latestOnChange = useRef(onChange);
  const initial = useRef({ value, markup, ariaLabel });
  useEffect(() => {
    latestOnChange.current = onChange;
  }, [onChange]);

  useEffect(() => {
    if (!parent.current) return;
    const { markup } = initial.current;
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
          }),
          ...(markup === undefined ? [] : [markupPlugin(markup)])
        ]
      })
    });
    view.current = created;
    return () => {
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

function markupPlugin(markup: DslSourceMarkup): Extension {
  const line = Decoration.line({ attributes: markup.line });
  const character = Decoration.mark({ attributes: markup.character });
  const build = (view: EditorView): DecorationSet => {
    const builder = new RangeSetBuilder<Decoration>();
    const doc = view.state.doc;
    for (let number = 1; number <= doc.lines; number += 1) {
      const current = doc.line(number);
      builder.add(current.from, current.from, line);
      for (let offset = 0; offset < current.text.length; offset += 1) {
        if (current.text[offset]?.trim() === "") continue;
        builder.add(current.from + offset, current.from + offset + 1, character);
      }
    }
    return builder.finish();
  };
  return ViewPlugin.fromClass(class {
    decorations: DecorationSet;
    constructor(view: EditorView) { this.decorations = build(view); }
    update(update: ViewUpdate) { if (update.docChanged) this.decorations = build(update.view); }
  }, { decorations: (plugin) => plugin.decorations });
}

import { useEffect, useRef } from "react";
import { runCommand } from "../commandRegistry";
import { desktopLanguageClient } from "../editor/desktopLanguage";
import type { PersistedEditorViewState, TextDocumentSyntax } from "../types";
import type { TextNavigation } from "../workspace/navigation";
import { editorOptions, languageForSyntax, monaco } from "./source/monaco";

/** A project document's text in Monaco, with the language server attached. */
export function TextEditor({
  root,
  path,
  openPaths,
  syntax,
  text,
  readOnly,
  restore,
  navigation,
  onChange,
  onViewState
}: {
  root: string;
  path: string;
  /** Documents with tabs; models of closed ones are released. */
  openPaths: string[];
  syntax: TextDocumentSyntax;
  text: string;
  readOnly: boolean;
  restore: PersistedEditorViewState | undefined;
  navigation: TextNavigation | null;
  onChange: (text: string) => void;
  onViewState: (path: string, state: PersistedEditorViewState) => void;
}) {
  const host = useRef<HTMLDivElement | null>(null);
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const applying = useRef(false);
  const restored = useRef(new Set<string>());
  const latest = useRef({ onChange, onViewState, text });
  useEffect(() => { latest.current = { onChange, onViewState, text }; });

  useEffect(() => {
    if (host.current === null) return;
    const created = monaco.editor.create(host.current, { ...editorOptions(), model: null });
    created.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => { runCommand("file.save"); });
    editor.current = created;
    return () => {
      created.dispose();
      editor.current = null;
    };
  }, []);

  // One model per document, kept while its tab is open.
  useEffect(() => {
    const current = editor.current;
    if (current === null) return;
    const uri = monaco.Uri.file(`${root}/${path}`);
    const language = languageForSyntax(syntax);
    let model = monaco.editor.getModel(uri);
    if (model === null) model = monaco.editor.createModel(latest.current.text, language, uri);
    else if (model.getLanguageId() !== language) monaco.editor.setModelLanguage(model, language);
    current.setModel(model);
    desktopLanguageClient(root)?.attach(model);
    const state = restore;
    if (state !== undefined && !restored.current.has(path)) {
      restored.current.add(path);
      const anchor = model.getPositionAt(state.cursorAnchor);
      const head = model.getPositionAt(state.cursorHead);
      current.setSelection(new monaco.Selection(anchor.lineNumber, anchor.column, head.lineNumber, head.column));
      current.setScrollTop(state.scrollTop);
    }
    const document = model;
    const save = () => {
      const selection = current.getSelection();
      if (selection === null) return;
      latest.current.onViewState(path, {
        cursorAnchor: document.getOffsetAt(selection.getSelectionStart()),
        cursorHead: document.getOffsetAt(selection.getPosition()),
        scrollTop: current.getScrollTop()
      });
    };
    const subscriptions = [
      document.onDidChangeContent(() => {
        if (!applying.current) latest.current.onChange(document.getValue());
      }),
      current.onDidChangeCursorSelection(save),
      current.onDidScrollChange(save)
    ];
    return () => { for (const subscription of subscriptions) subscription.dispose(); };
  }, [path, restore, root, syntax]);

  useEffect(() => {
    const open = new Set(openPaths.map((open) => monaco.Uri.file(`${root}/${open}`).toString()));
    for (const model of monaco.editor.getModels()) {
      if (model.uri.scheme === "file" && !open.has(model.uri.toString())) model.dispose();
    }
  }, [openPaths, root]);

  // Text from the working copy, such as a reload or a GUI edit.
  useEffect(() => {
    const model = editor.current?.getModel();
    if (model === null || model === undefined || model.getValue() === text) return;
    applying.current = true;
    model.pushEditOperations([], [{ range: model.getFullModelRange(), text }], () => null);
    applying.current = false;
  }, [text]);

  useEffect(() => { editor.current?.updateOptions({ readOnly }); }, [readOnly]);

  useEffect(() => {
    const current = editor.current;
    if (current === null || navigation === null || navigation.path !== path) return;
    if (navigation.range !== null) {
      const range = new monaco.Range(
        navigation.range.start.line + 1,
        navigation.range.start.character + 1,
        navigation.range.end.line + 1,
        navigation.range.end.character + 1
      );
      current.setSelection(range);
      current.revealRangeInCenter(range);
    }
    current.focus();
  }, [navigation, path]);

  return <div ref={host} className="editor-host" />;
}

import { useEffect, useRef } from "react";
import { commands } from "../api";
import { runCommand } from "../commandRegistry";
import { desktopLanguageClient } from "../editor/desktopLanguage";
import { documentSync, useAppStore } from "../store";
import type { PersistedEditorViewState, TextDocumentSyntax } from "../types";
import type { TextNavigation } from "../workspace/navigation";
import { contentEdits } from "./source/languageClient";
import { editorOptions, languageForSyntax, monaco } from "./source/monaco";

/**
 * A project document's text in Monaco, with the language server attached.
 * The model is the editor's text: edits go to the backend as they happen, and
 * the backend's text is read again only when it changed some other way.
 */
export function TextEditor({
  root,
  epoch,
  path,
  revision,
  openPaths,
  syntax,
  readOnly,
  restore,
  navigation,
  onViewState
}: {
  root: string;
  epoch: number;
  path: string;
  /** The backend revision of the document. */
  revision: number;
  /** Documents with tabs; models of closed ones are released. */
  openPaths: string[];
  syntax: TextDocumentSyntax;
  readOnly: boolean;
  restore: PersistedEditorViewState | undefined;
  navigation: TextNavigation | null;
  onViewState: (path: string, state: PersistedEditorViewState) => void;
}) {
  const host = useRef<HTMLDivElement | null>(null);
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const applying = useRef(false);
  const restored = useRef(new Set<string>());
  const latest = useRef({ onViewState });
  useEffect(() => { latest.current = { onViewState }; });
  const textReload = useAppStore((store) => store.textReload);

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
    const subscriptions: monaco.IDisposable[] = [];
    let cancelled = false;
    const show = (model: monaco.editor.ITextModel) => {
      if (model.getLanguageId() !== language) monaco.editor.setModelLanguage(model, language);
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
      const save = () => {
        const selection = current.getSelection();
        if (selection === null) return;
        latest.current.onViewState(path, {
          cursorAnchor: model.getOffsetAt(selection.getSelectionStart()),
          cursorHead: model.getOffsetAt(selection.getPosition()),
          scrollTop: current.getScrollTop()
        });
      };
      subscriptions.push(
        model.onDidChangeContent((event) => {
          if (!applying.current) documentSync.queue(epoch, path, contentEdits(event), () => model.getValue());
        }),
        current.onDidChangeCursorSelection(save),
        current.onDidScrollChange(save)
      );
    };
    const existing = monaco.editor.getModel(uri);
    if (existing !== null && documentSync.revision(epoch, path) !== null) {
      show(existing);
    } else {
      current.setModel(null);
      void commands.documentText(epoch, path).then((document) => {
        if (cancelled) return;
        const model = monaco.editor.getModel(uri) ?? monaco.editor.createModel(document.text, language, uri);
        replaceText(model, document.text, applying);
        documentSync.loaded(epoch, path, document.documentRevision);
        show(model);
      }).catch((error: unknown) => { useAppStore.getState().setError(String(error)); });
    }
    return () => {
      cancelled = true;
      for (const subscription of subscriptions) subscription.dispose();
    };
  }, [epoch, path, restore, root, syntax]);

  useEffect(() => {
    const open = new Set(openPaths.map((open) => monaco.Uri.file(`${root}/${open}`).toString()));
    for (const model of monaco.editor.getModels()) {
      if (model.uri.scheme === "file" && !open.has(model.uri.toString())) model.dispose();
    }
  }, [openPaths, root]);

  // The backend's text changed some other way: a reload, a GUI edit, a rename.
  useEffect(() => {
    const known = documentSync.revision(epoch, path);
    if (known === null || known === revision || documentSync.pending(epoch, path)) return;
    const uri = monaco.Uri.file(`${root}/${path}`);
    void commands.documentText(epoch, path).then((document) => {
      const model = monaco.editor.getModel(uri);
      if (model === null || documentSync.pending(epoch, path) || documentSync.revision(epoch, path) !== known) return;
      replaceText(model, document.text, applying);
      documentSync.loaded(epoch, path, document.documentRevision);
    }).catch((error: unknown) => { useAppStore.getState().setError(String(error)); });
  }, [epoch, path, revision, root, textReload]);

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

/** Show the backend's text without sending it back as an edit. */
function replaceText(model: monaco.editor.ITextModel, text: string, applying: { current: boolean }) {
  if (model.getValue() === text) return;
  applying.current = true;
  model.pushEditOperations([], [{ range: model.getFullModelRange(), text }], () => null);
  applying.current = false;
}

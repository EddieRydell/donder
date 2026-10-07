// The desktop's language server session: the server runs in the app process
// behind a Tauri command and event, one session per open project.
import { listen } from "@tauri-apps/api/event";
import EditorWorker from "monaco-editor/editor/editor.worker.js?worker";
import { commands } from "../api";
import { runSnapshotCommand, useAppStore } from "../store";
import { LanguageClient, type LanguageTransport } from "../ui/source/languageClient";
import { configureMonaco, monaco } from "../ui/source/monaco";
import { navigateToText } from "../workspace/navigation";

configureMonaco(() => new EditorWorker());

const transport: LanguageTransport = {
  send: (message) => {
    void commands.languageServerSend(message).catch((error: unknown) => { useAppStore.getState().setError(String(error)); });
  },
  listen: (listener) => {
    const stop = listen<string>("language_server_message", (event) => { listener(event.payload); });
    return () => { void stop.then((unlisten) => { unlisten(); }); };
  }
};

let session: { root: string; client: LanguageClient } | null = null;

/** The language client of the open project, started on first use. */
export function desktopLanguageClient(root: string | null): LanguageClient | null {
  if (session !== null && session.root === root) return session.client;
  session?.client.dispose();
  session = null;
  if (root === null) return null;
  const client = new LanguageClient(transport, {
    rootUri: monaco.Uri.file(root).toString(),
    diagnostics: true,
    applyExternalEdits: async (edits) => {
      const epoch = useAppStore.getState().snapshot?.projectEpoch;
      if (epoch === undefined) throw new Error("No project is loaded.");
      await runSnapshotCommand(() => commands.applyTextEdits(epoch, edits.map((edit) => ({
        uri: edit.uri,
        edits: edit.edits.map((change) => ({ range: change.range, text: change.newText }))
      }))));
    }
  });
  session = { root, client };
  return client;
}

// Saves, GUI edits and external changes reach the server as file changes.
let revision: number | null = null;
useAppStore.subscribe((state) => {
  const next = state.snapshot?.projectRevision ?? null;
  if (next === revision) return;
  revision = next;
  session?.client.filesChanged();
});

/** The project path of a document URI, or null outside the project. */
export function projectPath(uri: monaco.Uri): string | null {
  const root = session?.root;
  if (root === undefined) return null;
  const prefix = monaco.Uri.file(root).toString();
  const text = uri.toString();
  if (!text.startsWith(`${prefix}/`)) return null;
  return decodeURIComponent(text.slice(prefix.length + 1));
}

// Go to definition in another document opens it in the editor.
monaco.editor.registerEditorOpener({
  openCodeEditor: (_source, resource, selection) => {
    const path = projectPath(resource);
    if (path === null) return false;
    const range = selection === undefined ? null : monaco.Range.isIRange(selection)
      ? selection
      : { startLineNumber: selection.lineNumber, startColumn: selection.column, endLineNumber: selection.lineNumber, endColumn: selection.column };
    void navigateToText(path, range === null ? null : {
      start: { line: range.startLineNumber - 1, character: range.startColumn - 1 },
      end: { line: range.endLineNumber - 1, character: range.endColumn - 1 }
    });
    return true;
  }
});

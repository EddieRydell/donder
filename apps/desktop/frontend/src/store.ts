import { create } from "zustand";
import { useShallow } from "zustand/react/shallow";
import { listen } from "@tauri-apps/api/event";
import { commands } from "./api";
import { effectiveEditorViewMode } from "./editorViewMode";
import { DocumentSync } from "./documentSync";
import { isNewerSnapshot, reconcileGuiRequest, sameGuiDocument } from "./snapshotState";
import type { AppSnapshot, AudioTransportSnapshot, GuiDocument, GuiDocumentRequest, GuiEditResult, LiveOutputSnapshot, ProjectRestoreState } from "./types";

type SnapshotApplySource = "event" | "command" | "hydrate";

type GuiParent = { request: GuiDocumentRequest; document: GuiDocument; revision: number };

export type AppStaticSnapshot = Omit<AppSnapshot, "audioTransport" | "liveOutput">;

type AppStore = {
  snapshot: AppSnapshot | null;
  restoreState: ProjectRestoreState | null;
  guiRequest: GuiDocumentRequest | null;
  guiDocument: GuiDocument | null;
  guiDocumentRevision: number | null;
  guiEditPending: boolean;
  guiParents: GuiParent[];
  guiResetRevision: number;
  compositionGraphEditing: boolean;
  error: string | null;
  failedDocumentSync: { epoch: number; path: string } | null;
  localText: string;
  setSnapshot: (snapshot: AppSnapshot, source?: SnapshotApplySource, preserveLocalText?: boolean) => void;
  setAudioTransport: (transport: AudioTransportSnapshot) => void;
  setLiveOutput: (liveOutput: LiveOutputSnapshot) => void;
  setPreviewOpen: (previewOpen: boolean) => void;
  setRestoreState: (restoreState: ProjectRestoreState | null) => void;
  setGuiDocument: (document: GuiDocument | null) => void;
  resetGuiLocalState: () => void;
  setCompositionGraphEditing: (editing: boolean) => void;
  applyGuiEditResult: (request: GuiDocumentRequest, result: GuiEditResult) => boolean;
  setError: (error: string | null) => void;
  setLocalText: (text: string) => void;
  hydrate: () => Promise<void>;
};

export const useAppStore = create<AppStore>((set) => ({
  snapshot: null,
  restoreState: null,
  guiRequest: null,
  guiDocument: null,
  guiDocumentRevision: null,
  guiEditPending: false,
  guiParents: [],
  guiResetRevision: 0,
  compositionGraphEditing: false,
  error: null,
  failedDocumentSync: null,
  localText: "",
  setSnapshot: (snapshot, source = "command", preserveLocalText = false) => {
    set((current) => snapshotUpdate(current, snapshot, source, preserveLocalText));
  },
  setAudioTransport: (audioTransport) => {
    set((current) => current.snapshot === null ? current : {
      snapshot: {
        ...current.snapshot,
        audioTransport: mergeTransport(current.snapshot.audioTransport, audioTransport, "event")
      }
    });
  },
  setLiveOutput: (liveOutput) => {
    set((current) => current.snapshot === null ? current : {
      snapshot: {
        ...current.snapshot,
        liveOutput: mergeTransport(current.snapshot.liveOutput, liveOutput, "event")
      }
    });
  },
  setPreviewOpen: (previewOpen) => {
    set((current) => current.snapshot === null
      ? current
      : {
          snapshot: {
            ...current.snapshot,
            previewOpen
          }
        });
  },
  setRestoreState: (restoreState) => {
    set({ restoreState });
  },
  setGuiDocument: (guiDocument) => {
    set((current) => ({ guiDocument, guiDocumentRevision: guiDocument === null ? null : current.guiRequest?.projectRevision ?? null }));
  },
  resetGuiLocalState: () => {
    set((current) => ({
      guiResetRevision: current.guiResetRevision + 1,
      compositionGraphEditing: false
    }));
  },
  setCompositionGraphEditing: (compositionGraphEditing) => {
    set({ compositionGraphEditing });
  },
  applyGuiEditResult: (request, result) => {
    let applied = false;
    set((current) => {
      if (
        !sameGuiDocument(current.guiRequest, request)
        || current.snapshot?.projectEpoch !== result.snapshot.projectEpoch
        || current.snapshot.projectRoot !== result.snapshot.projectRoot
        || result.snapshot.projectRevision < current.snapshot.projectRevision
      ) return current;
      const update = snapshotUpdate(current, result.snapshot, "command", false);
      const nextRequest = update.guiRequest === undefined ? current.guiRequest : update.guiRequest;
      if (!sameGuiDocument(nextRequest, request) || nextRequest?.projectRevision !== result.snapshot.projectRevision) return current;
      applied = true;
      return {
        ...update,
        guiDocument: result.document,
        guiDocumentRevision: result.snapshot.projectRevision
      };
    });
    return applied;
  },
  setError: (error) => {
    set({ error });
  },
  setLocalText: (localText) => {
    const snapshot = useAppStore.getState().snapshot;
    if (snapshot?.activeBuffer !== null && snapshot?.activeBuffer !== undefined) {
      documentSync.queue(snapshot.projectEpoch, snapshot.activeBuffer.path, snapshot.activeBuffer.documentRevision, localText);
    }
    set({ localText });
  },
  hydrate: async () => {
    const snapshot = await commands.getSnapshot();
    useAppStore.getState().setSnapshot(snapshot, "hydrate");
    const restoreState = await commands.getRestoredViewState();
    useAppStore.getState().setRestoreState(restoreState);
    set({ error: null });
  }
}));

const documentSync = new DocumentSync(
  commands.updateDocument,
  (snapshot) => {
    useAppStore.getState().setSnapshot(snapshot);
    const current = useAppStore.getState().snapshot;
    if (current?.activeBuffer !== null && current?.activeBuffer !== undefined && documentSync.pendingText(current.projectEpoch, current.activeBuffer.path) === null) {
      useAppStore.setState({ localText: current.activeBuffer.text });
    }
  },
  (error, epoch, path) => {
    useAppStore.setState({ error: String(error), failedDocumentSync: { epoch, path } });
  }
);

export const flushDocumentSync = () => documentSync.flush();

export async function resolveDocumentSyncFailure(keepText: boolean) {
  const failed = useAppStore.getState().failedDocumentSync;
  if (failed === null) return;
  try {
    let snapshot = await commands.getSnapshot();
    const buffer = snapshot.tabs.find((tab) => tab.path === failed.path);
    if (keepText && (snapshot.projectEpoch !== failed.epoch || buffer === undefined)) {
      throw new Error("The edited file is no longer open. Copy your text before discarding the pending edit.");
    }
    if (snapshot.projectEpoch === failed.epoch && buffer !== undefined && buffer.externalState !== "current") {
      snapshot = await commands.resolveExternalConflict(failed.epoch, failed.path, buffer.documentRevision, keepText ? "keepWorkingCopy" : "reload");
    }
    const revision = snapshot.tabs.find((tab) => tab.path === failed.path)?.documentRevision ?? null;
    documentSync.resolveFailure(failed.epoch, failed.path, keepText ? revision : null);
    useAppStore.getState().setSnapshot(snapshot);
    useAppStore.setState({ failedDocumentSync: null, error: null });
    if (keepText) await flushDocumentSync();
    else useAppStore.setState({ localText: snapshot.activeBuffer?.text ?? "" });
  } catch (error) {
    useAppStore.getState().setError(String(error));
  }
}

export function subscribeToSnapshots(): Promise<() => void> {
  return Promise.all([
    listen<AppSnapshot>("app_snapshot_changed", (event) => {
      useAppStore.getState().setSnapshot(event.payload, "event");
    }),
    listen<AudioTransportSnapshot>("audio_transport_changed", (event) => {
      useAppStore.getState().setAudioTransport(event.payload);
    }),
    listen<boolean>("preview_window_changed", (event) => {
      useAppStore.getState().setPreviewOpen(event.payload);
    }),
    listen<LiveOutputSnapshot>("live_output_changed", (event) => {
      useAppStore.getState().setLiveOutput(event.payload);
    })
  ]).then((unsubscribes) => () => {
    for (const unsubscribe of unsubscribes) {
      unsubscribe();
    }
  });
}

export function useStaticAppSnapshot(): AppStaticSnapshot | null {
  return useAppStore(useShallow((store) => {
    if (store.snapshot === null) return null;
    const { audioTransport, liveOutput, ...snapshot } = store.snapshot;
    return snapshot;
  }));
}

export async function runSnapshotCommand(command: () => Promise<AppSnapshot>) {
  try {
    await flushDocumentSync();
    const previousRoot = useAppStore.getState().snapshot?.projectRoot ?? null;
    const snapshot = await command();
    useAppStore.getState().setSnapshot(snapshot, "command");
    if (snapshot.projectRoot !== previousRoot) {
      const restoreState = await commands.getRestoredViewState();
      useAppStore.getState().setRestoreState(restoreState);
    }
    useAppStore.getState().setError(null);
    return snapshot;
  } catch (error) {
    useAppStore.getState().setError(String(error));
    throw error;
  }
}

export async function runGuiEditCommand<T extends GuiEditResult>(command: (request: GuiDocumentRequest) => Promise<T>, origin?: GuiDocumentRequest | null): Promise<T> {
  if (useAppStore.getState().snapshot?.activeBuffer?.readOnly === true) throw new Error("Dependency sources are read-only. Create an independent copy to edit them.");
  const { guiRequest: request, guiDocumentRevision, guiEditPending } = useAppStore.getState();
  if (request === null) throw new Error("GUI edit attempted without an active GUI document request.");
  if (guiDocumentRevision !== request.projectRevision) throw new Error("The current GUI document is still loading.");
  if (guiEditPending) throw new Error("A GUI edit is already pending.");
  if (origin !== undefined && origin !== request) throw new Error("The GUI document changed during the gesture.");
  useAppStore.setState({ guiEditPending: true });
  try {
    const result = await command(request);
    if (result.document.type === "blocked") throw new Error(result.document.reason);
    if (!useAppStore.getState().applyGuiEditResult(request, result)) {
      throw new Error("The GUI document changed before the edit completed.");
    }
    useAppStore.getState().setError(null);
    return result;
  } catch (error) {
    if (useAppStore.getState().guiRequest === request) useAppStore.getState().setError(String(error));
    throw error;
  } finally {
    useAppStore.setState({ guiEditPending: false });
  }
}

function mergeTransport<T extends { generation: number }>(
  current: T | null,
  incoming: T,
  source: SnapshotApplySource
): T {
  if (current === null) return incoming;
  if (incoming.generation < current.generation) return current;
  if (source === "command" && incoming.generation === current.generation) return current;
  return incoming;
}

function snapshotUpdate(current: AppStore, incoming: AppSnapshot, source: SnapshotApplySource, preserveLocalText: boolean): Partial<AppStore> {
  const previous = current.snapshot;
  if (!isNewerSnapshot(previous, incoming)) return {};
  const snapshot = {
    ...incoming,
    audioTransport: mergeTransport(previous?.audioTransport ?? null, incoming.audioTransport, source),
    liveOutput: mergeTransport(previous?.liveOutput ?? null, incoming.liveOutput, source)
  };
  const nextRequest = guiRequestForSnapshot(snapshot, current.guiRequest);
  const { request, retainDocument } = reconcileGuiRequest(current.guiRequest, nextRequest,
    previous?.projectEpoch === snapshot.projectEpoch && previous.projectRoot === snapshot.projectRoot);
  const update: Partial<AppStore> = {
    snapshot,
    guiRequest: request
  };
  if (request === null || previous?.projectEpoch !== snapshot.projectEpoch || previous.projectRoot !== snapshot.projectRoot || previous.activeFile !== snapshot.activeFile) {
    update.guiParents = [];
  }
  if (!retainDocument) {
    update.guiDocument = null;
    update.guiDocumentRevision = null;
  }
  const projection = snapshot.guiProjection;
  if (projection !== null && request !== null
    && projection.projectRevision === request.projectRevision
    && projection.request.projectRevision === request.projectRevision
    && sameGuiDocument(projection.request, request)
    && (!retainDocument || current.guiDocumentRevision !== request.projectRevision)) {
    update.guiDocument = projection.document;
    update.guiDocumentRevision = projection.projectRevision;
  }
  if (!preserveLocalText && snapshot.activeBuffer !== null && effectiveEditorViewMode(snapshot) === "text") {
    update.localText = documentSync.pendingText(snapshot.projectEpoch, snapshot.activeBuffer.path) ?? snapshot.activeBuffer.text;
  }
  return update;
}

function guiRequestForSnapshot(
  snapshot: AppSnapshot,
  preferred: GuiDocumentRequest | null
): GuiDocumentRequest | null {
  const descriptor = snapshot.activeDocumentDescriptor;
  const activePath = snapshot.activeFile;
  if (descriptor === null || activePath === null || effectiveEditorViewMode(snapshot) !== "gui") return null;
  const preferredObject = preferred?.path === activePath
    ? descriptor.defaultObjectKeys.find(
        (item) => item.view === preferred.view && item.objectKey === preferred.objectKey
      )
    : undefined;
  if (preferredObject !== undefined) {
    return {
      path: activePath,
      view: preferredObject.view,
      objectKey: preferredObject.objectKey,
      projectRevision: snapshot.projectRevision
    };
  }
  const defaultObject = descriptor.defaultObjectKeys[0];
  if (defaultObject === undefined) return null;
  return { path: activePath, view: defaultObject.view, objectKey: defaultObject.objectKey, projectRevision: snapshot.projectRevision };
}

export function selectGuiObject(request: Pick<GuiDocumentRequest, "path" | "objectKey">, presentation: "tab" | "modal" = "tab"): void {
  const current = useAppStore.getState();
  const snapshot = current.snapshot;
  if (snapshot === null || snapshot.activeFile !== request.path) {
    throw new Error("The requested GUI object is not in the active document.");
  }
  const target = snapshot.activeDocumentDescriptor?.defaultObjectKeys.find(
    (item) => item.objectKey === request.objectKey
  );
  if (target === undefined) throw new Error("The requested GUI object is unavailable.");
  if (current.guiRequest?.path === request.path && current.guiRequest.objectKey === request.objectKey) return;
  const parents = presentation === "modal" ? current.guiParents.slice() : [];
  if (presentation === "modal") {
    if (current.guiRequest === null || current.guiDocument === null || current.guiDocumentRevision !== current.guiRequest.projectRevision || current.guiEditPending) {
      throw new Error("The current resource is still loading or saving.");
    }
    parents.push({ request: current.guiRequest, document: current.guiDocument, revision: current.guiDocumentRevision });
  }
  useAppStore.setState({
    guiParents: parents,
    guiRequest: { ...request, view: target.view, projectRevision: snapshot.projectRevision },
    guiDocument: null,
    guiDocumentRevision: null
  });
}

export function closeInlineEditor(): void {
  const current = useAppStore.getState();
  const parent = current.guiParents[current.guiParents.length - 1];
  if (parent === undefined || current.snapshot === null || current.guiEditPending) return;
  useAppStore.setState({
    guiParents: current.guiParents.slice(0, -1),
    guiRequest: parent.request.projectRevision === current.snapshot.projectRevision
      ? parent.request
      : { ...parent.request, projectRevision: current.snapshot.projectRevision },
    guiDocument: parent.document,
    guiDocumentRevision: parent.revision
  });
}

import { create } from "zustand";
import { useShallow } from "zustand/react/shallow";
import { commands, events, errorMessage } from "./api";
import { effectiveEditorViewMode } from "./editorViewMode";
import { DocumentSync } from "./documentSync";
import { PREVIEW_APPEARANCE } from "./previewAppearance";
import { applyGuiDocumentChange, isNewerSnapshot, reconcileGuiRequest, sameGuiDocument } from "./snapshotState";
import type { AppSnapshot, AudioTransportSnapshot, GuiDocument, GuiDocumentRequest, GuiEditUpdate, GuiProblem, LiveOutputSnapshot, ProjectRestoreState } from "./types";

type SnapshotApplySource = "event" | "command" | "hydrate";

type GuiParent = { request: GuiDocumentRequest; document: GuiDocument; revision: number };

export type AppStaticSnapshot = Omit<AppSnapshot, "audioTransport" | "liveOutput">;

/** What can fail outside an edit; each shows in the status bar's failure icon until it next succeeds. */
export type SystemFailure = "command" | "documentSync" | "viewState" | "menu" | "languageServer";

type AppStore = {
  snapshot: AppSnapshot | null;
  restoreState: ProjectRestoreState | null;
  guiRequest: GuiDocumentRequest | null;
  guiDocument: GuiDocument | null;
  guiDocumentRevision: number | null;
  guiEditPending: boolean;
  guiParents: GuiParent[];
  guiResetRevision: number;
  /** Bumped by each GUI undo or redo, which replaces the document. */
  guiHistoryRevision: number;
  /** The failures that have not succeeded since, by what failed. */
  failures: Partial<Record<SystemFailure, string>>;
  /** Why the last GUI edit was refused, until the project or the open object changes. */
  refusedEdit: GuiProblem | null;
  /** How many fields show the refused edit; the status bar shows it when none does. */
  refusedEditClaims: number;
  /** The changes to other objects the pending GUI edit waits to have confirmed, and the answer. */
  editConfirmation: { changes: string[]; decide: (confirmed: boolean) => void } | null;
  failedDocumentSync: { epoch: number; path: string } | null;
  /** Whether a text editor has edits the backend has not acknowledged. */
  documentEditsPending: boolean;
  /** Bumped when text editors must reload their documents' backend text. */
  textReload: number;
  setSnapshot: (snapshot: AppSnapshot, source?: SnapshotApplySource) => void;
  setAudioTransport: (transport: AudioTransportSnapshot) => void;
  setLiveOutput: (liveOutput: LiveOutputSnapshot) => void;
  setPreviewOpen: (previewOpen: boolean) => void;
  setRestoreState: (restoreState: ProjectRestoreState | null) => void;
  setGuiDocument: (document: GuiDocument | null) => void;
  resetGuiLocalState: () => void;
  bumpGuiHistory: () => void;
  applyGuiEditResult: (request: GuiDocumentRequest, result: GuiEditUpdate) => boolean;
  /** Report that `source` failed with `message`, or succeeded with null. */
  reportFailure: (source: SystemFailure, message: string | null) => void;
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
  guiHistoryRevision: 0,
  failures: {},
  refusedEdit: null,
  refusedEditClaims: 0,
  editConfirmation: null,
  failedDocumentSync: null,
  documentEditsPending: false,
  textReload: 0,
  setSnapshot: (snapshot, source = "command") => {
    set((current) => snapshotUpdate(current, snapshot, source));
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
    set((current) => ({ guiResetRevision: current.guiResetRevision + 1 }));
  },
  bumpGuiHistory: () => {
    set((current) => ({ guiHistoryRevision: current.guiHistoryRevision + 1 }));
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
      const update = snapshotUpdate(current, result.snapshot, "command");
      const nextRequest = update.guiRequest === undefined ? current.guiRequest : update.guiRequest;
      if (!sameGuiDocument(nextRequest, request) || nextRequest?.projectRevision !== result.snapshot.projectRevision) return current;
      applied = true;
      const base = current.guiDocumentRevision === request.projectRevision ? current.guiDocument : null;
      const document = applyGuiDocumentChange(base, result.change);
      // A change that does not apply leaves the document stale, so it is fetched whole.
      return document === null
        ? { ...update, guiDocument: current.guiDocument, guiDocumentRevision: current.guiDocumentRevision }
        : { ...update, guiDocument: document, guiDocumentRevision: result.snapshot.projectRevision };
    });
    return applied;
  },
  reportFailure: (source, message) => {
    set((current) => {
      if ((current.failures[source] ?? null) === message) return current;
      const failures = Object.fromEntries(Object.entries(current.failures).filter(([key]) => key !== source));
      return { failures: message === null ? failures : { ...failures, [source]: message } };
    });
  },
  hydrate: async () => {
    await commands.setPreviewAppearance(PREVIEW_APPEARANCE);
    const snapshot = await commands.getSnapshot();
    useAppStore.getState().setSnapshot(snapshot, "hydrate");
    const restoreState = await commands.getRestoredViewState();
    useAppStore.getState().setRestoreState(restoreState);
  }
}));

export const documentSync = new DocumentSync(
  commands.updateDocument,
  (snapshot) => { useAppStore.getState().setSnapshot(snapshot); },
  (error, epoch, path) => {
    useAppStore.setState({ failedDocumentSync: { epoch, path } });
    useAppStore.getState().reportFailure("documentSync", errorMessage(error));
  },
  (pending) => { useAppStore.setState({ documentEditsPending: pending }); }
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
    useAppStore.setState({ failedDocumentSync: null });
    useAppStore.getState().reportFailure("documentSync", null);
    if (keepText) await flushDocumentSync();
    else useAppStore.setState((store) => ({ textReload: store.textReload + 1 }));
  } catch (error) {
    useAppStore.getState().reportFailure("documentSync", errorMessage(error));
  }
}

export function subscribeToSnapshots(): Promise<() => void> {
  return Promise.all([
    events.appSnapshotChanged.listen((event) => {
      useAppStore.getState().setSnapshot(event.payload.snapshot, "event");
    }),
    events.audioTransportChanged.listen((event) => {
      useAppStore.getState().setAudioTransport(event.payload.transport);
    }),
    events.previewWindowChanged.listen((event) => {
      useAppStore.getState().setPreviewOpen(event.payload.open);
    }),
    events.liveOutputChanged.listen((event) => {
      useAppStore.getState().setLiveOutput(event.payload.output);
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
    useAppStore.getState().reportFailure("command", null);
    return snapshot;
  } catch (error) {
    useAppStore.getState().reportFailure("command", errorMessage(error));
    throw error;
  }
}

/** An edit that was not applied because it raced the open document; nothing for a person to read. */
function editNotApplied(reason: string): null {
  console.error(`GUI edit not applied: ${reason}`);
  return null;
}

/**
 * Apply one edit of the open GUI document, as `origin` showed it when one is given. Resolves to the
 * update, or null when the edit was not applied: refused, with its problem in `refusedEdit`, or
 * racing another edit or a newer document.
 */
export async function runGuiEditCommand(command: (request: GuiDocumentRequest) => Promise<GuiEditUpdate>, origin?: GuiDocumentRequest | null): Promise<GuiEditUpdate | null> {
  const { guiRequest: request, guiDocumentRevision, guiEditPending } = useAppStore.getState();
  if (request === null) return editNotApplied("no GUI document is open");
  if (guiDocumentRevision !== request.projectRevision) return editNotApplied("the GUI document is still loading");
  if (guiEditPending) return editNotApplied("another GUI edit is pending");
  if (origin !== undefined && origin !== request) return editNotApplied("the GUI document changed during the gesture");
  useAppStore.setState({ guiEditPending: true, refusedEdit: null });
  try {
    let result = await command(request);
    if (result.change.type === "needsConfirmation") {
      if (!await confirmEdit(result.change.changes)) return null;
      result = await commands.confirmGuiEdit(request);
    }
    if (result.change.type === "needsConfirmation") return editNotApplied("the confirmed edit asked again");
    if (result.change.type === "refused") {
      if (useAppStore.getState().guiRequest === request) showRefusedEdit(result.change.problem);
      return null;
    }
    if (!useAppStore.getState().applyGuiEditResult(request, result)) return editNotApplied("the GUI document changed before the edit completed");
    useAppStore.getState().reportFailure("command", null);
    return result;
  } catch (error) {
    useAppStore.getState().reportFailure("command", errorMessage(error));
    return null;
  } finally {
    useAppStore.setState({ guiEditPending: false });
  }
}

/** Ask the person to confirm an edit's `changes` to other objects; resolves with the answer. */
function confirmEdit(changes: string[]): Promise<boolean> {
  return new Promise((resolve) => {
    useAppStore.setState({
      editConfirmation: {
        changes,
        decide: (confirmed) => {
          useAppStore.setState({ editConfirmation: null });
          resolve(confirmed);
        }
      }
    });
  });
}

/** Show why an edit was refused, at its field or in the status bar; null clears it. */
export function showRefusedEdit(problem: GuiProblem | null) {
  useAppStore.setState({ refusedEdit: problem });
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

function snapshotUpdate(current: AppStore, incoming: AppSnapshot, source: SnapshotApplySource): Partial<AppStore> {
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
  // A refused edit's problem belongs to the project revision and object it was refused in.
  if (previous?.projectRevision !== snapshot.projectRevision || !sameGuiDocument(current.guiRequest, request)) {
    update.refusedEdit = null;
  }
  if (request === null || previous?.projectEpoch !== snapshot.projectEpoch || previous.projectRoot !== snapshot.projectRoot || previous.activeFile !== snapshot.activeFile) {
    update.guiParents = [];
  }
  if (!retainDocument) {
    update.guiDocument = null;
    update.guiDocumentRevision = null;
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
    ? descriptor.guiObjects.find((item) => item.key === preferred.objectKey)
    : undefined;
  if (preferredObject !== undefined) {
    return {
      path: activePath,
      objectKey: preferredObject.key,
      projectRevision: snapshot.projectRevision
    };
  }
  const defaultObject = descriptor.guiObjects[0];
  if (defaultObject === undefined) return null;
  return { path: activePath, objectKey: defaultObject.key, projectRevision: snapshot.projectRevision };
}

/** Opens `request`, an object of the active document, full size in the editor. A `nested` open keeps the open
 * object as a parent the breadcrumb returns to; opening one of those parents returns to it. */
export function selectGuiObject(request: Pick<GuiDocumentRequest, "path" | "objectKey">, placement: "root" | "nested" = "root"): void {
  const current = useAppStore.getState();
  const snapshot = current.snapshot;
  if (snapshot === null || snapshot.activeFile !== request.path) {
    throw new Error("The requested GUI object is not in the active document.");
  }
  const target = snapshot.activeDocumentDescriptor?.guiObjects.find(
    (item) => item.key === request.objectKey
  );
  if (target === undefined) throw new Error("The requested GUI object is unavailable.");
  const requested = { ...request, projectRevision: snapshot.projectRevision };
  if (current.guiRequest !== null && sameGuiDocument(current.guiRequest, requested)) return;
  const ancestor = placement === "nested" ? current.guiParents.findIndex((parent) => sameGuiDocument(parent.request, requested)) : -1;
  if (ancestor >= 0) {
    openGuiParent(ancestor);
    return;
  }
  const parents = placement === "nested" ? current.guiParents.slice() : [];
  if (placement === "nested") {
    if (current.guiRequest === null || current.guiDocument === null || current.guiDocumentRevision !== current.guiRequest.projectRevision || current.guiEditPending) {
      throw new Error("The current resource is still loading or saving.");
    }
    parents.push({ request: current.guiRequest, document: current.guiDocument, revision: current.guiDocumentRevision });
  }
  useAppStore.setState({
    guiParents: parents,
    guiRequest: requested,
    guiDocument: null,
    guiDocumentRevision: null,
    refusedEdit: null
  });
}

/** Returns to the open object's parent at `depth`, dropping the parents after it. */
export function openGuiParent(depth: number): void {
  const current = useAppStore.getState();
  const parent = current.guiParents[depth];
  if (parent === undefined || current.snapshot === null || current.guiEditPending) return;
  useAppStore.setState({
    guiParents: current.guiParents.slice(0, depth),
    guiRequest: parent.request.projectRevision === current.snapshot.projectRevision
      ? parent.request
      : { ...parent.request, projectRevision: current.snapshot.projectRevision },
    guiDocument: parent.document,
    guiDocumentRevision: parent.revision,
    refusedEdit: null
  });
}

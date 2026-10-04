import { createContext, useContext, type ReactNode } from "react";
import type * as Wire from "./types";

export type EditorSnapshot = Pick<Wire.AppSnapshot,
  "settings" | "projectRevision" | "audioTransport" | "liveOutput" | "activeBuffer">;
export type EditorEditResult = { snapshot: EditorSnapshot; document: Wire.GuiDocument };
export type EditorSelectionResult = EditorEditResult & Pick<Wire.SequenceSelectionEditResult,
  "selection" | "copiedCount" | "skippedCount">;
export type SequenceEditorState = {
  snapshot: EditorSnapshot | null;
  restoreState: Wire.ProjectRestoreState | null;
  guiRequest: Wire.GuiDocumentRequest | null;
  guiDocument: Wire.GuiDocument | null;
  guiDocumentRevision: number | null;
  guiEditPending: boolean;
  error: string | null;
  setError: (error: string | null) => void;
  setRestoreState: (state: Wire.ProjectRestoreState | null) => void;
  setSnapshot: (snapshot: EditorSnapshot, source?: "event" | "command" | "hydrate") => void;
};
export type SequenceEditorStore = {
  <T>(selector: (state: SequenceEditorState) => T): T;
  getState: () => SequenceEditorState;
};
export type SequenceEditorCommands = {
  applySequenceGuiEdit: (request: Wire.GuiDocumentRequest, edit: Wire.SequenceGuiEdit) => Promise<EditorEditResult>;
  applySequenceSelectionEdit: (request: Wire.GuiDocumentRequest, edit: Wire.SequenceSelectionEdit) => Promise<EditorSelectionResult>;
  rebindDetachedAutomation: (request: Wire.GuiDocumentRequest, clipId: number, detachedIndex: number, target: Wire.SequenceAutomationTarget, mapping: Wire.SequenceAutomationMapping) => Promise<EditorEditResult>;
  discardDetachedAutomation: (request: Wire.GuiDocumentRequest, clipId: number, detachedIndex: number) => Promise<EditorEditResult>;
  saveGraphViewState: (state: Wire.PersistedGraphViewStateUpdate) => Promise<EditorSnapshot>;
  saveSequenceViewportState: (state: Wire.PersistedSequenceViewportStateUpdate) => Promise<EditorSnapshot>;
  requestSequenceClipRasters: (request: Wire.SequenceClipRasterRequest) => Promise<Wire.SequenceClipRasterResponse>;
  takeSequenceClipRasterResults: (request: Wire.GuiDocumentRequest, requestId: number) => Promise<Wire.SequenceClipRasterResultBatch>;
  finishCompositionGraphEditing: () => Promise<EditorSnapshot>;
  audioPlay: () => Promise<EditorSnapshot>;
  audioPause: () => Promise<EditorSnapshot>;
  audioStop: () => Promise<EditorSnapshot>;
  audioRewindToZero: () => Promise<EditorSnapshot>;
  audioSeek: (seconds: number) => Promise<EditorSnapshot>;
  chooseSequenceAudio: (request: Wire.GuiDocumentRequest) => Promise<EditorEditResult>;
  setLiveOutputActive: (active: boolean) => Promise<EditorSnapshot>;
  setPreviewWindowOpen: (open: boolean) => Promise<EditorSnapshot>;
};
export type SequenceEditorHost = {
  commands: SequenceEditorCommands;
  store: SequenceEditorStore;
  runGuiEditCommand: <T extends EditorEditResult>(command: (request: Wire.GuiDocumentRequest) => Promise<T>, origin?: Wire.GuiDocumentRequest | null) => Promise<T>;
  runSnapshotCommand: (command: () => Promise<EditorSnapshot>) => Promise<EditorSnapshot>;
  resolveAssetUrl: (path: string, protocol?: string) => string;
  navigateToGuiObject: (reference: Pick<Wire.GuiObjectRef, "moduleId" | "path" | "objectKey" | "ownedPath">) => Promise<void>;
  capabilities: { audioFile: boolean; liveOutput: boolean; previewWindow: boolean };
  exportControls?: ReactNode;
};

const HostContext = createContext<SequenceEditorHost | null>(null);
export function SequenceEditorHostProvider({ host, children }: { host: SequenceEditorHost; children: ReactNode }) {
  return <HostContext.Provider value={host}>{children}</HostContext.Provider>;
}
export function useSequenceEditorHost() {
  const host = useContext(HostContext);
  if (host === null) throw new Error("The sequence editor requires a host provider.");
  return host;
}
export const GUI_HISTORY_CHANGED_EVENT = "donder:gui-history-changed";

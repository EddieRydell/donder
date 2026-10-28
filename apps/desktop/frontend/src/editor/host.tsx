import { createContext, useContext, type ReactNode } from "react";
import type * as Wire from "./types";

export type EditorSnapshot = Pick<Wire.AppSnapshot,
  "settings" | "projectRevision" | "audioTransport" | "liveOutput" | "activeBuffer">;
export type EditorEditResult = Omit<Wire.GuiEditUpdate, "snapshot"> & { snapshot: EditorSnapshot };
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
  applyGuiEdit: (request: Wire.GuiDocumentRequest, edit: Wire.GuiEditCommand) => Promise<EditorEditResult>;
  copySequenceSelection: (request: Wire.GuiDocumentRequest, selection: Wire.SequenceSelection) => Promise<EditorEditResult>;
  saveViewState: (update: Wire.ViewStateUpdate) => Promise<EditorSnapshot>;
  getSequenceEffectDetails: (request: Wire.GuiDocumentRequest, effectIds: number[]) => Promise<Wire.SequenceEffectDetailsResult>;
  matchSequenceClips: (request: Wire.GuiDocumentRequest, query: Wire.SequenceClipMatchRequest) => Promise<Wire.SequenceSelection>;
  requestSequenceClipRasters: (request: Wire.SequenceClipRasterRequest) => Promise<Wire.SequenceClipRasterResponse>;
  takeSequenceClipRasterResults: (request: Wire.GuiDocumentRequest, sinceRevision: number) => Promise<Wire.SequenceClipRasterResultBatch>;
  finishCompositionGraphEditing: () => Promise<EditorSnapshot>;
  audioPlay: () => Promise<EditorSnapshot>;
  audioPause: () => Promise<EditorSnapshot>;
  audioStop: () => Promise<EditorSnapshot>;
  audioRewindToZero: () => Promise<EditorSnapshot>;
  audioSeek: (seconds: number) => Promise<EditorSnapshot>;
  audioSetPlaybackSpeed: (speed: Wire.PlaybackSpeed) => Promise<EditorSnapshot>;
  audioSetRange: (range: Wire.PlaybackRange | null) => Promise<EditorSnapshot>;
  audioSetLooping: (looping: boolean) => Promise<EditorSnapshot>;
  chooseSequenceAudio: (request: Wire.GuiDocumentRequest) => Promise<EditorEditResult>;
  setLiveOutputActive: (active: boolean) => Promise<EditorSnapshot>;
  setPreviewWindowOpen: (open: boolean) => Promise<EditorSnapshot>;
  updateAppSettings: (settings: Wire.AppSettings) => Promise<EditorSnapshot>;
};
export type SequenceEditorHost = {
  commands: SequenceEditorCommands;
  store: SequenceEditorStore;
  runGuiEditCommand: (command: (request: Wire.GuiDocumentRequest) => Promise<EditorEditResult>, origin?: Wire.GuiDocumentRequest | null) => Promise<EditorEditResult>;
  runSnapshotCommand: (command: () => Promise<EditorSnapshot>) => Promise<EditorSnapshot>;
  resolveAssetUrl: (path: string, protocol?: string) => string;
  navigateToGuiObject: (reference: Pick<Wire.GuiObjectRef, "path" | "objectKey" | "ownedPath">) => Promise<void>;
  capabilities: { audioFile: boolean; liveOutput: boolean; previewWindow: boolean; playbackSpeed: boolean };
  /** Present when the host can analyze a sequence's audio for beats. */
  detectSequenceBeats?: (request: Wire.GuiDocumentRequest) => Promise<Wire.SequenceBeatDetection>;
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

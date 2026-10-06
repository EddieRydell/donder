import { convertFileSrc } from "@tauri-apps/api/core";
import { commands } from "../api";
import { useAppStore, runGuiEditCommand, runSnapshotCommand } from "../store";
import { navigateToGuiObject } from "../workspace/navigation";
import { SequenceExportDialog } from "../ui/gui/sequence/SequenceExportDialog";
import type { AppSnapshot } from "./types";
import type { EditorSnapshot, SequenceEditorHost, SequenceEditorState } from "./host";

function completeSnapshot(snapshot: EditorSnapshot): AppSnapshot {
  const current = useAppStore.getState().snapshot;
  if (current === null) throw new Error("No desktop project is loaded.");
  return { ...current, ...snapshot };
}
function editorState(): SequenceEditorState {
  return {
    ...useAppStore.getState(),
    setSnapshot: (snapshot, source) => { useAppStore.getState().setSnapshot(completeSnapshot(snapshot), source); }
  };
}
function useDesktopEditorState<T>(selector: (state: SequenceEditorState) => T): T {
  return useAppStore((state) => selector({ ...state, setSnapshot: editorState().setSnapshot }));
}
export const desktopSequenceEditorHost: SequenceEditorHost = {
  commands,
  store: Object.assign(useDesktopEditorState, { getState: editorState }),
  runGuiEditCommand: (command, origin) => runGuiEditCommand(async (request) => {
    const result = await command(request);
    return { ...result, snapshot: completeSnapshot(result.snapshot) };
  }, origin),
  runSnapshotCommand: (command) => runSnapshotCommand(async () => completeSnapshot(await command())),
  resolveAssetUrl: convertFileSrc,
  navigateToGuiObject,
  capabilities: { audioFile: true, liveOutput: true, previewWindow: true, playbackSpeed: true },
  exportControls: <SequenceExportDialog />
};

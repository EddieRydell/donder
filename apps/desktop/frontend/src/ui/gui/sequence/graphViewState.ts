import { useSequenceEditorHost } from "../../../editor/host";
import { objectViewKey } from "../../../workspace/guiIdentity";
import { useCallback, useState } from "react";
import type { PersistedGraphViewState, GuiObjectRef } from "../../../editor/types";
import { scheduleViewStateSave } from "../../../viewStatePersistence";
import { useSequenceEditErrorReporter } from "./sequenceLayers";

export function useGraphViewState(reference: GuiObjectRef) {
  const host = useSequenceEditorHost();
  const { commands, store: useAppStore } = host;
  const reportSequenceEditError = useSequenceEditErrorReporter();

  const { path, objectKey, ownedPath } = reference;
  const key = objectViewKey(reference);
  const [initial] = useState<PersistedGraphViewState>(() =>
    useAppStore.getState().restoreState?.graphViews[key] ?? { viewport: null, nodeSizes: {} });
  const [view, setView] = useState(initial);
  const save = useCallback((update: Partial<PersistedGraphViewState>) => {
    const restore = useAppStore.getState().restoreState;
    const state = { ...(restore?.graphViews[key] ?? initial), ...update };
    useAppStore.getState().setRestoreState({
      spatialViews: restore?.spatialViews ?? {},
      editorStates: restore?.editorStates ?? {}, sequenceViewports: restore?.sequenceViewports ?? {},
      graphViews: { ...restore?.graphViews, [key]: state }
    });
    setView(state);
    scheduleViewStateSave(JSON.stringify(["graph", key]), async () => {
      const snapshot = await commands.saveGraphViewState({ path, objectKey, ownedPath, state });
      useAppStore.getState().setSnapshot(snapshot, "command");
    }, reportSequenceEditError);
  }, [commands, reportSequenceEditError, useAppStore, initial, key, objectKey, path, ownedPath]);
  const saveSize = useCallback((id: string, width: number, height: number) => {
    const current = useAppStore.getState().restoreState?.graphViews[key] ?? initial;
    save({ nodeSizes: { ...current.nodeSizes, [id]: { width, height } } });
  }, [useAppStore, initial, key, save]);
  return { initial, view, save, saveSize };
}

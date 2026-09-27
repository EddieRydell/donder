import { useCallback, useState } from "react";
import { commands } from "../../../api";
import { useAppStore } from "../../../store";
import type { PersistedGraphViewState } from "../../../types";
import { scheduleViewStateSave } from "../../../viewStatePersistence";
import { reportSequenceEditError } from "./sequenceLayers";

export function useGraphViewState(path: string, objectKey: string) {
  const key = `${path}::${objectKey}`;
  const [initial] = useState<PersistedGraphViewState>(() =>
    useAppStore.getState().restoreState?.graphViews[key] ?? { viewport: null, nodeSizes: {} });
  const [view, setView] = useState(initial);
  const save = useCallback((update: Partial<PersistedGraphViewState>) => {
    const restore = useAppStore.getState().restoreState;
    const state = { ...(restore?.graphViews[key] ?? initial), ...update };
    useAppStore.getState().setRestoreState({
      editorStates: restore?.editorStates ?? {}, sequenceViewports: restore?.sequenceViewports ?? {},
      graphViews: { ...restore?.graphViews, [key]: state }
    });
    setView(state);
    scheduleViewStateSave(JSON.stringify(["graph", path, objectKey]), async () => {
      const snapshot = await commands.saveGraphViewState({ path, objectKey, state });
      useAppStore.getState().setSnapshot(snapshot, "command");
    }, reportSequenceEditError);
  }, [initial, key, objectKey, path]);
  const saveSize = useCallback((id: string, width: number, height: number) => {
    const current = useAppStore.getState().restoreState?.graphViews[key] ?? initial;
    save({ nodeSizes: { ...current.nodeSizes, [id]: { width, height } } });
  }, [initial, key, save]);
  return { initial, view, save, saveSize };
}

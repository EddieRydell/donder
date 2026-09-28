import { useState } from "react";
import { objectViewKey } from "../../../workspace/guiIdentity";
import { commands } from "../../../api";
import { useAppStore } from "../../../store";
import { scheduleViewStateSave } from "../../../viewStatePersistence";
import type { GuiObjectRef, SpatialGuide } from "../../../types";
export function useSpatialGuides(reference: GuiObjectRef) {
  const key = objectViewKey(reference);
  const [guides, setGuides] = useState<SpatialGuide[]>(() => useAppStore.getState().restoreState?.spatialViews[key]?.guides ?? []);
  const [error, setError] = useState<string | null>(null);
  const save = (guides: SpatialGuide[]) => {
    const restore = useAppStore.getState().restoreState;
    const state = { guides };
    setGuides(guides);
    useAppStore.getState().setRestoreState({ editorStates: restore?.editorStates ?? {}, sequenceViewports: restore?.sequenceViewports ?? {}, graphViews: restore?.graphViews ?? {}, spatialViews: { ...restore?.spatialViews, [key]: state } });
    scheduleViewStateSave(JSON.stringify(["spatial", key]), async () => {
      const snapshot = await commands.saveSpatialViewState({ ...reference, state });
      useAppStore.getState().setSnapshot(snapshot, "command");
      setError(null);
    }, (error) => { setError(String(error)); });
  };
  return { guides, save, error };
}

import { useCallback } from "react";
import { commands, errorMessage } from "./api";
import type { GuiObjectRef, PersistedGraphViewState, PersistedSequenceViewportState, PersistedSpatialViewState, ProjectRestoreState, ViewStateUpdate } from "./types";
import { objectViewKey } from "./workspace/guiIdentity";
import { useAppStore } from "./store";
import { THEME_METRICS } from "./theme";

const pending = new Map<string, () => Promise<unknown>>();
let timer: number | undefined;
let saving: Promise<unknown> = Promise.resolve();

/** Save view state under `key` soon, or at once; a failed save shows in the status bar until a save succeeds. */
export function scheduleViewStateSave(key: string, save: () => Promise<unknown>, immediate = false) {
  pending.set(key, save);
  window.clearTimeout(timer);
  const flush = () => {
    void flushViewStateSaves().then(
      () => { useAppStore.getState().reportFailure("viewState", null); },
      (error: unknown) => { useAppStore.getState().reportFailure("viewState", errorMessage(error)); }
    );
  };
  if (immediate) flush();
  else timer = window.setTimeout(flush, THEME_METRICS.workspaceLayoutSaveDelay);
}

export async function flushViewStateSaves(): Promise<void> {
  window.clearTimeout(timer);
  const saves = [...pending.entries()];
  pending.clear();
  const previous = saving;
  const job = (async () => {
    await previous;
    for (const [index, [, save]] of saves.entries()) {
      try {
        await save();
      } catch (error) {
        for (const [key, save] of saves.slice(index)) {
          if (!pending.has(key)) pending.set(key, save);
        }
        throw error;
      }
    }
  })();
  // Callers receive the failure; a later flush can retry the retained writes.
  saving = job.catch(() => undefined);
  await job;
}

type ObjectViews = { spatial: PersistedSpatialViewState; graph: PersistedGraphViewState; sequenceViewport: PersistedSequenceViewportState };
/** Where the restore state keeps each kind of object view. */
const RESTORED = { spatial: "spatialViews", graph: "graphViews", sequenceViewport: "sequenceViewports" } as const satisfies Record<keyof ObjectViews, keyof ProjectRestoreState>;

/** A GUI object's saved view of `kind`, and how to save another: at once for this session, soon to disk. */
export function useObjectViewState<K extends keyof ObjectViews>(kind: K, reference: GuiObjectRef): [ObjectViews[K] | undefined, (state: ObjectViews[K]) => void] {
  const key = objectViewKey(reference);
  const { path, objectKey } = reference;
  const restored = useAppStore((store) => store.restoreState?.[RESTORED[kind]][key]) as ObjectViews[K] | undefined;
  const save = useCallback((state: ObjectViews[K]) => {
    const restore = useAppStore.getState().restoreState ?? { editorStates: {}, sequenceViewports: {}, spatialViews: {}, graphViews: {} };
    useAppStore.getState().setRestoreState({ ...restore, [RESTORED[kind]]: { ...restore[RESTORED[kind]], [key]: state } });
    scheduleViewStateSave(JSON.stringify([kind, key]), async () => {
      const snapshot = await commands.saveViewState({ type: kind, state: { path, objectKey, state } } as ViewStateUpdate);
      useAppStore.getState().setSnapshot(snapshot, "command");
    });
  }, [key, kind, objectKey, path]);
  return [restored, save];
}

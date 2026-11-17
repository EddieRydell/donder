import { errorMessage } from "./api";
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

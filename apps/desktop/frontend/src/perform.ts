import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { commands } from "./api";
import { editTarget } from "./platform";
import { useAppStore } from "./store";
import type { PerformSnapshot } from "./types";

type PerformStore = {
  perform: PerformSnapshot | null;
  setPerform: (perform: PerformSnapshot) => void;
};

/** Perform mode as the backend reports it; the latest revision wins. */
export const usePerformStore = create<PerformStore>((set) => ({
  perform: null,
  setPerform: (perform) => {
    set((current) => current.perform !== null && perform.revision < current.perform.revision ? current : { perform });
  }
}));

async function runPerformCommand(command: () => Promise<PerformSnapshot>) {
  try {
    usePerformStore.getState().setPerform(await command());
  } catch (error) {
    useAppStore.getState().setError(String(error));
  }
}

export function startPerforming(console: number) {
  const projectRevision = useAppStore.getState().snapshot?.projectRevision;
  if (projectRevision === undefined) return;
  void runPerformCommand(() => commands.startPerforming(projectRevision, console));
}

export function stopPerforming() {
  void runPerformCommand(commands.stopPerforming);
}

export function isPerforming(): boolean {
  return (usePerformStore.getState().perform?.session ?? null) !== null;
}

/** Refreshes the project's consoles whenever the project changes. */
export function refreshPerform() {
  void runPerformCommand(commands.getPerformSnapshot);
}

/**
 * While performing, the console's keys play it instead of reaching editor shortcuts, except in text
 * entry and the code editor. Losing focus releases every held key.
 */
export function installPerformKeys() {
  const onKey = (event: KeyboardEvent) => {
    const keys = usePerformStore.getState().perform?.session?.keys;
    if (keys === undefined || !keys.includes(event.code) || editTarget(event.target) !== "app") return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (event.repeat) return;
    void runPerformCommand(() => commands.performKey(event.code, event.type === "keydown"));
  };
  const onBlur = () => {
    if (isPerforming()) void runPerformCommand(commands.performReleaseAll);
  };
  const changed = listen<PerformSnapshot>("perform_changed", (event) => {
    usePerformStore.getState().setPerform(event.payload);
  });
  window.addEventListener("keydown", onKey, { capture: true });
  window.addEventListener("keyup", onKey, { capture: true });
  window.addEventListener("blur", onBlur);
  return () => {
    window.removeEventListener("keydown", onKey, { capture: true });
    window.removeEventListener("keyup", onKey, { capture: true });
    window.removeEventListener("blur", onBlur);
    void changed.then((unlisten) => { unlisten(); });
  };
}

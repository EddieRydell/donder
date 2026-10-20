import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { commands } from "./api";
import { editTarget } from "./platform";
import { runSnapshotCommand, useAppStore } from "./store";
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

export function startPerforming(rig: number) {
  const projectRevision = useAppStore.getState().snapshot?.projectRevision;
  if (projectRevision === undefined) return;
  void runPerformCommand(() => commands.startPerforming(projectRevision, rig));
}

export function stopPerforming() {
  void runPerformCommand(commands.stopPerforming);
}

/**
 * Asks for a MIDI file and plays it into the performance; the first time, asks for the SoundFont
 * songs are heard on and keeps it in the settings.
 */
export async function playSong() {
  const settings = useAppStore.getState().snapshot?.settings;
  if (settings === undefined) return;
  if ((settings.songSoundFont ?? null) === null) {
    const songSoundFont = await commands.chooseSongSoundFont();
    if (songSoundFont === null) return;
    try {
      await runSnapshotCommand(() => commands.updateAppSettings({ ...settings, songSoundFont }));
    } catch (error) {
      useAppStore.getState().setError(String(error));
      return;
    }
  }
  const path = await commands.chooseMidiSong();
  if (path !== null) void runPerformCommand(() => commands.performPlaySong(path));
}

export function stopSong() {
  void runPerformCommand(commands.performStopSong);
}

export function isPerforming(): boolean {
  return (usePerformStore.getState().perform?.session ?? null) !== null;
}

/** Refreshes the project's rigs whenever the project changes. */
export function refreshPerform() {
  void runPerformCommand(commands.getPerformSnapshot);
}

/**
 * While performing, the rig's keys play it instead of reaching editor shortcuts, except in text
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

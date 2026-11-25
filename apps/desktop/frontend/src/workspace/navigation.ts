import { commands, errorMessage } from "../api";
import { runSnapshotCommand, selectGuiObject, useAppStore } from "../store";
import type { GuiDocumentRequest, GuiObjectRef, TextRange } from "../types";
import { objectViewKey } from "./guiIdentity";
import { dispatchUiEvent } from "../ui/uiEvents";

export type TextNavigation = {
  path: string;
  range: TextRange | null;
};

export async function navigateToText(path: string, range: TextRange | null): Promise<void> {
  const snapshot = await runSnapshotCommand(() => commands.openFile(path));
  const hasGuiView = (snapshot.activeDocumentDescriptor?.guiObjects.length ?? 0) > 0;
  if (hasGuiView) {
    await runSnapshotCommand(() => commands.setEditorViewMode("text"));
  }
  window.requestAnimationFrame(() => {
    dispatchUiEvent("navigateToText", { path, range });
  });
}

export async function navigateToGuiObject(reference: Pick<GuiObjectRef, "path" | "objectKey">): Promise<void> {
  try {
    const target = await commands.resolveGuiSource(reference.path, reference.objectKey);
    const selection = { path: target.path, objectKey: reference.objectKey };
    if (useAppStore.getState().guiRequest?.path === target.path) {
      selectGuiObject(selection, "nested");
      return;
    }
    const snapshot = await runSnapshotCommand(() => commands.openFile(target.path));
    if (snapshot.activeDocumentDescriptor?.guiObjects.some((object) => object.key === reference.objectKey) !== true) {
      await navigateToText(target.path, null);
      return;
    }
    if (snapshot.settings.editorViewMode !== "gui") {
      await runSnapshotCommand(() => commands.setEditorViewMode("gui"));
    }
    selectGuiObject(selection);
  } catch (error) {
    useAppStore.getState().reportFailure("command", errorMessage(error));
    throw error;
  }
}

/** The sequence `playSequence` opened, which plays once its audio loads. */
let pendingPlayback: string | null = null;

/** Opens a sequence and plays it from its playhead. */
export async function playSequence(reference: Pick<GuiObjectRef, "path" | "objectKey">): Promise<void> {
  const state = useAppStore.getState();
  if (state.guiRequest !== null && objectViewKey(state.guiRequest) === objectViewKey(reference) && state.guiDocument?.type === "sequence") {
    await runSnapshotCommand(commands.audioPlay);
    return;
  }
  pendingPlayback = objectViewKey(reference);
  try {
    await navigateToGuiObject(reference);
  } catch (error) {
    pendingPlayback = null;
    throw error;
  }
}

/** Whether the sequence `request` opens, whose audio just loaded, was opened to play; each request plays once. */
export function takePendingPlayback(request: GuiDocumentRequest): boolean {
  const pending = pendingPlayback;
  pendingPlayback = null;
  return pending !== null && pending === objectViewKey(request);
}

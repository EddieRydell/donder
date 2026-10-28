import { commands } from "../api";
import { runSnapshotCommand, selectGuiObject, useAppStore } from "../store";
import type { GuiObjectRef, TextRange } from "../types";

export const NAVIGATE_TO_TEXT_EVENT = "donder:navigate-to-text";

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
    window.dispatchEvent(
      new CustomEvent<TextNavigation>(NAVIGATE_TO_TEXT_EVENT, {
        detail: { path, range }
      })
    );
  });
}

export async function navigateToGuiObject(reference: Pick<GuiObjectRef, "path" | "objectKey" | "ownedPath">): Promise<void> {
  try {
    const target = await commands.resolveGuiSource(reference.path, reference.objectKey);
    const selection = { path: target.path, objectKey: reference.objectKey, ownedPath: reference.ownedPath };
    if (useAppStore.getState().guiRequest?.path === target.path) {
      selectGuiObject(selection, "modal");
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
    useAppStore.getState().setError(String(error));
    throw error;
  }
}

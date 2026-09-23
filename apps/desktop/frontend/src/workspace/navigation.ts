import { commands } from "../api";
import { runSnapshotCommand, selectGuiObject, useAppStore } from "../store";
import type { GuiObjectRef, TextRange } from "../types";

export const NAVIGATE_TO_TEXT_EVENT = "dawn:navigate-to-text";

export type TextNavigation = {
  path: string;
  range: TextRange | null;
};

export async function navigateToText(path: string, range: TextRange | null): Promise<void> {
  const snapshot = await runSnapshotCommand(() => commands.openFile(path));
  const hasGuiView = snapshot.activeDocumentDescriptor?.availableViews.some((view) => view !== "text") ?? false;
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

export async function navigateToGuiObject(reference: Pick<GuiObjectRef, "moduleId" | "path" | "objectKey">): Promise<void> {
  try {
    const target = await commands.resolveGuiSource(reference.moduleId, reference.path, reference.objectKey);
    if (target.view === "text") {
      await navigateToText(target.path, null);
      return;
    }
    if (useAppStore.getState().guiRequest?.path === target.path) {
      selectGuiObject({ path: target.path, objectKey: reference.objectKey }, "modal");
      return;
    }
    const snapshot = await runSnapshotCommand(() => commands.openFile(target.path));
    if (snapshot.settings.editorViewMode !== "gui") {
      await runSnapshotCommand(() => commands.setEditorViewMode("gui"));
    }
    selectGuiObject({ path: target.path, objectKey: reference.objectKey });
  } catch (error) {
    useAppStore.getState().setError(String(error));
    throw error;
  }
}

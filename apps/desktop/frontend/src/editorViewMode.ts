import type { AppSnapshot, EditorViewMode } from "./types";

type EditorViewSnapshot = Pick<AppSnapshot, "settings" | "activeDocumentDescriptor" | "projectHealth">;

export function effectiveEditorViewMode(snapshot: EditorViewSnapshot | null): EditorViewMode {
  if (snapshot === null || snapshot.projectHealth !== "ready" || snapshot.settings.editorViewMode === "text") {
    return "text";
  }
  return descriptorHasGuiView(snapshot.activeDocumentDescriptor) ? "gui" : "text";
}

function descriptorHasGuiView(descriptor: AppSnapshot["activeDocumentDescriptor"]): boolean {
  return (descriptor?.guiObjects.length ?? 0) > 0;
}

import type { AppSnapshot, GuiDocumentRequest } from "./types";

export function isNewerSnapshot(current: Pick<AppSnapshot, "stateRevision"> | null, incoming: Pick<AppSnapshot, "stateRevision">): boolean {
  return current === null || incoming.stateRevision > current.stateRevision;
}

export function sameGuiDocument(left: GuiDocumentRequest | null, right: GuiDocumentRequest | null): boolean {
  return left?.path === right?.path && left?.view === right?.view && left?.objectKey === right?.objectKey && JSON.stringify(left?.ownedPath) === JSON.stringify(right?.ownedPath);
}

/** Content revisions invalidate projections, not the editor's local view state. */
export function reconcileGuiRequest(previous: GuiDocumentRequest | null, next: GuiDocumentRequest | null, sameProject: boolean) {
  const sameDocument = sameProject && previous !== null && next !== null && sameGuiDocument(previous, next);
  return {
    request: sameDocument && previous.projectRevision === next.projectRevision ? previous : next,
    retainDocument: sameDocument
  };
}

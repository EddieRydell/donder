import type { AppSnapshot, GuiDocument, GuiDocumentChange, GuiDocumentRequest } from "./types";

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

/** The document after an edit's change, or `null` when a clip change does not
 *  apply to `document`, which must then be fetched again. */
export function applyGuiDocumentChange(document: GuiDocument | null, change: GuiDocumentChange): GuiDocument | null {
  if (change.type === "document") return change.document;
  if (document?.type !== "sequence") return null;
  const effects = new Map(document.document.effects.map((effect) => [effect.id, effect]));
  for (const effect of change.document.effects) effects.set(effect.id, effect);
  const ordered = [];
  for (const id of change.effectIds) {
    const effect = effects.get(id);
    if (effect === undefined) return null;
    ordered.push(effect);
  }
  return { type: "sequence", document: { ...change.document, effects: ordered } };
}

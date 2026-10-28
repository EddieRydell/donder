import type { DocumentDescriptor, GuiDocumentRequest, GuiObjectRef, ObjectKind, OwnedObjectSlot } from "../types";

type Address = Pick<GuiObjectRef, "path" | "objectKey" | "ownedPath">;

function pathIdentity(path: OwnedObjectSlot[]) {
  return path.map((slot) => {
    if ("id" in slot) return { type: slot.type, id: slot.id };
    if ("name" in slot) return { type: slot.type, name: slot.name };
    return { type: slot.type };
  });
}

export function guiObjectKey(reference: GuiObjectRef): string {
  return JSON.stringify([reference.path, reference.objectKey, pathIdentity(reference.ownedPath)]);
}

export function sameGuiObject(a: GuiObjectRef, b: GuiObjectRef): boolean {
  return guiObjectKey(a) === guiObjectKey(b);
}

export function objectViewKey(reference: Address): string {
  return `${reference.path}::${JSON.stringify([reference.objectKey, pathIdentity(reference.ownedPath)])}`;
}

/** The kind of object a GUI request opens: its last owned slot's, else its declared object's. */
export function guiRequestKind(request: GuiDocumentRequest | null, descriptor: DocumentDescriptor | null): ObjectKind | null {
  if (request === null) return null;
  const slot = request.ownedPath[request.ownedPath.length - 1];
  if (slot !== undefined) return slot.type;
  return descriptor?.guiObjects.find((object) => object.key === request.objectKey)?.kind ?? null;
}

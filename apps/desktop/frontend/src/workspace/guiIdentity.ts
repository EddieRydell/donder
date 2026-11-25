import type { DocumentDescriptor, GuiDocumentRequest, GuiObjectRef, ObjectKind } from "../types";

type Address = Pick<GuiObjectRef, "path" | "objectKey">;

export function guiObjectKey(reference: GuiObjectRef): string {
  return JSON.stringify([reference.path, reference.objectKey]);
}

export function sameGuiObject(a: GuiObjectRef, b: GuiObjectRef): boolean {
  return guiObjectKey(a) === guiObjectKey(b);
}

export function objectViewKey(reference: Address): string {
  return `${reference.path}::${reference.objectKey}`;
}

/** The kind of the declared object a GUI request opens. */
export function guiRequestKind(request: GuiDocumentRequest | null, descriptor: DocumentDescriptor | null): ObjectKind | null {
  if (request === null) return null;
  return descriptor?.guiObjects.find((object) => object.key === request.objectKey)?.kind ?? null;
}

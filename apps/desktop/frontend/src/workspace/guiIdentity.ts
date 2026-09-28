import type { GuiObjectRef, GuiOwnedStep } from "../types";

type Address = Pick<GuiObjectRef, "path" | "objectKey" | "ownedPath">;

function pathIdentity(path: GuiOwnedStep[]) {
  return path.map((slot) => "id" in slot ? { type: slot.type, id: slot.id } : { type: slot.type });
}

export function guiObjectKey(reference: GuiObjectRef): string {
  return JSON.stringify([reference.moduleId, reference.path, reference.objectKey, pathIdentity(reference.ownedPath)]);
}

export function sameGuiObject(a: GuiObjectRef, b: GuiObjectRef): boolean {
  return guiObjectKey(a) === guiObjectKey(b);
}

export function objectViewKey(reference: Address): string {
  return `${reference.path}::${JSON.stringify([reference.objectKey, pathIdentity(reference.ownedPath)])}`;
}

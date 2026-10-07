import type { GuiObjectRef, GuiOwnedStep } from "../types";

type Address = Pick<GuiObjectRef, "path" | "objectKey" | "ownedPath">;

function pathIdentity(path: GuiOwnedStep[]) {
  return path.map((slot) => {
    if ("id" in slot) return { type: slot.type, id: slot.id };
    if ("name" in slot) return { type: slot.type, name: slot.name };
    return { type: slot.type };
  });
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

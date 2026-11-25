import { commands } from "../../api";
import { runGuiEditCommand } from "../../store";
import type { GuiDocumentRequest, GuiEdit, GuiEditUpdate, GuiField, GuiMember, GuiSite, GuiValue } from "../../types";

/** `field` of `member` of the open object; a null member is the object, a null field the whole member or object. */
export function site(member: GuiMember | null, field: GuiField | null = null): GuiSite {
  return { object: null, member, field };
}

/** Apply `edit` to the open object, or to the object `origin` opens; resolves to null when it was not applied. */
export function applyEdit(edit: GuiEdit, origin?: GuiDocumentRequest | null): Promise<GuiEditUpdate | null> {
  return runGuiEditCommand((request) => commands.applyGuiEdit(request, edit), origin);
}

/** Write `value` at `at`, or at each site of `at` together. */
export function put(at: GuiSite | GuiSite[], value: GuiValue, origin?: GuiDocumentRequest | null): Promise<GuiEditUpdate | null> {
  return applyEdit({ type: "put", sites: Array.isArray(at) ? at : [at], value }, origin);
}

/** Remove what `at` names; what named it gives way. */
export function remove(at: GuiSite): Promise<GuiEditUpdate | null> {
  return applyEdit({ type: "remove", site: at });
}

/** Name `member`, or the open object when it is null. */
export function rename(member: GuiMember | null, name: string): Promise<GuiEditUpdate | null> {
  return put(site(member, { type: "name" }), { type: "text", text: name });
}

/** Describe `member`, or the open object when it is null; empty text removes the description. */
export function describe(member: GuiMember | null, description: string | null): Promise<GuiEditUpdate | null> {
  return put(site(member, { type: "description" }), { type: "optionalText", text: description });
}

export function recolor(member: GuiMember, color: string): Promise<GuiEditUpdate | null> {
  return put(site(member, { type: "color" }), { type: "text", text: color });
}

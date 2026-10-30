import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { ConsoleAction, ConsoleClipEdit, ConsoleControlRef, ConsoleGuiDocument, ConsoleGuiEdit, ConsoleInputDevice } from "../../../types";
import type { CompositionEditing } from "../sequence/composition";
import type { GuiItemEdit } from "../shared";

/** Apply an edit to the open console; a rejected edit's error is shown and rethrown. */
export async function applyConsoleEdit(edit: ConsoleGuiEdit): Promise<void> {
  await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "console", edit }));
}

/** Rename, describe, color or delete one of the open console's items. */
export async function applyConsoleItemEdit(edit: GuiItemEdit): Promise<void> {
  await runGuiEditCommand((request) => commands.applyGuiEdit(request, edit));
}

export function editConsoleClip(id: number, edit: ConsoleClipEdit) {
  return applyConsoleEdit({ type: "editClip", id, edit });
}

export function setConsoleActions(id: number, actions: ConsoleAction[]) {
  return applyConsoleEdit({ type: "setActions", id, actions });
}

export function consoleComposition(document: ConsoleGuiDocument): CompositionEditing {
  return {
    document: { ...document, markCollections: document.liveMarks },
    layerUses: (layerId) => document.clips.filter((clip) => clip.layerId === layerId).length,
    apply: (edit, origin) => runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "console", edit: { type: "composition", edit } }), origin),
    applyItem: applyConsoleItemEdit,
    automation: null
  };
}

/** The first button, or axis, of the setup's first device that has one. */
export function firstControl(devices: ConsoleInputDevice[], axis: boolean): ConsoleControlRef | null {
  for (const device of devices) {
    const control = (axis ? device.axes : device.buttons)[0];
    if (control !== undefined) return { device: device.device, control: control.control };
  }
  return null;
}

/** How a clip plays: copies `Play` actions start, one copy that `Start`,
 * `Toggle` or `always` holds, or `null` while nothing plays it. */
export function clipUse(document: ConsoleGuiDocument, clipId: number): "played" | "held" | null {
  const actions = document.events.flatMap((on) => on.actions);
  if (actions.some((action) => action.type === "play" && action.clipId === clipId)) return "played";
  const held = document.always.includes(clipId)
    || actions.some((action) => (action.type === "start" || action.type === "toggle") && action.clipId === clipId);
  return held ? "held" : null;
}

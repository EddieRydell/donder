import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { ConsoleClipEdit, ConsoleControlRef, ConsoleEventKind, ConsoleGuiDocument, ConsoleGuiEdit, ConsoleInputControl, ConsoleInputDevice, ConsoleOn, GuiControlInput, GuiEditUpdate, GuiWindow, SequenceAutomationTarget } from "../../../types";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { automationTargetsEqual, type GuiFocus, type GuiItemEdit } from "../shared";

/** Apply an edit to the open console; a rejected edit's error is shown and rethrown. */
export async function applyConsoleEdit(edit: ConsoleGuiEdit): Promise<GuiEditUpdate> {
  return runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "console", edit }));
}

/** Rename, describe, color or delete one of the open console's items. */
export async function applyConsoleItemEdit(edit: GuiItemEdit): Promise<void> {
  await runGuiEditCommand((request) => commands.applyGuiEdit(request, edit));
}

export function editConsoleEffect(id: number, edit: ConsoleClipEdit) {
  return applyConsoleEdit({ type: "editClip", id, edit });
}

/** The console's reference to a control of an input device. */
export function controlRef(device: ConsoleInputDevice, control: ConsoleInputControl): ConsoleControlRef {
  return { device: device.device, control: control.control };
}

/** Whether `on` answers `event` of `control` of `device`. */
export function answers(on: ConsoleOn, event: ConsoleEventKind, device: ConsoleInputDevice, control: ConsoleInputControl): boolean {
  return on.event === event && on.input.control.control === control.control && guiObjectKey(on.input.control.device) === guiObjectKey(device.device);
}

/** A control's input as a short phrase: `A`, `Notes 52–72`, `CC 1`, `/tempo`. */
export function inputLabel(input: GuiControlInput): string {
  const channel = (value: number | null) => value === null ? "" : ` · channel ${value}`;
  switch (input.type) {
    case "key": return input.code.replace(/^(Key|Digit)/, "");
    case "notes": return `${input.low === input.high ? `Note ${input.low}` : `Notes ${input.low}–${input.high}`}${channel(input.channel)}`;
    case "controlChange": return `CC ${input.number}${channel(input.channel)}`;
    case "pitchBend": return `Pitch bend${channel(input.channel)}`;
    case "address":
    case "addressRange": return input.path;
  }
}

/** The action an edit that created the effect `name` left to select, or the effect itself when only it plays. */
export function focusOnEffect(update: GuiEditUpdate, name: string | undefined): GuiFocus {
  if (name === undefined || update.change.type !== "document" || update.change.document.type !== "console") return null;
  const document = update.change.document.document;
  const effect = document.clips.find((clip) => clip.name === name);
  if (effect === undefined) return null;
  for (const on of document.events) {
    const index = on.actions.findIndex((action) => action.type !== "stop" && "clipId" in action && action.clipId === effect.id);
    if (index >= 0) return { type: "consoleAction", on: on.id, index };
  }
  return { type: "consoleEffect", id: effect.id };
}

/** The effect and parameter, or operator and parameter, a `Set` writes: `flash.level`. */
export function targetLabel(document: ConsoleGuiDocument, target: SequenceAutomationTarget): string {
  if (target.type === "clipParam") return `${document.clips.find((clip) => clip.id === target.clipId)?.name ?? "Missing effect"}.${target.param}`;
  const node = document.compositionGraph.nodes.find((candidate) => candidate.id === target.nodeId);
  return `${node?.kind.type === "operator" ? node.kind.name : "Missing operator"}.${target.param}`;
}

/** The event that `Set`s `target`: the control that controls it. */
export function controllerOf(document: ConsoleGuiDocument, target: SequenceAutomationTarget): ConsoleOn | undefined {
  return document.events.find((on) => on.actions.some((action) => action.type === "set" && automationTargetsEqual(action.target, target)));
}

/** A binding that uses the whole range of its parameter. */
export const FULL_WINDOW: GuiWindow = { low: 0, high: 1 };

/** The window the `Set` of `target` writes through; the whole range while no control sets it. */
export function setWindowOf(document: ConsoleGuiDocument, target: SequenceAutomationTarget): GuiWindow {
  for (const on of document.events) {
    for (const action of on.actions) {
      if (action.type === "set" && automationTargetsEqual(action.target, target)) return action.window;
    }
  }
  return FULL_WINDOW;
}

/** A control as the key a select uses for it. */
export function controlKey(control: ConsoleControlRef): string {
  return `${guiObjectKey(control.device)}:${control.control}`;
}

/** The effects whose marks parameters read the live mark collection `key`. */
export function marksReaders(document: ConsoleGuiDocument, key: string): string[] {
  return document.clips.filter((clip) => clip.params.some((param) => param.value.type === "marks" && param.value.key === key)).map((clip) => clip.name);
}

import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { ConsoleAction, ConsoleControlRef, ConsoleEventKind, ConsoleGuiDocument, ConsoleGuiEdit, ConsoleInputControl, ConsoleInputDevice, ConsoleOn, ConsolePlayedValue, GuiClip, GuiControlInput, SequenceClipDetails, GuiEditUpdate, GuiWindow, SequenceAutomationTarget } from "../../../types";
import { put, site } from "../edits";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { automationTargetsEqual } from "../shared";
import { clipFocus, type GuiFocus } from "../documentUi";

/** Apply an edit to the open console; resolves to null when it was not applied. */
export async function applyConsoleEdit(edit: ConsoleGuiEdit): Promise<GuiEditUpdate | null> {
  return runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "console", edit }));
}

/** Give event `on` the actions `actions`. */
export function putActions(on: ConsoleOn, actions: ConsoleAction[]) {
  return put(site({ type: "event", id: on.id }), { type: "actions", actions });
}

/** Play a console effect for the whole session, repeating every `seconds`, or no longer. */
export function putAlways(clip: number, seconds: number | null) {
  return put(site({ type: "clip", id: clip }, { type: "always" }), { type: "optionalSeconds", seconds });
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
  return clipFocus(effect.id);
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

/** The value each copy of the effect takes for `param` from its press, if any. */
export function playedOf(clip: GuiClip, param: string): ConsolePlayedValue | undefined {
  return clip.placement.type === "played" ? clip.placement.values.find((value) => value.param === param) : undefined;
}

/** Whether a knob, a button or, for an effect's parameter, its presses set `target` live. */
export function isLinked(document: ConsoleGuiDocument, target: SequenceAutomationTarget, clip: GuiClip | null): boolean {
  return controllerOf(document, target) !== undefined || (clip !== null && playedOf(clip, target.param) !== undefined);
}

/** The controls whose events start a copy of `clip`. */
export function startersOf(document: ConsoleGuiDocument, clip: number): ConsoleOn[] {
  return document.events.filter((on) => on.actions.some((action) => (action.type === "play" || action.type === "start" || action.type === "toggle") && action.clipId === clip));
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

/** The effects whose marks parameters read the live mark collection `id`. */
export function marksReaders(document: ConsoleGuiDocument, details: Map<number, SequenceClipDetails>, id: number): string[] {
  return document.clips.filter((clip) => (details.get(clip.id)?.params ?? []).some((param) => param.value.type === "marks" && param.value.id === id)).map((clip) => clip.name);
}

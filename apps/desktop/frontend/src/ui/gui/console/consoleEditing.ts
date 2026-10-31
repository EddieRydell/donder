import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { ConsoleClipEdit, ConsoleControlRef, ConsoleEventKind, ConsoleGuiDocument, ConsoleGuiEdit, ConsoleInputControl, ConsoleInputDevice, ConsoleOn, GuiControlInput, GuiEditUpdate, SequenceAutomationTarget } from "../../../types";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import type { CompositionEditing } from "../sequence/composition";
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

export function consoleComposition(document: ConsoleGuiDocument): CompositionEditing {
  return {
    document: { ...document, markCollections: document.liveMarks },
    layerUses: (layerId) => document.clips.filter((clip) => clip.layerId === layerId).length,
    apply: (edit, origin) => runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "console", edit: { type: "composition", edit } }), origin),
    applyItem: applyConsoleItemEdit,
    automation: null
  };
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

/** The action an edit that linked `target` to a control left to select. */
export function focusOnSet(update: GuiEditUpdate, target: SequenceAutomationTarget): GuiFocus {
  if (update.change.type !== "document" || update.change.document.type !== "console") return null;
  for (const on of update.change.document.document.events) {
    const index = on.actions.findIndex((action) => action.type === "set" && automationTargetsEqual(action.target, target));
    if (index >= 0) return { type: "consoleAction", on: on.id, index };
  }
  return null;
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

/** A control as the key a select uses for it. */
export function controlKey(control: ConsoleControlRef): string {
  return `${guiObjectKey(control.device)}:${control.control}`;
}

/** A group of parameters a control can be linked to: those of one effect or operator. */
export type ParameterGroup = { label: string; params: { name: string; target: SequenceAutomationTarget }[] };

/** The parameters no control controls yet, and no played value sets, by effect then operator. */
export function freeParameters(document: ConsoleGuiDocument): { effects: ParameterGroup[]; operators: ParameterGroup[] } {
  const played = (clipId: number, param: string) => document.events.some((on) => on.actions.some((action) =>
    (action.type === "play" || action.type === "start" || action.type === "toggle") && action.clipId === clipId && action.values.some((value) => value.param === param)));
  const effects = document.clips.map((clip): ParameterGroup => ({
    label: clip.name,
    params: clip.params
      .filter((param) => param.supportsAutomation && !played(clip.id, param.name))
      .map((param) => ({ name: param.name, target: { type: "clipParam", clipId: clip.id, param: param.name } as const }))
      .filter(({ target }) => controllerOf(document, target) === undefined)
  })).filter((group) => group.params.length > 0);
  const operators = document.compositionGraph.nodes.flatMap((node): ParameterGroup[] => {
    if (node.kind.type !== "operator") return [];
    const params = node.kind.params
      .filter((param) => param.supportsAutomation)
      .map((param) => ({ name: param.name, target: { type: "compositionNodeParam", nodeId: node.id, param: param.name } as const }))
      .filter(({ target }) => controllerOf(document, target) === undefined);
    return params.length === 0 ? [] : [{ label: node.kind.name, params }];
  });
  return { effects, operators };
}

/** The effects whose marks parameters read the live mark collection `key`. */
export function marksReaders(document: ConsoleGuiDocument, key: string): string[] {
  return document.clips.filter((clip) => clip.params.some((param) => param.value.type === "marks" && param.value.key === key)).map((clip) => clip.name);
}

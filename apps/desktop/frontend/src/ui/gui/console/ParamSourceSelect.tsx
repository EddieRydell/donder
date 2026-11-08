import type { ConsoleClip, ConsoleGuiDocument, ConsoleParamSource, ConsolePlayedQuantity, SequenceAutomationTarget, SequenceParamRange } from "../../../types";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { Select } from "../../kit";
import { WindowFields } from "../WindowFields";
import { applyConsoleEdit, controllerOf, controlKey, controlRef, FULL_WINDOW, playedOf, setWindowOf, startersOf } from "./consoleEditing";

/** The select's value while nothing sets the parameter live. */
const UNLINKED = "unlinked";

/** A note's name needs a button that reads notes. */
const PLAYED: { value: ConsolePlayedQuantity; label: string; needsNote: boolean }[] = [
  { value: "velocity", label: "Velocity", needsNote: false },
  { value: "pitchClass", label: "Note name", needsNote: true }
];

/** The select's value for a played quantity, apart from control keys. */
const playedKey = (from: ConsolePlayedQuantity) => `played:${from}`;

/** What sets a parameter live: nothing, any knob of the setup's input
 * devices, or, for an effect's parameter, how the press that started each
 * copy was played. */
export function ParamSourceSelect({ document, target, clip, label, range }: {
  document: ConsoleGuiDocument;
  target: SequenceAutomationTarget;
  /** The effect whose parameter it is; an operator's parameter is not played. */
  clip: ConsoleClip | null;
  label: string;
  range: SequenceParamRange | null;
}) {
  const controller = controllerOf(document, target);
  const played = clip === null ? undefined : playedOf(clip, target.param);
  const value = played !== undefined ? playedKey(played.from) : controller === undefined ? UNLINKED : controlKey(controller.input.control);
  const window = played?.window ?? setWindowOf(document, target);
  const knobs = document.inputDevices.flatMap((device) => device.axes.map((axis) => ({ ref: controlRef(device, axis), label: `${device.name}.${axis.name}` })));
  const listed = knobs.some(({ ref }) => controlKey(ref) === value);
  // A copy is played only from a press, and from a note only when every control that starts it reads notes.
  const starters = clip === null ? [] : startersOf(document, clip.id);
  const pressed = clip !== null && !document.always.includes(clip.id) && starters.every((on) => on.event === "press");
  const notes = starters.every((on) => document.inputDevices.some((device) => guiObjectKey(device.device) === guiObjectKey(on.input.control.device)
    && device.buttons.some((button) => button.control === on.input.control.control && button.notes)));
  const link = (source: ConsoleParamSource | null, next = window) => void applyConsoleEdit({ type: "controlParam", target, source, window: next });
  const source = (key: string): ConsoleParamSource | null => {
    const from = PLAYED.find((choice) => playedKey(choice.value) === key);
    if (from !== undefined) return { type: "played", from: from.value };
    const knob = knobs.find(({ ref }) => controlKey(ref) === key);
    return knob === undefined ? null : { type: "control", control: knob.ref };
  };
  const current: ConsoleParamSource | null = played !== undefined ? { type: "played", from: played.from } : controller === undefined ? null : { type: "control", control: controller.input.control };
  return <>
    <Select name={`Live source for ${label}`} value={value} onChange={(key) => { link(source(key), current === null ? FULL_WINDOW : window); }} options={[
      { value: UNLINKED, label: "Not linked" },
      ...(clip === null ? [] : PLAYED.map((choice) => ({ value: playedKey(choice.value), label: choice.label, disabled: !pressed || (choice.needsNote && !notes) }))),
      ...(controller !== undefined && !listed ? [{ value, label: controller.input.controlName, disabled: true }] : []),
      ...knobs.map(({ ref, label: knob }) => ({ value: controlKey(ref), label: knob }))
    ]} />
    {current !== null && <WindowFields value={window} range={range} onChange={(next) => { link(current, next); }} />}
  </>;
}

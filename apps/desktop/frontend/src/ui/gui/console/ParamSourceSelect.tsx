import { Link2, Link2Off } from "lucide-react";
import type { ConsoleGuiDocument, GuiClip, ConsoleParamSource, ConsolePlayedQuantity, SequenceAutomationTarget, SequenceParamRange } from "../../../types";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { IconButton, MenuButton, MenuSelect, type MenuEntry } from "../../kit";
import { WindowFields } from "../WindowFields";
import { applyConsoleEdit, controllerOf, controlKey, controlRef, FULL_WINDOW, playedOf, setWindowOf, startersOf } from "./consoleEditing";

/** A note's name needs a button that reads notes. */
const PLAYED: Record<ConsolePlayedQuantity, { label: string; needsNote: boolean }> = {
  velocity: { label: "Velocity", needsNote: false },
  pitchClass: { label: "Note name", needsNote: true }
};

/** What sets a parameter live: nothing, any knob of the project's input
 * devices, or, for an effect's parameter, how the press that started each
 * copy was played. Unlinked, it is a link button that lists the sources, as a
 * sequence parameter's automation link is; linked, it names its source, with
 * the window it maps and an unlink button. */
export function ParamSourceSelect({ document, target, clip, label, range }: {
  document: ConsoleGuiDocument;
  target: SequenceAutomationTarget;
  /** The effect whose parameter it is; an operator's parameter is not played. */
  clip: GuiClip | null;
  label: string;
  range: SequenceParamRange | null;
}) {
  const controller = controllerOf(document, target);
  const played = clip === null ? undefined : playedOf(clip, target.param);
  const window = played?.window ?? setWindowOf(document, target);
  const knobs = document.inputDevices.flatMap((device) => device.axes.map((axis) => ({ ref: controlRef(device, axis), label: `${device.name}.${axis.name}` })));
  // A copy is played only from a press, and from a note only when every control that starts it reads notes.
  const starters = clip === null ? [] : startersOf(document, clip.id);
  const pressed = clip !== null && !document.always.includes(clip.id) && starters.every((on) => on.event === "press");
  const notes = starters.every((on) => document.inputDevices.some((device) => guiObjectKey(device.device) === guiObjectKey(on.input.control.device)
    && device.buttons.some((button) => button.control === on.input.control.control && button.notes)));
  // The source and its name; a control the project no longer lists keeps the name it was linked by.
  const linked: { source: ConsoleParamSource; name: string } | null = played !== undefined
    ? { source: { type: "played", from: played.from }, name: PLAYED[played.from].label }
    : controller === undefined ? null : {
      source: { type: "control", control: controller.input.control },
      name: knobs.find(({ ref }) => controlKey(ref) === controlKey(controller.input.control))?.label ?? controller.input.controlName
    };
  const current = linked?.source ?? null;
  const link = (source: ConsoleParamSource | null, next: typeof window) => void applyConsoleEdit({ type: "controlParam", target, source, window: next });
  // A new link maps the whole range; another source keeps the window.
  const choose = (source: ConsoleParamSource) => { link(source, current === null ? FULL_WINDOW : window); };
  const playedEntries: MenuEntry[] = clip === null ? [] : (Object.keys(PLAYED) as ConsolePlayedQuantity[]).map((from) => ({
    type: "action", label: PLAYED[from].label, disabled: !pressed || (PLAYED[from].needsNote && !notes),
    onSelect: () => { choose({ type: "played", from }); }
  }));
  const knobEntries: MenuEntry[] = knobs.map(({ ref, label: knob }) => ({ type: "action", label: knob, onSelect: () => { choose({ type: "control", control: ref }); } }));
  const entries: MenuEntry[] = [...playedEntries, ...(playedEntries.length > 0 && knobEntries.length > 0 ? [{ type: "separator" } as const] : []), ...knobEntries];
  const name = `Live source for ${label}`;
  if (linked === null) {
    return <MenuButton entries={entries}>
      <IconButton icon={Link2} label={name} disabled={entries.length === 0} />
    </MenuButton>;
  }
  return <>
    <MenuSelect label={linked.name} name={name} entries={entries} />
    <WindowFields value={window} range={range} onChange={(next) => { link(linked.source, next); }} />
    <IconButton icon={Link2Off} label="Unlink live source" onClick={() => { link(null, window); }} />
  </>;
}

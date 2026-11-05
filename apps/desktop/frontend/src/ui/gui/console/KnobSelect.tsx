import type { ConsoleGuiDocument, SequenceAutomationTarget, SequenceParamRange } from "../../../types";
import { Select } from "../../kit";
import { WindowFields } from "../WindowFields";
import { applyConsoleEdit, controllerOf, controlKey, controlRef, setWindowOf } from "./consoleEditing";

/** The select's value while no knob sets the parameter. */
const UNLINKED = "unlinked";

/** Which knob sets a parameter: none, or any axis of the setup's input devices. */
export function KnobSelect({ document, target, label, range }: { document: ConsoleGuiDocument; target: SequenceAutomationTarget; label: string; range: SequenceParamRange | null }) {
  const current = controllerOf(document, target);
  const value = current === undefined ? UNLINKED : controlKey(current.input.control);
  const knobs = document.inputDevices.flatMap((device) => device.axes.map((axis) => ({ ref: controlRef(device, axis), label: `${device.name}.${axis.name}` })));
  const listed = knobs.some(({ ref }) => controlKey(ref) === value);
  const bounds = setWindowOf(document, target);
  return <>
    <Select name={`Knob for ${label}`} value={value} onChange={(key) => {
      const chosen = knobs.find(({ ref }) => controlKey(ref) === key);
      void applyConsoleEdit({ type: "controlParam", target, control: chosen?.ref ?? null, window: bounds });
    }} options={[
      { value: UNLINKED, label: "No knob" },
      ...(current !== undefined && !listed ? [{ value, label: current.input.controlName, disabled: true }] : []),
      ...knobs.map(({ ref, label: knob }) => ({ value: controlKey(ref), label: knob }))
    ]} />
    {current !== undefined && <WindowFields value={bounds} range={range} onChange={(next) => { void applyConsoleEdit({ type: "controlParam", target, control: current.input.control, window: next }); }} />}
  </>;
}

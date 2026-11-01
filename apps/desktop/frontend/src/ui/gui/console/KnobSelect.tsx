import type { ConsoleGuiDocument, SequenceAutomationTarget, SequenceParamRange } from "../../../types";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { WindowFields } from "../WindowFields";
import { applyConsoleEdit, controllerOf, controlKey, controlRef, setWindowOf } from "./consoleEditing";

/** Which knob sets a parameter: none, or any axis of the setup's input devices. */
export function KnobSelect({ document, target, label, range }: { document: ConsoleGuiDocument; target: SequenceAutomationTarget; label: string; range: SequenceParamRange | null }) {
  const current = controllerOf(document, target);
  const value = current === undefined ? "" : controlKey(current.input.control);
  const knobs = document.inputDevices.map((device) => ({ device, refs: device.axes.map((axis) => ({ axis, ref: controlRef(device, axis) })) }));
  const listed = knobs.some(({ refs }) => refs.some(({ ref }) => controlKey(ref) === value));
  const bounds = setWindowOf(document, target);
  return <>
    <select aria-label={`Link ${label} to a knob`} value={value} title="Set this from a knob's position" onChange={(event) => {
    const key = event.currentTarget.value;
    const chosen = knobs.flatMap(({ refs }) => refs).find(({ ref }) => controlKey(ref) === key);
    void applyConsoleEdit({ type: "controlParam", target, control: chosen?.ref ?? null, window: bounds });
  }}>
    <option value="">Not linked</option>
    {current !== undefined && !listed && <option value={value} disabled>{current.input.controlName}</option>}
    {knobs.filter(({ refs }) => refs.length > 0).map(({ device, refs }) => <optgroup key={guiObjectKey(device.device)} label={device.name}>
      {refs.map(({ axis, ref }) => <option key={axis.control} value={controlKey(ref)}>{axis.name}</option>)}
    </optgroup>)}
  </select>
    {current !== undefined && <WindowFields value={bounds} range={range} onChange={(next) => { void applyConsoleEdit({ type: "controlParam", target, control: current.input.control, window: next }); }} />}
  </>;
}

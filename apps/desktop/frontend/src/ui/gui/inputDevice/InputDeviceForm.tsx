import { useState } from "react";
import type { GuiInputControl, GuiInputDriver, SetupInputDevice } from "../../../types";
import { DescriptionField } from "../DescriptionField";

/** The first `control_<n>` no other control of the device uses. */
function freshControlName(controls: GuiInputControl[]): string {
  const used = new Set(controls.map((control) => control.name));
  for (let index = controls.length + 1; ; index += 1) {
    const name = `control_${index}`;
    if (!used.has(name)) return name;
  }
}

/** A device's driver and controls, applied together. */
export function InputDeviceForm({ device, keyCodes, onSave }: {
  device?: SetupInputDevice;
  /** Every key a control can read, in the language's order. */
  keyCodes: string[];
  onSave: (driver: GuiInputDriver, controls: GuiInputControl[]) => Promise<void>;
}) {
  const [driver, setDriver] = useState<GuiInputDriver>(device?.driver ?? "keyboard");
  const [controls, setControls] = useState<GuiInputControl[]>(device?.controls ?? []);
  const update = (index: number, change: Partial<GuiInputControl>) => {
    setControls(controls.map((control, i) => i === index ? { ...control, ...change } : control));
  };
  const move = (index: number, offset: number) => {
    const next = [...controls];
    const [control] = next.splice(index, 1);
    if (control === undefined) return;
    next.splice(index + offset, 0, control);
    setControls(next);
  };
  const usedKeys = new Set(controls.map((control) => control.input.code));
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    void onSave(driver, controls);
  }}>
    <h4>{device === undefined ? "Add an input device" : "Input device settings"}</h4>
    <fieldset disabled={device?.readOnly === true}>
      <label>Driver<select value={driver} onChange={(event) => { if (event.target.value === "keyboard") setDriver(event.target.value); }}>
        <option value="keyboard">Keyboard</option>
      </select></label>
      {controls.length === 0 && <p className="object-overview-empty">No controls. Add a button for each key a rig plays from.</p>}
      {controls.map((control, index) => <div className="controller-port-row" key={control.id}>
        <label>Name<input required pattern="[a-z_][a-z0-9_]*" value={control.name} onChange={(event) => { update(index, { name: event.target.value }); }} /></label>
        <label>Key<select value={control.input.code} onChange={(event) => { update(index, { input: { type: "key", code: event.target.value } }); }}>
          {keyCodes.map((code) => <option key={code} value={code} disabled={code !== control.input.code && usedKeys.has(code)}>{code}</option>)}
        </select></label>
        <DescriptionField description={control.description} onCommit={(description) => { update(index, { description }); return Promise.resolve(); }} />
        <button type="button" disabled={index === 0} onClick={() => { move(index, -1); }}>Move up</button>
        <button type="button" disabled={index === controls.length - 1} onClick={() => { move(index, 1); }}>Move down</button>
        <button type="button" onClick={() => { setControls(controls.filter((_, i) => i !== index)); }}>Remove control</button>
      </div>)}
      <button type="button" disabled={keyCodes.every((code) => usedKeys.has(code))} onClick={() => {
        const code = keyCodes.find((candidate) => !usedKeys.has(candidate));
        if (code === undefined) return;
        const id = Math.max(0, ...controls.map((control) => control.id)) + 1;
        setControls([...controls, { id, name: freshControlName(controls), description: null, input: { type: "key", code } }]);
      }}>Add button</button>
      <button type="submit">{device === undefined ? "Add input device" : "Apply input device settings"}</button>
    </fieldset>
  </form>;
}

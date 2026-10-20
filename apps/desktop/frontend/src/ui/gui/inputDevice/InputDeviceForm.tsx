import { useState } from "react";
import type { GuiControlInput, GuiInputControl, GuiInputControlKind, GuiInputDriver, GuiInputLimits, SetupInputDevice } from "../../../types";
import { DescriptionField } from "../DescriptionField";

const MIDDLE_C = 60;
const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/** A MIDI note's name, where note 60 is middle C, C4. */
function noteName(note: number): string {
  return `${NOTE_NAMES[note % 12] ?? ""}${Math.floor(note / 12) - 1}`;
}

/** The first `control_<n>` no other control of the device uses. */
function freshControlName(controls: GuiInputControl[]): string {
  const used = new Set(controls.map((control) => control.name));
  for (let index = controls.length + 1; ; index += 1) {
    const name = `control_${index}`;
    if (!used.has(name)) return name;
  }
}

/** What an input reads, so two controls never read the same one. */
function inputKey(input: GuiControlInput): string {
  switch (input.type) {
    case "key": return `key ${input.code}`;
    case "note": return `note ${input.note}`;
    case "controlChange": return `cc ${input.number}`;
    case "pitchBend": return "pitch bend";
  }
}

/** The first input of `type` the device does not read yet, from `start`. */
function freeInput(type: "key" | "note" | "controlChange", used: Set<string>, keyCodes: string[], limits: GuiInputLimits, start = 0): GuiControlInput | null {
  if (type === "key") {
    const code = keyCodes.find((candidate) => !used.has(inputKey({ type: "key", code: candidate })));
    return code === undefined ? null : { type: "key", code };
  }
  const max = type === "note" ? limits.maxNote : limits.maxControlChange;
  for (let offset = 0; offset <= max; offset += 1) {
    const value = (start + offset) % (max + 1);
    const input: GuiControlInput = type === "note" ? { type: "note", note: value } : { type: "controlChange", number: value };
    if (!used.has(inputKey(input))) return input;
  }
  return null;
}

/** The controls carried over to another driver: each button reads a free
 * input of the new driver, and a keyboard has no dials. */
function convertControls(controls: GuiInputControl[], driver: GuiInputDriver, keyCodes: string[], limits: GuiInputLimits): GuiInputControl[] {
  const used = new Set<string>();
  const converted: GuiInputControl[] = [];
  for (const control of controls) {
    const fits = driver.type === "keyboard" ? control.input.type === "key" : control.input.type !== "key";
    const input = fits ? control.input
      : control.kind === "dial" ? null
        : freeInput(driver.type === "keyboard" ? "key" : "note", used, keyCodes, limits, MIDDLE_C);
    if (input === null) continue;
    used.add(inputKey(input));
    converted.push({ ...control, input });
  }
  return converted;
}

/** A device's driver and controls, applied together. */
export function InputDeviceForm({ device, keyCodes, limits, onSave }: {
  device?: SetupInputDevice;
  /** Every key a control can read, in the language's order. */
  keyCodes: string[];
  limits: GuiInputLimits;
  onSave: (driver: GuiInputDriver, controls: GuiInputControl[]) => Promise<void>;
}) {
  const [driver, setDriver] = useState<GuiInputDriver>(device?.driver ?? { type: "keyboard" });
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
  const used = new Set(controls.map((control) => inputKey(control.input)));
  const add = (kind: GuiInputControlKind, input: GuiControlInput | null) => {
    if (input === null) return;
    const id = Math.max(0, ...controls.map((control) => control.id)) + 1;
    setControls([...controls, { id, name: freshControlName(controls), description: null, kind, input }]);
  };
  const midi = driver.type === "midi";
  const nextKey = freeInput("key", used, keyCodes, limits);
  const nextNote = freeInput("note", used, keyCodes, limits, MIDDLE_C);
  const nextControlChange = freeInput("controlChange", used, keyCodes, limits, 1);
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    void onSave(driver, controls);
  }}>
    <h4>{device === undefined ? "Add an input device" : "Input device settings"}</h4>
    <fieldset disabled={device?.readOnly === true}>
      <label>Driver<select value={driver.type} onChange={(event) => {
        const next: GuiInputDriver = event.target.value === "midi" ? { type: "midi", port: null } : { type: "keyboard" };
        setDriver(next);
        setControls(convertControls(controls, next, keyCodes, limits));
      }}>
        <option value="keyboard">Keyboard</option>
        <option value="midi">MIDI</option>
      </select></label>
      {driver.type === "midi" && <label>Port<input value={driver.port ?? ""} placeholder="Every MIDI port" onChange={(event) => {
        setDriver({ type: "midi", port: event.target.value === "" ? null : event.target.value });
      }} /></label>}
      {controls.length === 0 && <p className="object-overview-empty">
        {midi ? "No controls. Add a button for each note or pedal a rig plays from, and a dial for each knob." : "No controls. Add a button for each key a rig plays from."}
      </p>}
      {controls.map((control, index) => <div className="controller-port-row" key={control.id}>
        <label>Name<input required pattern="[a-z_][a-z0-9_]*" value={control.name} onChange={(event) => { update(index, { name: event.target.value }); }} /></label>
        {midi
          ? <MidiInputFields control={control} used={used} limits={limits} onChange={(change) => { update(index, change); }} />
          : control.input.type === "key" && <label>Key<select value={control.input.code} onChange={(event) => { update(index, { input: { type: "key", code: event.target.value } }); }}>
            {keyCodes.map((code) => <option key={code} value={code} disabled={control.input.type === "key" && code !== control.input.code && used.has(inputKey({ type: "key", code }))}>{code}</option>)}
          </select></label>}
        <DescriptionField description={control.description} onCommit={(description) => { update(index, { description }); return Promise.resolve(); }} />
        <button type="button" disabled={index === 0} onClick={() => { move(index, -1); }}>Move up</button>
        <button type="button" disabled={index === controls.length - 1} onClick={() => { move(index, 1); }}>Move down</button>
        <button type="button" onClick={() => { setControls(controls.filter((_, i) => i !== index)); }}>Remove control</button>
      </div>)}
      {midi
        ? <>
          <button type="button" disabled={nextNote === null} onClick={() => { add("button", nextNote); }}>Add button</button>
          <button type="button" disabled={nextControlChange === null} onClick={() => { add("dial", nextControlChange); }}>Add dial</button>
        </>
        : <button type="button" disabled={nextKey === null} onClick={() => { add("button", nextKey); }}>Add button</button>}
      <button type="submit">{device === undefined ? "Add input device" : "Apply input device settings"}</button>
    </fieldset>
  </form>;
}

/** A MIDI control's kind and the note, control change or pitch bend it reads. */
function MidiInputFields({ control, used, limits, onChange }: {
  control: GuiInputControl;
  used: Set<string>;
  limits: GuiInputLimits;
  onChange: (change: Partial<GuiInputControl>) => void;
}) {
  const { input } = control;
  const taken = (candidate: GuiControlInput) => inputKey(candidate) !== inputKey(input) && used.has(inputKey(candidate));
  return <>
    <label>Kind<select value={control.kind} onChange={(event) => {
      const kind = event.target.value === "dial" ? "dial" : "button";
      // A dial reads no notes and a button no pitch bend.
      const fits = kind === "dial" ? input.type !== "note" : input.type !== "pitchBend";
      onChange({ kind, input: fits ? input : { type: "controlChange", number: 1 } });
    }}>
      <option value="button">Button</option>
      <option value="dial">Dial</option>
    </select></label>
    <label>Input<select value={input.type} onChange={(event) => {
      const type = event.target.value;
      onChange({ input: type === "note" ? { type: "note", note: MIDDLE_C } : type === "pitchBend" ? { type: "pitchBend" } : { type: "controlChange", number: 1 } });
    }}>
      {control.kind === "button" && <option value="note">Note</option>}
      <option value="controlChange">Control change</option>
      {control.kind === "dial" && <option value="pitchBend" disabled={taken({ type: "pitchBend" })}>Pitch bend</option>}
    </select></label>
    {input.type === "note" && <label>Note<input type="number" min={0} max={limits.maxNote} required value={input.note}
      title={noteName(input.note)} aria-invalid={taken(input)}
      onChange={(event) => { onChange({ input: { type: "note", note: Number(event.target.value) } }); }} /><span>{noteName(input.note)}</span></label>}
    {input.type === "controlChange" && <label>Number<input type="number" min={0} max={limits.maxControlChange} required value={input.number}
      aria-invalid={taken(input)}
      onChange={(event) => { onChange({ input: { type: "controlChange", number: Number(event.target.value) } }); }} /></label>}
  </>;
}

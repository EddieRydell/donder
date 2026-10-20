import { useState } from "react";
import type { GuiControlInput, GuiInputControl, GuiInputControlKind, GuiInputDriver, GuiInputLimits, SetupInputDevice } from "../../../types";
import { DescriptionField } from "../DescriptionField";

const MIDDLE_C = 60;
/** A full piano's lowest and highest keys, A0 and C8. */
const PIANO_LOW = 21;
const PIANO_HIGH = 108;
const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
/** The UDP port a new OSC device listens on, and the highest there is. */
const DEFAULT_OSC_PORT = 9000;
const MAX_UDP_PORT = 65535;

type DriverType = GuiInputDriver["type"];

const DRIVERS: { type: DriverType; label: string }[] = [
  { type: "keyboard", label: "Keyboard" },
  { type: "midi", label: "MIDI" },
  { type: "osc", label: "OSC" }
];

/** The kinds of control a driver has: a keyboard has only buttons, and OSC has no keys. */
function driverHas(driver: DriverType, kind: GuiInputControlKind): boolean {
  switch (driver) {
    case "keyboard": return kind === "button";
    case "midi": return true;
    case "osc": return kind !== "keys";
  }
}

/** Whether `driver` reads `input`. */
function driverReads(driver: DriverType, input: GuiControlInput): boolean {
  switch (input.type) {
    case "key": return driver === "keyboard";
    case "note":
    case "notes":
    case "controlChange":
    case "pitchBend": return driver === "midi";
    case "address":
    case "addressRange": return driver === "osc";
  }
}

/** The kinds of control `driver` has none of but the device has, in the plural. */
function droppedKinds(controls: GuiInputControl[], driver: DriverType): string[] {
  return ([["dial", "dials"], ["keys", "keys"]] as const)
    .filter(([kind]) => !driverHas(driver, kind) && controls.some((control) => control.kind === kind))
    .map(([, plural]) => plural);
}

/** A MIDI note's name, where note 60 is middle C, C4. */
function noteName(note: number): string {
  return `${NOTE_NAMES[note % 12] ?? ""}${Math.floor(note / 12) - 1}`;
}

/** A number field's value within `0..=max`, or `null` while the field is empty. */
function fieldNumber(value: string, max: number): number | null {
  return value === "" ? null : Math.min(max, Math.max(0, Math.trunc(Number(value))));
}

/** Whether `path` is an address OSC matches exactly: a `/`, then printable
 * characters other than spaces and the pattern characters `*?[]{}#,`. */
function validAddress(path: string): boolean {
  return /^\/[!-~]*$/.test(path) && !/[*?[\]{}#,]/.test(path);
}

/** The first `control_<n>` no other control of the device uses. */
function freshControlName(controls: GuiInputControl[]): string {
  const used = new Set(controls.map((control) => control.name));
  for (let index = controls.length + 1; ; index += 1) {
    const name = `control_${index}`;
    if (!used.has(name)) return name;
  }
}

/** What an input reads, one entry per note of a range, so two controls never
 * read the same input. */
function inputKeys(input: GuiControlInput): string[] {
  switch (input.type) {
    case "key": return [`key ${input.code}`];
    case "note": return [`note ${input.note}`];
    case "notes": return Array.from({ length: Math.max(0, input.high - input.low + 1) }, (_, offset) => `note ${input.low + offset}`);
    case "controlChange": return [`cc ${input.number}`];
    case "pitchBend": return ["pitch bend"];
    case "address":
    case "addressRange": return [`address ${input.path.trim()}`];
  }
}

function inputKey(input: GuiControlInput): string {
  return inputKeys(input).join(" ");
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

/** An OSC input for a control of `kind` named `name`: `/name`, or `/name_<n>`
 * when the device already reads that address. A dial's value runs from 0 to 1. */
function oscInput(kind: GuiInputControlKind, name: string, used: Set<string>): GuiControlInput {
  for (let index = 1; ; index += 1) {
    const path = index === 1 ? `/${name}` : `/${name}_${index}`;
    if (used.has(inputKey({ type: "address", path }))) continue;
    return kind === "dial" ? { type: "addressRange", path, low: 0, high: 1 } : { type: "address", path };
  }
}

/** A free input of `driver` for a control that reads another driver's. */
function convertedInput(control: GuiInputControl, driver: DriverType, used: Set<string>, keyCodes: string[], limits: GuiInputLimits): GuiControlInput | null {
  switch (driver) {
    case "keyboard": return freeInput("key", used, keyCodes, limits);
    case "midi": return control.kind === "dial" ? freeInput("controlChange", used, keyCodes, limits, 1) : freeInput("note", used, keyCodes, limits, MIDDLE_C);
    case "osc": return oscInput(control.kind, control.name, used);
  }
}

/** The controls carried over to another driver, each reading a free input of
 * it. The driver has every kind of control the device has. */
function convertControls(controls: GuiInputControl[], driver: GuiInputDriver, keyCodes: string[], limits: GuiInputLimits): GuiInputControl[] {
  const used = new Set<string>();
  const converted: GuiInputControl[] = [];
  for (const control of controls) {
    const input = driverReads(driver.type, control.input) ? control.input : convertedInput(control, driver.type, used, keyCodes, limits);
    if (input === null) continue;
    for (const key of inputKeys(input)) used.add(key);
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
  const used = new Set(controls.flatMap((control) => inputKeys(control.input)));
  const readByOthers = (index: number) => new Set(controls.flatMap((control, i) => i === index ? [] : inputKeys(control.input)));
  const nextName = freshControlName(controls);
  const add = (kind: GuiInputControlKind, input: GuiControlInput | null) => {
    if (input === null) return;
    const id = Math.max(0, ...controls.map((control) => control.id)) + 1;
    setControls([...controls, { id, name: nextName, description: null, kind, input }]);
  };
  const nextKey = freeInput("key", used, keyCodes, limits);
  const nextNote = freeInput("note", used, keyCodes, limits, MIDDLE_C);
  const nextControlChange = freeInput("controlChange", used, keyCodes, limits, 1);
  const piano: GuiControlInput = { type: "notes", low: PIANO_LOW, high: PIANO_HIGH };
  const pianoFree = inputKeys(piano).every((key) => !used.has(key));
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    void onSave(driver, controls);
  }}>
    <h4>{device === undefined ? "Add an input device" : "Input device settings"}</h4>
    <fieldset disabled={device?.readOnly === true}>
      <label>Driver<select value={driver.type} onChange={(event) => {
        const type = event.target.value;
        const next: GuiInputDriver = type === "midi" ? { type: "midi", port: null }
          : type === "osc" ? { type: "osc", port: DEFAULT_OSC_PORT }
            : { type: "keyboard" };
        setDriver(next);
        setControls(convertControls(controls, next, keyCodes, limits));
      }}>
        {DRIVERS.map(({ type, label }) => {
          // Switching would drop the controls the driver has no kind for.
          const dropped = droppedKinds(controls, type);
          return <option key={type} value={type} disabled={dropped.length > 0}>
            {dropped.length === 0 ? label : `${label} (remove the ${dropped.join(" and ")} first)`}
          </option>;
        })}
      </select></label>
      {driver.type === "midi" && <label>Port<input value={driver.port ?? ""} placeholder="Every MIDI port" onChange={(event) => {
        setDriver({ type: "midi", port: event.target.value === "" ? null : event.target.value });
      }} /></label>}
      {driver.type === "osc" && <label>UDP port<input type="number" min={1} max={MAX_UDP_PORT} required value={driver.port}
        aria-invalid={driver.port < 1}
        onChange={(event) => {
          const port = fieldNumber(event.target.value, MAX_UDP_PORT);
          if (port !== null) setDriver({ type: "osc", port });
        }} /></label>}
      {controls.length === 0 && <p className="object-overview-empty">
        {driver.type === "midi" ? "No controls. Add keys for a keyboard's notes, a button for each pad or pedal a rig plays from, and a dial for each knob."
          : driver.type === "osc" ? "No controls. Add a button for each OSC address that presses something, and a dial for each address that sends a value."
            : "No controls. Add a button for each key a rig plays from."}
      </p>}
      {controls.map((control, index) => <div className="controller-port-row" key={control.id}>
        <label>Name<input required pattern="[a-z_][a-z0-9_]*" value={control.name} onChange={(event) => { update(index, { name: event.target.value }); }} /></label>
        {driver.type === "midi" && <MidiInputFields control={control} used={used} limits={limits} onChange={(change) => { update(index, change); }} />}
        {driver.type === "osc" && <OscInputFields control={control} taken={readByOthers(index).has(inputKey(control.input))} onChange={(change) => { update(index, change); }} />}
        {driver.type === "keyboard" && control.input.type === "key" && <label>Key<select value={control.input.code} onChange={(event) => { update(index, { input: { type: "key", code: event.target.value } }); }}>
          {keyCodes.map((code) => <option key={code} value={code} disabled={control.input.type === "key" && code !== control.input.code && used.has(inputKey({ type: "key", code }))}>{code}</option>)}
        </select></label>}
        <DescriptionField description={control.description} onCommit={(description) => { update(index, { description }); return Promise.resolve(); }} />
        <button type="button" disabled={index === 0} onClick={() => { move(index, -1); }}>Move up</button>
        <button type="button" disabled={index === controls.length - 1} onClick={() => { move(index, 1); }}>Move down</button>
        <button type="button" onClick={() => { setControls(controls.filter((_, i) => i !== index)); }}>Remove control</button>
      </div>)}
      {driver.type === "midi" && <>
        <button type="button" disabled={nextNote === null} onClick={() => { add("button", nextNote); }}>Add button</button>
        <button type="button" disabled={nextControlChange === null} onClick={() => { add("dial", nextControlChange); }}>Add dial</button>
        <button type="button" disabled={!pianoFree} onClick={() => { add("keys", piano); }}>Add keys</button>
      </>}
      {driver.type === "osc" && <>
        <button type="button" onClick={() => { add("button", oscInput("button", nextName, used)); }}>Add button</button>
        <button type="button" onClick={() => { add("dial", oscInput("dial", nextName, used)); }}>Add dial</button>
      </>}
      {driver.type === "keyboard" && <button type="button" disabled={nextKey === null} onClick={() => { add("button", nextKey); }}>Add button</button>}
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
  const own = new Set(inputKeys(input));
  const taken = (candidate: GuiControlInput) => inputKeys(candidate).some((key) => !own.has(key) && used.has(key));
  return <>
    <label>Kind<select value={control.kind} onChange={(event) => {
      const kind = event.target.value === "dial" ? "dial" : event.target.value === "keys" ? "keys" : "button";
      // Keys read a range of notes, a dial no notes, and a button no pitch bend.
      if (kind === "keys") onChange({ kind, input: { type: "notes", low: PIANO_LOW, high: PIANO_HIGH } });
      else {
        const fits = kind === "dial" ? input.type === "controlChange" || input.type === "pitchBend" : input.type === "note" || input.type === "controlChange";
        onChange({ kind, input: fits ? input : { type: "controlChange", number: 1 } });
      }
    }}>
      <option value="button">Button</option>
      <option value="dial">Dial</option>
      <option value="keys">Keys</option>
    </select></label>
    {input.type === "notes" && <>
      <label>Lowest<input type="number" min={0} max={limits.maxNote} required value={input.low} aria-invalid={taken(input)}
        onChange={(event) => {
          const low = fieldNumber(event.target.value, limits.maxNote);
          if (low !== null) onChange({ input: { ...input, low } });
        }} /><span>{noteName(input.low)}</span></label>
      <label>Highest<input type="number" min={0} max={limits.maxNote} required value={input.high} aria-invalid={taken(input) || input.high < input.low}
        onChange={(event) => {
          const high = fieldNumber(event.target.value, limits.maxNote);
          if (high !== null) onChange({ input: { ...input, high } });
        }} /><span>{noteName(input.high)}</span></label>
    </>}
    {control.kind !== "keys" && <label>Input<select value={input.type} onChange={(event) => {
      const type = event.target.value;
      onChange({ input: type === "note" ? { type: "note", note: MIDDLE_C } : type === "pitchBend" ? { type: "pitchBend" } : { type: "controlChange", number: 1 } });
    }}>
      {control.kind === "button" && <option value="note">Note</option>}
      <option value="controlChange">Control change</option>
      {control.kind === "dial" && <option value="pitchBend" disabled={taken({ type: "pitchBend" })}>Pitch bend</option>}
    </select></label>}
    {input.type === "note" && <label>Note<input type="number" min={0} max={limits.maxNote} required value={input.note}
      title={noteName(input.note)} aria-invalid={taken(input)}
      onChange={(event) => {
        const note = fieldNumber(event.target.value, limits.maxNote);
        if (note !== null) onChange({ input: { type: "note", note } });
      }} /><span>{noteName(input.note)}</span></label>}
    {input.type === "controlChange" && <label>Number<input type="number" min={0} max={limits.maxControlChange} required value={input.number}
      aria-invalid={taken(input)}
      onChange={(event) => {
        const number = fieldNumber(event.target.value, limits.maxControlChange);
        if (number !== null) onChange({ input: { type: "controlChange", number } });
      }} /></label>}
  </>;
}

/** An OSC control's kind, the address it reads, and the range a dial's value runs over. */
function OscInputFields({ control, taken, onChange }: {
  control: GuiInputControl;
  /** Whether another control of the device reads the address. */
  taken: boolean;
  onChange: (change: Partial<GuiInputControl>) => void;
}) {
  const { input } = control;
  if (input.type !== "address" && input.type !== "addressRange") return null;
  const { path } = input;
  return <>
    <label>Kind<select value={control.kind} onChange={(event) => {
      const kind = event.target.value === "dial" ? "dial" : "button";
      onChange({ kind, input: kind === "dial" ? { type: "addressRange", path, low: 0, high: 1 } : { type: "address", path } });
    }}>
      <option value="button">Button</option>
      <option value="dial">Dial</option>
    </select></label>
    <label>Address<input required value={path} placeholder="/chord/change" aria-invalid={!validAddress(path.trim()) || taken}
      onChange={(event) => { onChange({ input: { ...input, path: event.target.value } }); }} /></label>
    {input.type === "addressRange" && <>
      <label>From<input key={`low ${input.low}`} type="number" step="any" required defaultValue={input.low} aria-invalid={!(input.low < input.high)}
        onBlur={(event) => {
          const low = Number(event.currentTarget.value);
          if (event.currentTarget.value !== "" && Number.isFinite(low) && low !== input.low) onChange({ input: { ...input, low } });
        }} /></label>
      <label>To<input key={`high ${input.high}`} type="number" step="any" required defaultValue={input.high} aria-invalid={!(input.low < input.high)}
        onBlur={(event) => {
          const high = Number(event.currentTarget.value);
          if (event.currentTarget.value !== "" && Number.isFinite(high) && high !== input.high) onChange({ input: { ...input, high } });
        }} /></label>
    </>}
  </>;
}

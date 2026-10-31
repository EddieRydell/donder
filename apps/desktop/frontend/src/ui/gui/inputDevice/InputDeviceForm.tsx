import { CircleDot, GripVertical, Plus, X } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { commands } from "../../../api";
import { useAppStore } from "../../../store";
import type { GuiControlInput, GuiInputControl, GuiInputControlKind, GuiInputDriver, GuiInputLimits, LearnedInput, SetupInputDevice } from "../../../types";
import { useConfirmDialog } from "../../ConfirmDialog";
import { DescriptionField } from "../DescriptionField";

const MIDDLE_C = 60;
/** The UDP port a new OSC device listens on, and the highest there is. */
const DEFAULT_OSC_PORT = 9000;
const MAX_UDP_PORT = 65535;

type DriverType = GuiInputDriver["type"];

/** What is being learned from the controller: a whole input, one note of a range, or a control change number. */
type LearnPart = "input" | "low" | "high" | "number";

const LEARN_HINTS: Record<DriverType, string> = {
  keyboard: "Press a key…",
  midi: "Press a key or pad, or move a knob…",
  osc: "Send an OSC message…"
};

/** Names each learning request, so a late cancel cannot end a newer one. */
let learnRequests = 0;

/** Whether what a device sent is what `part` listens for. */
function learnAccepts(input: GuiControlInput, part: LearnPart): boolean {
  switch (part) {
    case "input": return true;
    case "low":
    case "high": return input.type === "notes";
    case "number": return input.type === "controlChange";
  }
}

/** A control with what it learned. A whole input also sets the kind; a MIDI input reads any channel unless another control already reads that input there. */
function learnControl(control: GuiInputControl, part: LearnPart, learned: LearnedInput, others: Set<string>, limits: GuiInputLimits): GuiInputControl {
  const { input } = control;
  const heard = learned.input;
  if (part === "low" && input.type === "notes" && heard.type === "notes") return { ...control, input: { ...input, low: heard.low, high: Math.max(input.high, heard.low) } };
  if (part === "high" && input.type === "notes" && heard.type === "notes") return { ...control, input: { ...input, high: heard.low, low: Math.min(input.low, heard.low) } };
  if (part === "number" && input.type === "controlChange" && heard.type === "controlChange") return { ...control, input: { ...input, number: heard.number } };
  if (part !== "input") return control;
  if (heard.type === "notes" || heard.type === "controlChange" || heard.type === "pitchBend") {
    const anyChannel = { ...heard, channel: null };
    return { ...control, kind: learned.kind, input: isFree(anyChannel, others, limits) ? anyChannel : heard };
  }
  // A rebound axis keeps the range it had.
  if (heard.type === "addressRange" && input.type === "addressRange") return { ...control, kind: learned.kind, input: { ...heard, low: input.low, high: input.high } };
  return { ...control, kind: learned.kind, input: heard };
}

const DRIVERS: { type: DriverType; label: string }[] = [
  { type: "keyboard", label: "Keyboard" },
  { type: "midi", label: "MIDI" },
  { type: "osc", label: "OSC" }
];

const NAME_COLUMN = { label: "Name", hint: "The name a console refers to this control by." };
const DESCRIPTION_COLUMN = { label: "Description", hint: "Notes for yourself; shown wherever the control is listed." };

/** The columns of a driver's control grid, before the remove button and drag handle. */
const GRID_COLUMNS: Record<DriverType, { label: string; hint: string }[]> = {
  keyboard: [NAME_COLUMN, { label: "Input", hint: "The physical key; it stays the same on any keyboard layout." }, DESCRIPTION_COLUMN],
  midi: [
    NAME_COLUMN,
    { label: "Kind", hint: "A button is pressed and released; an axis is a position from 0 to 1." },
    { label: "Input", hint: "What the control listens to: notes (a pad, or a range of keys; 60 is middle C), a control change (a knob, slider or pedal, numbered 0 to 119), or the pitch bend wheel." },
    { label: "Channel", hint: "The MIDI channel to read, or any." },
    DESCRIPTION_COLUMN
  ],
  osc: [
    NAME_COLUMN,
    { label: "Kind", hint: "A button is pressed and released; an axis is a value." },
    { label: "Input", hint: "The OSC address messages are sent to, matched exactly, such as /chord/change. An axis also has the range its value runs over: the first number reads as 0 and the second as 1." },
    DESCRIPTION_COLUMN
  ]
};

/** The kinds of control a driver has: a keyboard has only buttons. */
function driverHas(driver: DriverType, kind: GuiInputControlKind): boolean {
  return driver !== "keyboard" || kind === "button";
}

/** Whether `driver` reads `input`. */
function driverReads(driver: DriverType, input: GuiControlInput): boolean {
  switch (input.type) {
    case "key": return driver === "keyboard";
    case "notes":
    case "controlChange":
    case "pitchBend": return driver === "midi";
    case "address":
    case "addressRange": return driver === "osc";
  }
}

/** The kinds of control `driver` has none of but the device has, in the plural. */
function droppedKinds(controls: GuiInputControl[], driver: DriverType): string[] {
  return ([["axis", "axes"]] as const)
    .filter(([kind]) => !driverHas(driver, kind) && controls.some((control) => control.kind === kind))
    .map(([, plural]) => plural);
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

/** The channels a MIDI input reads: its own, or every channel when it has none. */
function channelsRead(channel: number | null, limits: GuiInputLimits): number[] {
  return channel === null ? Array.from({ length: limits.maxChannel }, (_, index) => index + 1) : [channel];
}

/** What an input reads, one entry per note of a range and per channel it
 * reads, so two controls never read the same input on a channel both read. */
function inputKeys(input: GuiControlInput, limits: GuiInputLimits): string[] {
  const onChannels = (reads: string[], channel: number | null) =>
    channelsRead(channel, limits).flatMap((read) => reads.map((key) => `${key} channel ${read}`));
  switch (input.type) {
    case "key": return [`key ${input.code}`];
    case "notes": return onChannels(Array.from({ length: Math.max(0, input.high - input.low + 1) }, (_, offset) => `note ${input.low + offset}`), input.channel);
    case "controlChange": return onChannels([`cc ${input.number}`], input.channel);
    case "pitchBend": return onChannels(["pitch bend"], input.channel);
    case "address":
    case "addressRange": return [`address ${input.path.trim()}`];
  }
}

/** Whether `input` reads nothing in `used`. */
function isFree(input: GuiControlInput, used: Set<string>, limits: GuiInputLimits): boolean {
  return inputKeys(input, limits).every((key) => !used.has(key));
}

/** The first input of `type` the device does not read yet, from `start`; a
 * note is a range of one, on every channel. */
function freeInput(type: "key" | "note" | "controlChange", used: Set<string>, keyCodes: string[], limits: GuiInputLimits, start = 0): GuiControlInput | null {
  if (type === "key") {
    const code = keyCodes.find((candidate) => isFree({ type: "key", code: candidate }, used, limits));
    return code === undefined ? null : { type: "key", code };
  }
  const max = type === "note" ? limits.maxNote : limits.maxControlChange;
  for (let offset = 0; offset <= max; offset += 1) {
    const value = (start + offset) % (max + 1);
    const input: GuiControlInput = type === "note" ? { type: "notes", low: value, high: value, channel: null } : { type: "controlChange", number: value, channel: null };
    if (isFree(input, used, limits)) return input;
  }
  return null;
}

/** An OSC input for a control of `kind` named `name`: `/name`, or `/name_<n>`
 * when the device already reads that address. An axis's value runs from 0 to 1. */
function oscInput(kind: GuiInputControlKind, name: string, used: Set<string>, limits: GuiInputLimits): GuiControlInput {
  for (let index = 1; ; index += 1) {
    const path = index === 1 ? `/${name}` : `/${name}_${index}`;
    if (!isFree({ type: "address", path }, used, limits)) continue;
    return kind === "axis" ? { type: "addressRange", path, low: 0, high: 1 } : { type: "address", path };
  }
}

/** A free input of `driver` for a control that reads another driver's. */
function convertedInput(control: GuiInputControl, driver: DriverType, used: Set<string>, keyCodes: string[], limits: GuiInputLimits): GuiControlInput | null {
  switch (driver) {
    case "keyboard": return freeInput("key", used, keyCodes, limits);
    case "midi": return control.kind === "axis" ? freeInput("controlChange", used, keyCodes, limits, 1) : freeInput("note", used, keyCodes, limits, MIDDLE_C);
    case "osc": return oscInput(control.kind, control.name, used, limits);
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
    for (const key of inputKeys(input, limits)) used.add(key);
    converted.push({ ...control, input });
  }
  return converted;
}

const NAME_PATTERN = /^[a-z_][a-z0-9_]*$/;

/** Whether an input is well formed: a note range runs up, an address is one OSC matches, an axis range runs up. */
function inputValid(input: GuiControlInput): boolean {
  switch (input.type) {
    case "notes": return input.low <= input.high;
    case "address": return validAddress(input.path.trim());
    case "addressRange": return validAddress(input.path.trim()) && input.low < input.high;
    case "key":
    case "controlChange":
    case "pitchBend": return true;
  }
}

/** Whether a device can be saved: names are unique snake_case, no input is read twice, inputs are well formed, an OSC port is usable. */
function deviceValid(driver: GuiInputDriver, controls: GuiInputControl[], limits: GuiInputLimits): boolean {
  const keys = controls.flatMap((control) => inputKeys(control.input, limits));
  return new Set(controls.map((control) => control.name)).size === controls.length
    && controls.every((control) => NAME_PATTERN.test(control.name) && inputValid(control.input))
    && new Set(keys).size === keys.length
    && (driver.type !== "osc" || (driver.port >= 1 && driver.port <= MAX_UDP_PORT));
}

/** A device's driver and controls. A new device is applied together; an existing one saves each change as it is made, so each is its own undo step. */
export function InputDeviceForm({ device, keyCodes, limits, onSave }: {
  device?: SetupInputDevice;
  /** Every key a control can read, in the language's order. */
  keyCodes: string[];
  limits: GuiInputLimits;
  onSave: (driver: GuiInputDriver, controls: GuiInputControl[]) => Promise<void>;
}) {
  const [driver, setDriver] = useState<GuiInputDriver>(device?.driver ?? { type: "keyboard" });
  const [controls, setControls] = useState<GuiInputControl[]>(device?.controls ?? []);
  const { confirm, dialog: confirmDialog } = useConfirmDialog();
  // Follow the saved device when it changes elsewhere, as an undo does.
  const saved = device === undefined ? null : JSON.stringify([device.driver, device.controls]);
  const [seen, setSeen] = useState(saved);
  if (saved !== seen) {
    setSeen(saved);
    if (device !== undefined) {
      setDriver(device.driver);
      setControls(device.controls);
    }
  }
  const save = () => {
    if (device === undefined || !deviceValid(driver, controls, limits) || JSON.stringify([driver, controls]) === saved) return;
    void onSave(driver, controls);
  };
  // A change made through a select or button saves once its state has rendered.
  const saveAfterRender = useRef(false);
  useEffect(() => {
    if (!saveAfterRender.current) return;
    saveAfterRender.current = false;
    save();
  });
  const update = (index: number, change: Partial<GuiInputControl>) => {
    setControls(controls.map((control, i) => i === index ? { ...control, ...change } : control));
  };
  const [learning, setLearning] = useState<{ id: number; part: LearnPart } | null>(null);
  const learningId = learning?.id;
  const learningPart = learning?.part;
  // Listens for the next key, note, knob or message while a control is learning.
  useEffect(() => {
    if (learningId === undefined || learningPart === undefined) return;
    let active = true;
    // Read through a function: the cleanup below changes it while a request is pending.
    const ended = () => !active;
    const token = learnRequests += 1;
    const finish = (learned: LearnedInput | null) => {
      if (ended()) return;
      if (learned !== null) {
        setControls((current) => current.map((control) => control.id !== learningId ? control : learnControl(
          control, learningPart, learned,
          new Set(current.filter((other) => other.id !== learningId).flatMap((other) => inputKeys(other.input, limits))), limits
        )));
        saveAfterRender.current = true;
      }
      setLearning(null);
    };
    const onKey = (event: KeyboardEvent) => {
      const learnsKeys = driver.type === "keyboard";
      if (event.code !== "Escape" && !(learnsKeys && keyCodes.includes(event.code))) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      finish(event.code === "Escape" ? null : { kind: "button", input: { type: "key", code: event.code } });
    };
    window.addEventListener("keydown", onKey, { capture: true });
    if (driver.type !== "keyboard") {
      void (async () => {
        try {
          // A message the part cannot use is skipped, and the device is listened to again.
          while (!ended()) {
            const learned = await commands.learnInput(token, driver);
            if (ended() || learned === null) return;
            if (learnAccepts(learned.input, learningPart)) {
              finish(learned);
              return;
            }
          }
        } catch (error) {
          useAppStore.getState().setError(String(error));
          finish(null);
        }
      })();
    }
    return () => {
      active = false;
      window.removeEventListener("keydown", onKey, { capture: true });
      if (driver.type !== "keyboard") void commands.cancelInputLearn(token);
    };
  }, [learningId, learningPart, driver, keyCodes, limits]);
  const learnProps = (control: GuiInputControl) => ({
    learning: learning?.id === control.id ? learning.part : null,
    onLearn: (part: LearnPart | null) => { setLearning(part === null ? null : { id: control.id, part }); }
  });
  const [drag, setDrag] = useState<{ from: number; over: number | null } | null>(null);
  const reorder = (from: number, to: number) => {
    const next = [...controls];
    const [control] = next.splice(from, 1);
    if (control === undefined) return;
    next.splice(to, 0, control);
    setControls(next);
    saveAfterRender.current = true;
  };
  const used = new Set(controls.flatMap((control) => inputKeys(control.input, limits)));
  const readByOthers = (index: number) => new Set(controls.flatMap((control, i) => i === index ? [] : inputKeys(control.input, limits)));
  const nextName = freshControlName(controls);
  /** The input a new button reads: the next free key, note or OSC address. */
  const nextInput = driver.type === "keyboard" ? freeInput("key", used, keyCodes, limits)
    : driver.type === "midi" ? freeInput("note", used, keyCodes, limits, MIDDLE_C)
      : oscInput("button", nextName, used, limits);
  const addControl = () => {
    if (nextInput === null) return;
    const id = Math.max(0, ...controls.map((control) => control.id)) + 1;
    setControls([...controls, { id, name: nextName, description: null, kind: "button", input: nextInput }]);
    saveAfterRender.current = true;
    // The new control starts out on a free input, and takes the next one the device sends.
    setLearning({ id, part: "input" });
  };
  const removeControl = async (index: number) => {
    const name = controls[index]?.name ?? "this control";
    if (device !== undefined && !await confirm({ title: "Remove control", description: `Remove ${name} from ${device.name}? You can undo this.`, confirmLabel: "Remove", danger: true })) return;
    setControls(controls.filter((_, i) => i !== index));
    saveAfterRender.current = true;
  };
  const nameCounts = new Map<string, number>();
  for (const control of controls) nameCounts.set(control.name, (nameCounts.get(control.name) ?? 0) + 1);
  return <form className="setup-authoring-form input-device-form"
    onChange={(event) => { if (event.target instanceof HTMLSelectElement) saveAfterRender.current = true; }}
    onBlur={(event) => { if (!(event.target instanceof HTMLSelectElement)) save(); }}
    onSubmit={(event) => {
      event.preventDefault();
      if (device === undefined) void onSave(driver, controls);
    }}>
    <h4>{device === undefined ? "Add an input device" : "Input device settings"}</h4>
    <fieldset>
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
      <button type="button" className="control-add" aria-label="Add control" title="Add a control, then press its key, pad or knob" disabled={nextInput === null} onClick={addControl}><Plus size={16} /></button>
      {controls.length === 0 && <p className="object-overview-empty">
        {driver.type === "midi" ? "No controls. Add one for each pad, key, pedal or knob a console plays from, and set its kind."
          : driver.type === "osc" ? "No controls. Add one for each OSC address that presses something or sends a value, and set its kind."
            : "No controls. Add one for each key a console plays from."}
      </p>}
      <div className={`control-grid ${driver.type}`}>
        <div className="control-grid-row">
          <Cell className="gutter" />
          {GRID_COLUMNS[driver.type].map(({ label, hint }) => <Cell key={label} className="head"><span title={hint}>{label}</span></Cell>)}
          <Cell className="gutter" />
        </div>
        {controls.map((control, index) => <div
          className={`control-grid-row${drag?.from === index ? " dragging" : ""}${drag?.over === index && drag.from !== index ? (drag.from > index ? " drop-above" : " drop-below") : ""}`}
          key={control.id}
          onDragOver={(event) => {
            if (drag === null) return;
            event.preventDefault();
            if (drag.over !== index) setDrag({ ...drag, over: index });
          }}
          onDrop={(event) => {
            if (drag === null) return;
            event.preventDefault();
            reorder(drag.from, index);
            setDrag(null);
          }}>
          <Cell className="gutter"><span className="control-icon handle" draggable aria-label="Drag to reorder"
            onDragStart={(event) => {
              event.dataTransfer.effectAllowed = "move";
              event.dataTransfer.setData("text/plain", control.name);
              if (event.currentTarget.parentElement !== null) event.dataTransfer.setDragImage(event.currentTarget.parentElement, 0, 0);
              setDrag({ from: index, over: null });
            }}
            onDragEnd={() => { setDrag(null); }}><GripVertical size={14} /></span></Cell>
          <Cell className="name"><input aria-label="Name" required pattern="[a-z_][a-z0-9_]*" aria-invalid={!NAME_PATTERN.test(control.name) || (nameCounts.get(control.name) ?? 0) > 1} value={control.name} onChange={(event) => { update(index, { name: event.target.value }); }} /></Cell>
          {driver.type === "midi" && <MidiInputFields control={control} used={used} limits={limits} {...learnProps(control)} onChange={(change) => { update(index, change); }} />}
          {driver.type === "osc" && <OscInputFields control={control} taken={!isFree(control.input, readByOthers(index), limits)} {...learnProps(control)} onChange={(change) => { update(index, change); }} />}
          {driver.type === "keyboard" && control.input.type === "key" && <KeyInputCell code={control.input.code} codes={keyCodes} taken={!isFree(control.input, readByOthers(index), limits)} used={used} limits={limits} {...learnProps(control)}
            onChange={(code) => { update(index, { input: { type: "key", code } }); }} />}
          <Cell><DescriptionField label="" description={control.description} onCommit={(description) => { update(index, { description }); saveAfterRender.current = true; return Promise.resolve(); }} /></Cell>
          <Cell className="gutter"><button type="button" className="control-icon remove" aria-label="Remove control" title="Remove control" onClick={() => { void removeControl(index); }}><X size={14} /></button></Cell>
        </div>)}
      </div>
      {device === undefined && <div className="control-grid-actions">
        <button type="submit" className="control-grid-submit">Add input device</button>
      </div>}
    </fieldset>
    {confirmDialog}
  </form>;
}

/** A MIDI control's kind, what it reads (notes, a control change or pitch bend, with its numbers),
 * and the channel it reads them on. */
function MidiInputFields({ control, used, limits, learning, onLearn, onChange }: {
  control: GuiInputControl;
  used: Set<string>;
  limits: GuiInputLimits;
  learning: LearnPart | null;
  onLearn: (part: LearnPart | null) => void;
  onChange: (change: Partial<GuiInputControl>) => void;
}) {
  const { input } = control;
  if (input.type !== "notes" && input.type !== "controlChange" && input.type !== "pitchBend") return null;
  const { channel } = input;
  const own = new Set(inputKeys(input, limits));
  const taken = (candidate: GuiControlInput) => inputKeys(candidate, limits).some((key) => !own.has(key) && used.has(key));
  return <>
    <Cell><select aria-label="Kind" value={control.kind} onChange={(event) => {
      const kind = event.target.value === "axis" ? "axis" : "button";
      // An axis reads no notes, and a button no pitch bend.
      const fits = kind === "axis" ? input.type === "controlChange" || input.type === "pitchBend" : input.type === "notes" || input.type === "controlChange";
      onChange({ kind, input: fits ? input : { type: "controlChange", number: 1, channel } });
    }}>
      <option value="button">Button</option>
      <option value="axis">Axis</option>
    </select></Cell>
    <Cell className={learning === "input" ? "compound learning" : "compound"}>
      {learning === "input" ? <span className="learn-hint">{LEARN_HINTS.midi}</span> : <>
        <select aria-label="Reads" value={input.type} onChange={(event) => {
          const type = event.target.value;
          onChange({ input: type === "notes" ? { type: "notes", low: MIDDLE_C, high: MIDDLE_C, channel } : type === "pitchBend" ? { type: "pitchBend", channel } : { type: "controlChange", number: 1, channel } });
        }}>
          {control.kind === "button" && <option value="notes">Notes</option>}
          <option value="controlChange">Control change</option>
          {control.kind === "axis" && <option value="pitchBend" disabled={taken({ type: "pitchBend", channel })}>Pitch bend</option>}
        </select>
        {input.type === "notes" && <>
          <input aria-label="Lowest note" title="Type a note number, or select this box and press the key" type="number" min={0} max={limits.maxNote} required value={input.low} aria-invalid={taken(input)}
            onFocus={() => { onLearn("low"); }} onBlur={() => { onLearn(null); }}
            onChange={(event) => {
              const low = fieldNumber(event.target.value, limits.maxNote);
              if (low !== null) onChange({ input: { ...input, low } });
            }} />
          <span>to</span>
          <input aria-label="Highest note" title="Type a note number, or select this box and press the key" type="number" min={0} max={limits.maxNote} required value={input.high} aria-invalid={taken(input) || input.high < input.low}
            onFocus={() => { onLearn("high"); }} onBlur={() => { onLearn(null); }}
            onChange={(event) => {
              const high = fieldNumber(event.target.value, limits.maxNote);
              if (high !== null) onChange({ input: { ...input, high } });
            }} />
        </>}
        {input.type === "controlChange" && <input aria-label="Control change number" title="Type a number, or select this box and move the knob" type="number" min={0} max={limits.maxControlChange} required value={input.number}
          aria-invalid={taken(input)}
          onFocus={() => { onLearn("number"); }} onBlur={() => { onLearn(null); }}
          onChange={(event) => {
            const number = fieldNumber(event.target.value, limits.maxControlChange);
            if (number !== null) onChange({ input: { type: "controlChange", number, channel } });
          }} />}
      </>}
      <LearnButton learning={learning === "input"} onLearn={onLearn} />
    </Cell>
    <Cell><select aria-label="Channel" value={channel ?? ""} aria-invalid={taken(input)} onChange={(event) => {
      const value = event.target.value;
      onChange({ input: { ...input, channel: value === "" ? null : Number(value) } });
    }}>
      <option value="">Any</option>
      {channelsRead(null, limits).map((number) => <option key={number} value={number}>{number}</option>)}
    </select></Cell>
  </>;
}

/** An OSC control's kind, the address it reads, and the range an axis's value runs over. */
function OscInputFields({ control, taken, learning, onLearn, onChange }: {
  control: GuiInputControl;
  /** Whether another control of the device reads the address. */
  taken: boolean;
  learning: LearnPart | null;
  onLearn: (part: LearnPart | null) => void;
  onChange: (change: Partial<GuiInputControl>) => void;
}) {
  const { input } = control;
  if (input.type !== "address" && input.type !== "addressRange") return null;
  const { path } = input;
  return <>
    <Cell><select aria-label="Kind" value={control.kind} onChange={(event) => {
      const kind = event.target.value === "axis" ? "axis" : "button";
      onChange({ kind, input: kind === "axis" ? { type: "addressRange", path, low: 0, high: 1 } : { type: "address", path } });
    }}>
      <option value="button">Button</option>
      <option value="axis">Axis</option>
    </select></Cell>
    <Cell className={learning === "input" ? "compound learning" : "compound"}>
      {learning === "input" ? <span className="learn-hint">{LEARN_HINTS.osc}</span> : <>
        <input aria-label="Address" required value={path} placeholder="/chord/change" aria-invalid={!validAddress(path.trim()) || taken}
          onChange={(event) => { onChange({ input: { ...input, path: event.target.value } }); }} />
        {input.type === "addressRange" && <>
          <input aria-label="Value at 0" key={`low ${input.low}`} type="number" step="any" required defaultValue={input.low} aria-invalid={!(input.low < input.high)}
            onBlur={(event) => {
              const low = Number(event.currentTarget.value);
              if (event.currentTarget.value !== "" && Number.isFinite(low) && low !== input.low) onChange({ input: { ...input, low } });
            }} />
          <span>to</span>
          <input aria-label="Value at 1" key={`high ${input.high}`} type="number" step="any" required defaultValue={input.high} aria-invalid={!(input.low < input.high)}
            onBlur={(event) => {
              const high = Number(event.currentTarget.value);
              if (event.currentTarget.value !== "" && Number.isFinite(high) && high !== input.high) onChange({ input: { ...input, high } });
            }} />
        </>}
      </>}
      <LearnButton learning={learning === "input"} onLearn={onLearn} />
    </Cell>
  </>;
}

/** A keyboard control's key, chosen from a list or learned by pressing it. */
function KeyInputCell({ code, codes, taken, used, limits, learning, onLearn, onChange }: {
  code: string;
  codes: string[];
  /** Whether another control of the device reads the key. */
  taken: boolean;
  used: Set<string>;
  limits: GuiInputLimits;
  learning: LearnPart | null;
  onLearn: (part: LearnPart | null) => void;
  onChange: (code: string) => void;
}) {
  return <Cell className={learning === "input" ? "compound learning" : "compound"}>
    {learning === "input" ? <span className="learn-hint">{LEARN_HINTS.keyboard}</span>
      : <select aria-label="Key" value={code} aria-invalid={taken} onChange={(event) => { onChange(event.target.value); }}>
        {codes.map((candidate) => <option key={candidate} value={candidate} disabled={candidate !== code && !isFree({ type: "key", code: candidate }, used, limits)}>{candidate}</option>)}
      </select>}
    <LearnButton learning={learning === "input"} onLearn={onLearn} />
  </Cell>;
}

/** Starts learning a control's input from the device, or stops it. */
function LearnButton({ learning, onLearn }: { learning: boolean; onLearn: (part: LearnPart | null) => void }) {
  return <button type="button" className="control-icon learn" aria-pressed={learning} aria-label="Learn input" title={learning ? "Stop listening (Esc)" : "Set this input by using the control"}
    onClick={() => { onLearn(learning ? null : "input"); }}><CircleDot size={14} /></button>;
}

/** One bordered cell of the control grid. Borders belong to cells alone, never to the controls inside them. */
function Cell({ children, className }: { children?: ReactNode; className?: string }) {
  return <div className={className === undefined ? "control-cell" : `control-cell ${className}`}>{children}</div>;
}

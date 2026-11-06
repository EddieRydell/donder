import { CircleDot, GripVertical, X } from "lucide-react";
import { type FocusEvent, type ReactNode, useEffect, useRef, useState } from "react";
import { commands } from "../../../api";
import { useAppStore } from "../../../store";
import type { GuiControlInput, GuiInputControl, GuiInputControlKind, GuiInputDriver, GuiInputLimits, LearnedInput, SetupInputDevice } from "../../../types";
import { AddRow, Button, Field, IconButton, NumberField, Select, TextField, Tip } from "../../kit";
import { DescriptionText } from "../DescriptionField";

const MIDDLE_C = 60;
/** The UDP port a new OSC device listens on, and the highest there is. */
const DEFAULT_OSC_PORT = 9000;
const MAX_UDP_PORT = 65535;

type DriverType = GuiInputDriver["type"];

/** What is being learned from the controller: a whole input, one note of a range, or a control change number. */
type LearnPart = "input" | "low" | "high" | "number";

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
    { label: "Input", hint: "What the control listens to: notes (a pad, or a range of keys; 60 is middle C), a control change (a knob, slider or pedal, numbered 0 to 119), or the pitch bend wheel. While a number is focused, playing the key or moving the knob sets it." },
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
  // Each committed change saves once its state has rendered.
  const saveAfterRender = useRef(false);
  useEffect(() => {
    if (!saveAfterRender.current) return;
    saveAfterRender.current = false;
    if (device === undefined || JSON.stringify([driver, controls]) === saved) return;
    void onSave(driver, controls);
  });
  const commitControls = (next: GuiInputControl[]) => {
    setControls(next);
    saveAfterRender.current = true;
  };
  const commitDriver = (next: GuiInputDriver) => {
    setDriver(next);
    saveAfterRender.current = true;
  };
  const update = (index: number, change: Partial<GuiInputControl>) => {
    commitControls(controls.map((control, i) => i === index ? { ...control, ...change } : control));
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
    commitControls(next);
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
    commitControls([...controls, { id, name: nextName, description: null, kind: "button", input: nextInput }]);
    // The new control starts out on a free input, and takes the next one the device sends.
    setLearning({ id, part: "input" });
  };
  const driverOptions = DRIVERS.map(({ type, label }) => {
    // Switching would drop the controls the driver has no kind for.
    const dropped = droppedKinds(controls, type);
    return { value: type, label: dropped.length === 0 ? label : `${label} (no ${dropped.join(", ")})`, disabled: dropped.length > 0 };
  });
  return <form className="setup-authoring-form input-device-form"
    onSubmit={(event) => {
      event.preventDefault();
      if (device === undefined) void onSave(driver, controls);
    }}>
    <fieldset>
      <Field label="Driver">
        <Select name="Driver" value={driver.type} options={driverOptions} onChange={(type) => {
          const next: GuiInputDriver = type === "midi" ? { type: "midi", port: null }
            : type === "osc" ? { type: "osc", port: DEFAULT_OSC_PORT }
              : { type: "keyboard" };
          commitDriver(next);
          setControls(convertControls(controls, next, keyCodes, limits));
        }} />
      </Field>
      {driver.type === "midi" && <Field label="Port" tip="The MIDI port to read; empty reads every port">
        <TextField name="MIDI port" placeholder="Any" value={driver.port ?? ""} onCommit={(port) => { commitDriver({ type: "midi", port: port === "" ? null : port }); }} />
      </Field>}
      {driver.type === "osc" && <Field label="UDP port">
        <NumberField name="UDP port" value={driver.port} step={1} min={1} max={MAX_UDP_PORT} defaultValue={DEFAULT_OSC_PORT} integer onCommit={(port) => { commitDriver({ type: "osc", port }); }} />
      </Field>}
      {controls.length > 0 && <div className={`control-grid ${driver.type}`}>
        <div className="control-grid-row">
          <Cell className="gutter" />
          {GRID_COLUMNS[driver.type].map(({ label, hint }) => <Cell key={label} className="head"><Tip label={hint}><span>{label}</span></Tip></Cell>)}
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
          <Cell className="name"><TextField name="Name" value={control.name} onCommit={(name) => { update(index, { name }); }} /></Cell>
          {driver.type === "midi" && <MidiInputFields control={control} used={used} limits={limits} {...learnProps(control)} onChange={(change) => { update(index, change); }} />}
          {driver.type === "osc" && <OscInputFields control={control} taken={!isFree(control.input, readByOthers(index), limits)} {...learnProps(control)} onChange={(change) => { update(index, change); }} />}
          {driver.type === "keyboard" && control.input.type === "key" && <KeyInputCell code={control.input.code} codes={keyCodes} taken={!isFree(control.input, readByOthers(index), limits)} used={used} limits={limits} {...learnProps(control)}
            onChange={(code) => { update(index, { input: { type: "key", code } }); }} />}
          <Cell><DescriptionText description={control.description} onCommit={(description) => { update(index, { description }); return Promise.resolve(); }} /></Cell>
          <Cell className="gutter"><IconButton icon={X} label="Remove control" danger className="control-icon remove" onClick={() => { commitControls(controls.filter((_, i) => i !== index)); }} /></Cell>
        </div>)}
      </div>}
      <AddRow label="Add control" disabled={nextInput === null} onAdd={addControl} />
      {device === undefined && <div className="control-grid-actions">
        <Button type="submit" variant="primary" className="control-grid-submit">Add input device</Button>
      </div>}
    </fieldset>
  </form>;
}

/** Learns part of a control's input while its field has focus: the note or number the device sends next. */
function LearnOnFocus({ part, onLearn, children }: { part: LearnPart; onLearn: (part: LearnPart | null) => void; children: ReactNode }) {
  return <span className="learn-on-focus" onFocus={() => { onLearn(part); }} onBlur={(event: FocusEvent<HTMLSpanElement>) => {
    if (!(event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget))) onLearn(null);
  }}>{children}</span>;
}

const KINDS = [{ value: "button", label: "Button" }, { value: "axis", label: "Axis" }] as const;

/** The select's value for a MIDI input on every channel. */
const ANY_CHANNEL = "any";

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
    <Cell><Select name="Kind" value={control.kind} options={KINDS} onChange={(kind) => {
      // An axis reads no notes, and a button no pitch bend.
      const fits = kind === "axis" ? input.type === "controlChange" || input.type === "pitchBend" : input.type === "notes" || input.type === "controlChange";
      onChange({ kind, input: fits ? input : { type: "controlChange", number: 1, channel } });
    }} /></Cell>
    <Cell className={learning === "input" ? "compound learning" : "compound"}>
      <Select name="Reads" value={input.type} onChange={(type) => {
        onChange({ input: type === "notes" ? { type: "notes", low: MIDDLE_C, high: MIDDLE_C, channel } : type === "pitchBend" ? { type: "pitchBend", channel } : { type: "controlChange", number: 1, channel } });
      }} options={[
        ...(control.kind === "button" ? [{ value: "notes", label: "Notes" } as const] : []),
        { value: "controlChange", label: "Control change" } as const,
        ...(control.kind === "axis" ? [{ value: "pitchBend", label: "Pitch bend", disabled: taken({ type: "pitchBend", channel }) } as const] : [])
      ]} />
      {input.type === "notes" && <>
        <LearnOnFocus part="low" onLearn={onLearn}>
          <NumberField name="Lowest note" value={input.low} step={1} min={0} max={limits.maxNote} integer invalid={taken(input)} onCommit={(low) => { onChange({ input: { ...input, low } }); }} />
        </LearnOnFocus>
        <span>to</span>
        <LearnOnFocus part="high" onLearn={onLearn}>
          <NumberField name="Highest note" value={input.high} step={1} min={0} max={limits.maxNote} integer invalid={taken(input)} onCommit={(high) => { onChange({ input: { ...input, high } }); }} />
        </LearnOnFocus>
      </>}
      {input.type === "controlChange" && <LearnOnFocus part="number" onLearn={onLearn}>
        <NumberField name="Control change number" value={input.number} step={1} min={0} max={limits.maxControlChange} integer invalid={taken(input)} onCommit={(number) => { onChange({ input: { type: "controlChange", number, channel } }); }} />
      </LearnOnFocus>}
      <LearnButton learning={learning === "input"} onLearn={onLearn} />
    </Cell>
    <Cell><Select name="Channel" value={channel === null ? ANY_CHANNEL : String(channel)} invalid={taken(input)} onChange={(value) => {
      onChange({ input: { ...input, channel: value === ANY_CHANNEL ? null : Number(value) } });
    }} options={[
      { value: ANY_CHANNEL, label: "Any" },
      ...channelsRead(null, limits).map((number) => ({ value: String(number), label: String(number) }))
    ]} /></Cell>
  </>;
}

const AXIS_VALUE_STEP = 0.1;

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
    <Cell><Select name="Kind" value={control.kind} options={KINDS} onChange={(kind) => {
      onChange({ kind, input: kind === "axis" ? { type: "addressRange", path, low: 0, high: 1 } : { type: "address", path } });
    }} /></Cell>
    <Cell className={learning === "input" ? "compound learning" : "compound"}>
      <TextField name="Address" value={path} placeholder="/chord/change" invalid={taken}
        onCommit={(next) => { onChange({ input: { ...input, path: next } }); }} />
      {input.type === "addressRange" && <>
        <NumberField name="Value at 0" value={input.low} step={AXIS_VALUE_STEP} defaultValue={0} onCommit={(low) => { onChange({ input: { ...input, low } }); }} />
        <span>to</span>
        <NumberField name="Value at 1" value={input.high} step={AXIS_VALUE_STEP} defaultValue={1} onCommit={(high) => { onChange({ input: { ...input, high } }); }} />
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
    <Select name="Key" value={code} invalid={taken} onChange={onChange}
      options={codes.map((candidate) => ({ value: candidate, label: candidate, disabled: candidate !== code && !isFree({ type: "key", code: candidate }, used, limits) }))} />
    <LearnButton learning={learning === "input"} onLearn={onLearn} />
  </Cell>;
}

/** Starts learning a control's input from the device, or stops it; pressed while it listens. */
function LearnButton({ learning, onLearn }: { learning: boolean; onLearn: (part: LearnPart | null) => void }) {
  return <IconButton icon={CircleDot} className="control-icon learn" pressed={learning} label={learning ? "Listening: use the control, or press again to stop" : "Learn input from the control"}
    onClick={() => { onLearn(learning ? null : "input"); }} />;
}

/** One bordered cell of the control grid. Borders belong to cells alone, never to the controls inside them. */
function Cell({ children, className }: { children?: ReactNode; className?: string }) {
  return <div className={className === undefined ? "control-cell" : `control-cell ${className}`}>{children}</div>;
}

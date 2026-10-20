import { useState } from "react";
import type { GuiChordQuality, GuiControlInput, GuiHeard, GuiInputControl, GuiInputControlKind, GuiInputDriver, GuiInputLimits, GuiKeyMode, GuiMeasure, SetupInputDevice } from "../../../types";
import { DescriptionField } from "../DescriptionField";

const MIDDLE_C = 60;
/** A full piano's lowest and highest keys, A0 and C8. */
const PIANO_LOW = 21;
const PIANO_HIGH = 108;
const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/** A MIDI note's name, where note 60 is middle C, C4. */
function noteName(note: number): string {
  return `${NOTE_NAMES[note % 12] ?? ""}${Math.floor(note / 12) - 1}`;
}

const NOTE_LETTERS: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };

/** A note from its name, such as `C4`, `F#3` or `Bb5`, or `null`. */
function parseNoteName(name: string, maxNote: number): number | null {
  const match = /^([A-Ga-g])([#b]?)(-?\d)$/.exec(name);
  if (match === null) return null;
  const [, letter = "", accidental, octave = ""] = match;
  const pitch = NOTE_LETTERS[letter.toUpperCase()];
  if (pitch === undefined) return null;
  const note = (Number(octave) + 1) * 12 + pitch + (accidental === "#" ? 1 : accidental === "b" ? -1 : 0);
  return note >= 0 && note <= maxNote ? note : null;
}

/** The notes of a melody written as note names apart, or `null`. */
function parseMelody(text: string, maxNote: number): number[] | null {
  const notes = text.trim().split(/\s+/).map((name) => parseNoteName(name, maxNote));
  return notes.every((note) => note !== null) ? notes : null;
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
    case "heard": return [`heard ${heardKey(input.heard)}`];
    case "measured": return [`measure ${input.measure}`];
  }
}

function heardKey(heard: GuiHeard): string {
  switch (heard.type) {
    case "chord": return `chord ${heard.quality}`;
    case "mode": return `mode ${heard.mode}`;
    case "idle": return `idle ${heard.seconds}`;
    case "tune": return `tune ${heard.notes.join(",")}`;
    case "chordChange":
    case "keyChange":
    case "resolution":
    case "beat":
    case "runUp":
    case "runDown":
    case "bigChord": return heard.type;
  }
}

/** The moments a listener hears, each with a default to start from. */
const MOMENTS: { label: string; heard: GuiHeard }[] = [
  { label: "Chord change", heard: { type: "chordChange" } },
  { label: "Key change", heard: { type: "keyChange" } },
  { label: "Resolution (V to I)", heard: { type: "resolution" } },
  { label: "Beat", heard: { type: "beat" } },
  { label: "Run up", heard: { type: "runUp" } },
  { label: "Run down", heard: { type: "runDown" } },
  { label: "Big chord", heard: { type: "bigChord" } },
  { label: "While the chord is", heard: { type: "chord", quality: "minor" } },
  { label: "While the key is", heard: { type: "mode", mode: "minor" } },
  { label: "Idle for", heard: { type: "idle", seconds: 20 } },
  // The chorus of Jingle Bells, to start from.
  { label: "Tune", heard: { type: "tune", notes: [64, 64, 64, 64, 64, 64, 64, 67, 60, 62, 64] } }
];
const CHORD_QUALITIES: GuiChordQuality[] = ["major", "minor", "dominant", "diminished", "augmented", "suspended"];
const KEY_MODES: GuiKeyMode[] = ["major", "minor"];
const MEASURES: { label: string; measure: GuiMeasure }[] = [
  { label: "Key colour", measure: "keyColor" },
  { label: "Chord colour", measure: "chordColor" },
  { label: "Tension", measure: "tension" },
  { label: "Loudness", measure: "loudness" },
  { label: "Density", measure: "density" },
  { label: "Register", measure: "register" },
  { label: "Tempo", measure: "tempo" }
];

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

/** Whether `driver` reads `input`. */
function driverReads(driver: GuiInputDriver, input: GuiControlInput): boolean {
  switch (driver.type) {
    case "keyboard": return input.type === "key";
    case "midi": return input.type !== "key" && input.type !== "heard" && input.type !== "measured";
    case "listen": return input.type === "heard" || input.type === "measured";
  }
}

/** The controls carried over to another driver: each keyboard or MIDI button
 * reads a free input of the new driver; a keyboard has no dials or keys, and
 * a listener keeps only its own moments and measures. */
function convertControls(controls: GuiInputControl[], driver: GuiInputDriver, keyCodes: string[], limits: GuiInputLimits): GuiInputControl[] {
  const used = new Set<string>();
  const converted: GuiInputControl[] = [];
  for (const control of controls) {
    const input = driverReads(driver, control.input) ? control.input
      : control.kind !== "button" || driver.type === "listen" ? null
        : freeInput(driver.type === "keyboard" ? "key" : "note", used, keyCodes, limits, MIDDLE_C);
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
  const add = (kind: GuiInputControlKind, input: GuiControlInput | null) => {
    if (input === null) return;
    const id = Math.max(0, ...controls.map((control) => control.id)) + 1;
    setControls([...controls, { id, name: freshControlName(controls), description: null, kind, input }]);
  };
  const midi = driver.type === "midi";
  const listen = driver.type === "listen";
  const nextMoment = MOMENTS.find((moment) => !used.has(inputKey({ type: "heard", heard: moment.heard })))?.heard;
  const nextMeasure = MEASURES.find((measure) => !used.has(inputKey({ type: "measured", measure: measure.measure })))?.measure;
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
        const next: GuiInputDriver = event.target.value === "midi" ? { type: "midi", port: null }
          : event.target.value === "listen" ? { type: "listen", port: null } : { type: "keyboard" };
        setDriver(next);
        setControls(convertControls(controls, next, keyCodes, limits));
      }}>
        <option value="keyboard">Keyboard</option>
        <option value="midi">MIDI</option>
        <option value="listen">Listener</option>
      </select></label>
      {driver.type !== "keyboard" && <label>Port<input value={driver.port ?? ""} placeholder="Every MIDI port" onChange={(event) => {
        setDriver({ type: driver.type, port: event.target.value === "" ? null : event.target.value });
      }} /></label>}
      {controls.length === 0 && <p className="object-overview-empty">
        {listen ? "No controls. Add a moment for each thing in the music a rig answers, and a measure for each it follows."
          : midi ? "No controls. Add keys for a keyboard's notes, a button for each pad or pedal a rig plays from, and a dial for each knob."
            : "No controls. Add a button for each key a rig plays from."}
      </p>}
      {controls.map((control, index) => <div className="controller-port-row" key={control.id}>
        <label>Name<input required pattern="[a-z_][a-z0-9_]*" value={control.name} onChange={(event) => { update(index, { name: event.target.value }); }} /></label>
        {listen
          ? <ListenerInputFields control={control} used={used} limits={limits} onChange={(change) => { update(index, change); }} />
          : midi
          ? <MidiInputFields control={control} used={used} limits={limits} onChange={(change) => { update(index, change); }} />
          : control.input.type === "key" && <label>Key<select value={control.input.code} onChange={(event) => { update(index, { input: { type: "key", code: event.target.value } }); }}>
            {keyCodes.map((code) => <option key={code} value={code} disabled={control.input.type === "key" && code !== control.input.code && used.has(inputKey({ type: "key", code }))}>{code}</option>)}
          </select></label>}
        <DescriptionField description={control.description} onCommit={(description) => { update(index, { description }); return Promise.resolve(); }} />
        <button type="button" disabled={index === 0} onClick={() => { move(index, -1); }}>Move up</button>
        <button type="button" disabled={index === controls.length - 1} onClick={() => { move(index, 1); }}>Move down</button>
        <button type="button" onClick={() => { setControls(controls.filter((_, i) => i !== index)); }}>Remove control</button>
      </div>)}
      {listen && <>
        <button type="button" disabled={nextMoment === undefined} onClick={() => { if (nextMoment !== undefined) add("button", { type: "heard", heard: nextMoment }); }}>Add moment</button>
        <button type="button" disabled={nextMeasure === undefined} onClick={() => { if (nextMeasure !== undefined) add("dial", { type: "measured", measure: nextMeasure }); }}>Add measure</button>
      </>}
      {listen ? null : midi
        ? <>
          <button type="button" disabled={nextNote === null} onClick={() => { add("button", nextNote); }}>Add button</button>
          <button type="button" disabled={nextControlChange === null} onClick={() => { add("dial", nextControlChange); }}>Add dial</button>
          <button type="button" disabled={!pianoFree} onClick={() => { add("keys", piano); }}>Add keys</button>
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
        onChange={(event) => { onChange({ input: { ...input, low: Number(event.target.value) } }); }} /><span>{noteName(input.low)}</span></label>
      <label>Highest<input type="number" min={0} max={limits.maxNote} required value={input.high} aria-invalid={taken(input) || input.high < input.low}
        onChange={(event) => { onChange({ input: { ...input, high: Number(event.target.value) } }); }} /><span>{noteName(input.high)}</span></label>
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
      onChange={(event) => { onChange({ input: { type: "note", note: Number(event.target.value) } }); }} /><span>{noteName(input.note)}</span></label>}
    {input.type === "controlChange" && <label>Number<input type="number" min={0} max={limits.maxControlChange} required value={input.number}
      aria-invalid={taken(input)}
      onChange={(event) => { onChange({ input: { type: "controlChange", number: Number(event.target.value) } }); }} /></label>}
  </>;
}

/** A listener's control: a moment it hears, as a button, or a measure it
 * follows, as a dial. */
function ListenerInputFields({ control, used, limits, onChange }: {
  control: GuiInputControl;
  used: Set<string>;
  limits: GuiInputLimits;
  onChange: (change: Partial<GuiInputControl>) => void;
}) {
  const { input } = control;
  const own = inputKey(input);
  const taken = (candidate: GuiControlInput) => inputKey(candidate) !== own && used.has(inputKey(candidate));
  if (input.type === "measured") {
    return <label>Measure<select value={input.measure} onChange={(event) => {
      const measure = MEASURES.find((choice) => choice.measure === event.target.value)?.measure;
      if (measure !== undefined) onChange({ kind: "dial", input: { type: "measured", measure } });
    }}>
      {MEASURES.map((choice) => <option key={choice.measure} value={choice.measure} disabled={taken({ type: "measured", measure: choice.measure })}>{choice.label}</option>)}
    </select></label>;
  }
  if (input.type !== "heard") return null;
  const { heard } = input;
  const set = (next: GuiHeard) => { onChange({ kind: "button", input: { type: "heard", heard: next } }); };
  return <>
    <label>Moment<select value={heard.type} onChange={(event) => {
      const choice = MOMENTS.find((moment) => moment.heard.type === event.target.value);
      if (choice !== undefined) set(choice.heard);
    }}>
      {MOMENTS.map((moment) => <option key={moment.heard.type} value={moment.heard.type}>{moment.label}</option>)}
    </select></label>
    {heard.type === "chord" && <label>Quality<select value={heard.quality} aria-invalid={taken(input)} onChange={(event) => {
      const quality = CHORD_QUALITIES.find((choice) => choice === event.target.value);
      if (quality !== undefined) set({ type: "chord", quality });
    }}>
      {CHORD_QUALITIES.map((quality) => <option key={quality} value={quality}>{quality}</option>)}
    </select></label>}
    {heard.type === "mode" && <label>Mode<select value={heard.mode} aria-invalid={taken(input)} onChange={(event) => {
      const mode = KEY_MODES.find((choice) => choice === event.target.value);
      if (mode !== undefined) set({ type: "mode", mode });
    }}>
      {KEY_MODES.map((mode) => <option key={mode} value={mode}>{mode}</option>)}
    </select></label>}
    {heard.type === "tune" && <TuneField notes={heard.notes} invalid={taken(input)} limits={limits} onChange={(notes) => { set({ type: "tune", notes }); }} />}
    {heard.type === "idle" && <label>Seconds<input type="number" min={1} max={3600} required value={heard.seconds} aria-invalid={taken(input)}
      onChange={(event) => { set({ type: "idle", seconds: Number(event.target.value) }); }} /></label>}
  </>;
}

/** A tune's melody as note names, applied when it reads as one. */
function TuneField({ notes, invalid, limits, onChange }: { notes: number[]; invalid: boolean; limits: GuiInputLimits; onChange: (notes: number[]) => void }) {
  const written = notes.map(noteName).join(" ");
  const [text, setText] = useState(written);
  const parsed = parseMelody(text, limits.maxNote);
  const fits = parsed !== null && parsed.length >= limits.minTuneNotes && parsed.length <= limits.maxTuneNotes;
  return <label>Melody<input value={text} aria-invalid={invalid || !fits} title={`${limits.minTuneNotes} to ${limits.maxTuneNotes} notes, such as E4 E4 E4 G4 C4; any key plays it`}
    onChange={(event) => { setText(event.target.value); }}
    onBlur={() => { if (fits && parsed.join(" ") !== notes.join(" ")) onChange(parsed); }} /></label>;
}

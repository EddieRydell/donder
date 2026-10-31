import type { ConsoleAction, ConsoleGuiDocument, ConsolePlayLength, ConsolePlayedQuantity, ConsolePlayedValue } from "../../../types";
import { controllerOf, controlKey, controlRef } from "./consoleEditing";

/** Every quantity but velocity needs a button that reads notes. */
const PLAYED_QUANTITIES: { quantity: ConsolePlayedQuantity; label: string; needsNote: boolean }[] = [
  { quantity: "velocity", label: "Velocity", needsNote: false },
  { quantity: "pitch", label: "Pitch (place in the note range)", needsNote: true },
  { quantity: "pitchClass", label: "Note name (C to B)", needsNote: true },
  { quantity: "fifths", label: "Circle of fifths", needsNote: true }
];

/** A seconds field that commits a changed value when it loses focus. */
function SecondsField({ label, seconds, onChange }: { label: string; seconds: number; onChange: (seconds: number) => void }) {
  return <label>{label}<input key={seconds} type="number" min={0} step="any" defaultValue={seconds} onBlur={(event) => {
    const next = Number(event.currentTarget.value);
    if (next !== seconds) onChange(next);
  }} /></label>;
}

/** How long a played copy lasts: a fixed length, or one scaled by the
 * press's velocity or an axis between the shortest and longest. */
function PlayLengthFields({ document, length, onChange }: { document: ConsoleGuiDocument; length: ConsolePlayLength; onChange: (length: ConsolePlayLength) => void }) {
  const by = length.type === "scaled" ? (length.by.type === "velocity" ? "velocity" : controlKey(length.by.control)) : "";
  const axes = document.inputDevices.flatMap((device) => device.axes.map((axis) => ({ name: `${device.name}.${axis.name}`, ref: controlRef(device, axis) })));
  return <>
    <div className="inspector-inline-row">
      <label>Length<select value={length.type} onChange={(event) => {
        const seconds = length.type === "fixed" ? length.durationSeconds : length.longestSeconds;
        onChange(event.currentTarget.value === "scaled"
          ? { type: "scaled", by: { type: "velocity" }, shortestSeconds: seconds, longestSeconds: seconds }
          : { type: "fixed", durationSeconds: seconds });
      }}>
        <option value="fixed">Fixed</option>
        <option value="scaled">Scaled</option>
      </select></label>
      {length.type === "fixed" && <SecondsField label="Seconds" seconds={length.durationSeconds} onChange={(durationSeconds) => { onChange({ type: "fixed", durationSeconds }); }} />}
    </div>
    {length.type === "scaled" && <div className="inspector-inline-row">
      <label>By<select value={by} onChange={(event) => {
        const value = event.currentTarget.value;
        const axis = axes.find((candidate) => controlKey(candidate.ref) === value);
        onChange({ ...length, by: axis === undefined ? { type: "velocity" } : { type: "axis", control: axis.ref } });
      }}>
        <option value="velocity">Velocity</option>
        {axes.map((axis) => <option key={controlKey(axis.ref)} value={controlKey(axis.ref)}>{axis.name}</option>)}
      </select></label>
      <SecondsField label="Shortest" seconds={length.shortestSeconds} onChange={(shortestSeconds) => { onChange({ ...length, shortestSeconds }); }} />
      <SecondsField label="Longest" seconds={length.longestSeconds} onChange={(longestSeconds) => { onChange({ ...length, longestSeconds }); }} />
    </div>}
  </>;
}

/** Parameters a press sets on the copy it starts, from how it was played.
 * A parameter a control sets is not played. */
function PlayedValues({ document, clipId, values, notes, onChange }: {
  document: ConsoleGuiDocument;
  clipId: number;
  values: ConsolePlayedValue[];
  notes: boolean;
  onChange: (values: ConsolePlayedValue[]) => void;
}) {
  const params = (document.clips.find((clip) => clip.id === clipId)?.params ?? [])
    .filter((param) => param.supportsAutomation && controllerOf(document, { type: "clipParam", clipId, param: param.name }) === undefined)
    .map((param) => param.name);
  const free = params.find((param) => !values.some((value) => value.param === param));
  return <div className="clip-param-section">
    <h3>Played values</h3>
    {values.length === 0 && <p className="object-overview-empty">Each press starts the effect as set. Add a played value to set a parameter from how hard or which note was played.</p>}
    {values.map((value, index) => <div key={value.param} className="inspector-inline-row">
      <label>Parameter<select value={value.param} onChange={(event) => {
        onChange(values.map((other, i) => i === index ? { ...other, param: event.currentTarget.value } : other));
      }}>
        {!params.includes(value.param) && <option value={value.param} disabled>{value.param}</option>}
        {params.map((param) => <option key={param} value={param} disabled={param !== value.param && values.some((other) => other.param === param)}>{param}</option>)}
      </select></label>
      <label>From<select value={value.from} onChange={(event) => {
        const from = PLAYED_QUANTITIES.find((choice) => choice.quantity === event.currentTarget.value)?.quantity;
        if (from !== undefined) onChange(values.map((other, i) => i === index ? { ...other, from } : other));
      }}>
        {PLAYED_QUANTITIES.map((choice) => <option key={choice.quantity} value={choice.quantity} disabled={choice.needsNote && !notes}>{choice.label}</option>)}
      </select></label>
      <button type="button" onClick={() => { onChange(values.filter((_, i) => i !== index)); }}>Remove</button>
    </div>)}
    <button type="button" disabled={free === undefined} onClick={() => {
      if (free !== undefined) onChange([...values, { param: free, from: "velocity" }]);
    }}>Add played value</button>
  </div>;
}

/** What one action does to the effect it plays: its copies and length, and
 * the values played into its parameters. */
export function PlayFields({ document, action, press, notes, onChange }: {
  document: ConsoleGuiDocument;
  action: ConsoleAction;
  /** Whether a press runs the action; values are played only on a press. */
  press: boolean;
  /** Whether the control reads notes. */
  notes: boolean;
  onChange: (action: ConsoleAction) => void;
}) {
  if (action.type !== "play" && action.type !== "start" && action.type !== "toggle") return null;
  return <>
    {action.type === "play" && <>
      <label>Copies<input key={action.copies} type="number" min={1} max={document.maxCopies} step={1} defaultValue={action.copies} onBlur={(event) => {
        const copies = Number(event.currentTarget.value);
        if (copies !== action.copies) onChange({ ...action, copies });
      }} /></label>
      <PlayLengthFields document={document} length={action.length} onChange={(length) => { onChange({ ...action, length }); }} />
    </>}
    {press && <PlayedValues document={document} clipId={action.clipId} values={action.values} notes={notes} onChange={(values) => { onChange({ ...action, values }); }} />}
  </>;
}

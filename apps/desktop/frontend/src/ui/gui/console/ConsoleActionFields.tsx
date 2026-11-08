import type { ConsoleAction, ConsoleGuiDocument, ConsolePlayLength } from "../../../types";
import { Field, NumberField, Select } from "../../kit";
import { controlKey, controlRef } from "./consoleEditing";

const LENGTHS = [{ value: "fixed", label: "Fixed" }, { value: "scaled", label: "Scaled" }] as const;

/** The select's value for scaling by velocity rather than an axis. */
const BY_VELOCITY = "velocity";

const SECONDS_STEP = 0.1;

function SecondsField({ label, seconds, onChange }: { label: string; seconds: number; onChange: (seconds: number) => void }) {
  return <Field label={label}><NumberField name={label} value={seconds} step={SECONDS_STEP} min={0} unit="s" onCommit={onChange} /></Field>;
}

/** How long a played copy lasts: a fixed length, or one scaled by the
 * press's velocity or an axis between the shortest and longest. */
function PlayLengthFields({ document, length, onChange }: { document: ConsoleGuiDocument; length: ConsolePlayLength; onChange: (length: ConsolePlayLength) => void }) {
  const by = length.type === "scaled" ? (length.by.type === "velocity" ? BY_VELOCITY : controlKey(length.by.control)) : BY_VELOCITY;
  const axes = document.inputDevices.flatMap((device) => device.axes.map((axis) => ({ name: `${device.name}.${axis.name}`, ref: controlRef(device, axis) })));
  return <>
    <Field label="Length">
      <Select name="Length" value={length.type} options={LENGTHS} onChange={(type) => {
        const seconds = length.type === "fixed" ? length.durationSeconds : length.longestSeconds;
        onChange(type === "scaled"
          ? { type: "scaled", by: { type: "velocity" }, shortestSeconds: seconds, longestSeconds: seconds }
          : { type: "fixed", durationSeconds: seconds });
      }} />
    </Field>
    {length.type === "fixed" && <SecondsField label="Seconds" seconds={length.durationSeconds} onChange={(durationSeconds) => { onChange({ type: "fixed", durationSeconds }); }} />}
    {length.type === "scaled" && <>
      <Field label="By">
        <Select name="Scaled by" value={by} onChange={(value) => {
          const axis = axes.find((candidate) => controlKey(candidate.ref) === value);
          onChange({ ...length, by: axis === undefined ? { type: "velocity" } : { type: "axis", control: axis.ref } });
        }} options={[{ value: BY_VELOCITY, label: "Velocity" }, ...axes.map((axis) => ({ value: controlKey(axis.ref), label: axis.name }))]} />
      </Field>
      <SecondsField label="Shortest" seconds={length.shortestSeconds} onChange={(shortestSeconds) => { onChange({ ...length, shortestSeconds }); }} />
      <SecondsField label="Longest" seconds={length.longestSeconds} onChange={(longestSeconds) => { onChange({ ...length, longestSeconds }); }} />
    </>}
  </>;
}

/** What a `Play` does to the effect it plays: its copies and length. */
export function PlayFields({ document, action, onChange }: {
  document: ConsoleGuiDocument;
  action: ConsoleAction;
  onChange: (action: ConsoleAction) => void;
}) {
  if (action.type !== "play") return null;
  return <>
    <Field label="Copies"><NumberField name="Copies" value={action.copies} step={1} min={1} max={document.maxCopies} defaultValue={1} integer onCommit={(copies) => { onChange({ ...action, copies }); }} /></Field>
    <PlayLengthFields document={document} length={action.length} onChange={(length) => { onChange({ ...action, length }); }} />
  </>;
}

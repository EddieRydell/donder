import { Plus, X } from "lucide-react";
import type { ConsoleAction, ConsoleGuiDocument, ConsolePlayLength, ConsolePlayedQuantity, ConsolePlayedValue } from "../../../types";
import { AddRow, Field, IconButton, NumberField, Section, Select } from "../../kit";
import { WindowFields } from "../WindowFields";
import { controllerOf, controlKey, controlRef, FULL_WINDOW } from "./consoleEditing";

/** Every quantity but velocity needs a button that reads notes. */
const PLAYED_QUANTITIES: { value: ConsolePlayedQuantity; label: string; needsNote: boolean }[] = [
  { value: "velocity", label: "Velocity", needsNote: false },
  { value: "pitch", label: "Pitch", needsNote: true },
  { value: "pitchClass", label: "Note name", needsNote: true },
  { value: "fifths", label: "Fifths", needsNote: true }
];

const FROM_TIP = "Velocity: how hard it was played. Pitch: its place in the note range. Note name: C to B. Fifths: its place on the circle of fifths.";

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

/** Parameters a press sets on the copy it starts, from how it was played.
 * A parameter a control sets is not played. */
function PlayedValues({ document, clipId, values, notes, onChange }: {
  document: ConsoleGuiDocument;
  clipId: number;
  values: ConsolePlayedValue[];
  notes: boolean;
  onChange: (values: ConsolePlayedValue[]) => void;
}) {
  const clip = document.clips.find((candidate) => candidate.id === clipId);
  const params = (clip?.params ?? [])
    .filter((param) => param.supportsAutomation && controllerOf(document, { type: "clipParam", clipId, param: param.name }) === undefined)
    .map((param) => param.name);
  const free = params.find((param) => !values.some((value) => value.param === param));
  const add = () => { if (free !== undefined) onChange([...values, { param: free, from: "velocity", window: FULL_WINDOW }]); };
  const label = "Add played value";
  return <Section title="Played values" actions={values.length > 0 ? <IconButton icon={Plus} label={label} disabled={free === undefined} onClick={add} /> : undefined}>
    {values.length === 0 && <AddRow label={label} disabled={free === undefined} onAdd={add} />}
    {values.map((value, index) => {
      const change = (next: Partial<ConsolePlayedValue>) => { onChange(values.map((other, i) => i === index ? { ...other, ...next } : other)); };
      return <div key={value.param} className="console-played-value">
        <Field label="Parameter">
          <Select name="Parameter" value={value.param} onChange={(param) => { change({ param }); }} options={[
            ...(params.includes(value.param) ? [] : [{ value: value.param, label: value.param, disabled: true }]),
            ...params.map((param) => ({ value: param, label: param, disabled: param !== value.param && values.some((other) => other.param === param) }))
          ]} />
          <IconButton icon={X} label="Remove played value" danger onClick={() => { onChange(values.filter((_, i) => i !== index)); }} />
        </Field>
        <Field label="From" tip={FROM_TIP}>
          <Select name="From" value={value.from} onChange={(from) => { change({ from }); }}
            options={PLAYED_QUANTITIES.map((choice) => ({ value: choice.value, label: choice.label, disabled: choice.needsNote && !notes }))} />
        </Field>
        <Field label="Range">
          <WindowFields value={value.window} range={clip?.params.find((param) => param.name === value.param)?.range ?? null} onChange={(window) => { change({ window }); }} />
        </Field>
      </div>;
    })}
  </Section>;
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
      <Field label="Copies"><NumberField name="Copies" value={action.copies} step={1} min={1} max={document.maxCopies} defaultValue={1} integer onCommit={(copies) => { onChange({ ...action, copies }); }} /></Field>
      <PlayLengthFields document={document} length={action.length} onChange={(length) => { onChange({ ...action, length }); }} />
    </>}
    {press && <PlayedValues document={document} clipId={action.clipId} values={action.values} notes={notes} onChange={(values) => { onChange({ ...action, values }); }} />}
  </>;
}

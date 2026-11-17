import type { ConsoleAction, ConsoleGuiDocument, ConsolePlayLength, GuiMember } from "../../../types";
import { useFieldProblem } from "../problems";
import { Field, NumberField, Select } from "../../kit";
import { controlKey, controlRef } from "./consoleEditing";

const LENGTHS = [{ value: "fixed", label: "Fixed" }, { value: "scaled", label: "Scaled" }] as const;

/** The select's value for scaling by velocity rather than an axis. */
const BY_VELOCITY = "velocity";

const SECONDS_STEP = 0.1;

/** A length of `member`, an event or an always-playing clip, marked when its length is refused. */
function SecondsField({ label, tip, member, seconds, disabled = false, onChange }: { label: string; tip?: string; member: GuiMember; seconds: number; disabled?: boolean; onChange: (seconds: number) => void }) {
  const problem = useFieldProblem(member, { type: "length" });
  return <Field label={label} tip={tip} error={problem}><NumberField name={label} value={seconds} step={SECONDS_STEP} min={0} unit="s" disabled={disabled} invalid={problem !== null} onCommit={onChange} /></Field>;
}

/** How often a held effect starts again while it plays. */
export function RepeatField({ member, seconds, disabled = false, onChange }: { member: GuiMember; seconds: number; disabled?: boolean; onChange: (seconds: number) => void }) {
  return <SecondsField label="Loop" tip="Starts again after this long, until stopped" member={member} seconds={seconds} disabled={disabled} onChange={onChange} />;
}

/** How long a played copy lasts: a fixed length, or one scaled by the
 * press's velocity or an axis between the shortest and longest. */
function PlayLengthFields({ document, member, length, onChange }: { document: ConsoleGuiDocument; member: GuiMember; length: ConsolePlayLength; onChange: (length: ConsolePlayLength) => void }) {
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
    {length.type === "fixed" && <SecondsField label="Seconds" member={member} seconds={length.durationSeconds} onChange={(durationSeconds) => { onChange({ type: "fixed", durationSeconds }); }} />}
    {length.type === "scaled" && <>
      <Field label="By">
        <Select name="Scaled by" value={by} onChange={(value) => {
          const axis = axes.find((candidate) => controlKey(candidate.ref) === value);
          onChange({ ...length, by: axis === undefined ? { type: "velocity" } : { type: "axis", control: axis.ref } });
        }} options={[{ value: BY_VELOCITY, label: "Velocity" }, ...axes.map((axis) => ({ value: controlKey(axis.ref), label: axis.name }))]} />
      </Field>
      <SecondsField label="Shortest" member={member} seconds={length.shortestSeconds} onChange={(shortestSeconds) => { onChange({ ...length, shortestSeconds }); }} />
      <SecondsField label="Longest" member={member} seconds={length.longestSeconds} onChange={(longestSeconds) => { onChange({ ...length, longestSeconds }); }} />
    </>}
  </>;
}

/** What a `Play` does to the effect it plays, its copies and length, or how
 * often a `Start` or `Toggle` repeats it. */
export function PlayFields({ document, on, action, onChange }: {
  document: ConsoleGuiDocument;
  /** The event the action answers. */
  on: number;
  action: ConsoleAction;
  onChange: (action: ConsoleAction) => void;
}) {
  const member: GuiMember = { type: "event", id: on };
  const copiesProblem = useFieldProblem(member, { type: "copies" });
  if (action.type === "start" || action.type === "toggle") return <RepeatField member={member} seconds={action.lengthSeconds} onChange={(lengthSeconds) => { onChange({ ...action, lengthSeconds }); }} />;
  if (action.type !== "play") return null;
  return <>
    <Field label="Copies" error={copiesProblem}><NumberField name="Copies" value={action.copies} step={1} min={1} max={document.maxCopies} defaultValue={1} integer invalid={copiesProblem !== null} onCommit={(copies) => { onChange({ ...action, copies }); }} /></Field>
    <PlayLengthFields document={document} member={member} length={action.length} onChange={(length) => { onChange({ ...action, length }); }} />
  </>;
}

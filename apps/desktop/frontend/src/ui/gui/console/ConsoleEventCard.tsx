import type { ConsoleAction, ConsoleEventKind, ConsoleGuiDocument, ConsoleOn, ConsolePlayLength, ConsolePlayedQuantity, ConsolePlayedValue } from "../../../types";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { ConsoleInputPicker } from "./ConsoleFields";
import { applyConsoleEdit, applyConsoleItemEdit, clipUse, firstControl, setConsoleActions } from "./consoleEditing";

type ActionKind = ConsoleAction["type"];

/** A new `Play` copy's length. */
const DEFAULT_PLAY_SECONDS = 1;

const ACTION_KINDS: { kind: ActionKind; label: string }[] = [
  { kind: "play", label: "Play a copy" },
  { kind: "start", label: "Start" },
  { kind: "stop", label: "Stop" },
  { kind: "toggle", label: "Toggle" },
  { kind: "mark", label: "Add a live mark" },
  { kind: "set", label: "Set a live automation" }
];

/** Every quantity but velocity needs a button that reads notes. */
const PLAYED_QUANTITIES: { quantity: ConsolePlayedQuantity; label: string; needsNote: boolean }[] = [
  { quantity: "velocity", label: "Velocity", needsNote: false },
  { quantity: "pitch", label: "Pitch (place in the note range)", needsNote: true },
  { quantity: "pitchClass", label: "Note name (C to B)", needsNote: true },
  { quantity: "fifths", label: "Circle of fifths", needsNote: true }
];

/** `items` with the one at `index` moved by `offset`. */
function moved<T>(items: T[], index: number, offset: number): T[] {
  const next = [...items];
  const [item] = next.splice(index, 1);
  if (item !== undefined) next.splice(index + offset, 0, item);
  return next;
}

/** Whether the event's control reads notes, as values played from a note need. */
function readsNotes(document: ConsoleGuiDocument, on: ConsoleOn): boolean {
  const device = document.inputDevices.find((candidate) => guiObjectKey(candidate.device) === guiObjectKey(on.input.control.device));
  return device?.buttons.find((button) => button.control === on.input.control.control)?.notes ?? false;
}

/** A new action of `kind` from the console's first choices, or `null` when
 * the console has none: `Play` takes a clip no `Start`, `Toggle` or `always`
 * holds, and `Start` and `Toggle` one no `Play` plays. */
function defaultAction(document: ConsoleGuiDocument, kind: ActionKind): ConsoleAction | null {
  const clip = (fits: (use: ReturnType<typeof clipUse>) => boolean) => document.clips.find((candidate) => fits(clipUse(document, candidate.id)))?.id;
  switch (kind) {
    case "play": {
      const clipId = clip((use) => use !== "held");
      return clipId === undefined ? null : { type: "play", clipId, copies: 1, length: { type: "fixed", durationSeconds: DEFAULT_PLAY_SECONDS }, values: [] };
    }
    case "start":
    case "toggle": {
      const clipId = clip((use) => use !== "played");
      return clipId === undefined ? null : { type: kind, clipId, values: [] };
    }
    case "stop": {
      const clipId = document.clips[0]?.id;
      return clipId === undefined ? null : { type: "stop", clipId };
    }
    case "mark": {
      const marks = document.liveMarks[0];
      return marks === undefined ? null : { type: "mark", into: marks.key };
    }
    case "set": {
      const automation = document.automations[0];
      return automation === undefined ? null : { type: "set", into: automation.id };
    }
  }
}

/** An event: what it answers, and its actions in order. */
export function ConsoleEventCard({ document, on }: { document: ConsoleGuiDocument; on: ConsoleOn }) {
  const axis = on.event === "move";
  const notes = readsNotes(document, on);
  const set = (actions: ConsoleAction[]) => void setConsoleActions(on.id, actions);
  const offered = (kind: ActionKind) => (!axis || kind === "set") && defaultAction(document, kind) !== null;
  const firstOffered = ACTION_KINDS.find(({ kind }) => offered(kind));
  return <div className="setup-section console-item">
    <div className="inspector-inline-row">
      <label>When<select value={on.event} onChange={(event) => {
        const next = event.currentTarget.value as ConsoleEventKind;
        // A move reads an axis, and a press or release a button.
        const input = (next === "move") === axis ? on.input.control : firstControl(document.inputDevices, next === "move");
        if (input !== null) void applyConsoleEdit({ type: "setEvent", id: on.id, event: next, input });
      }}>
        <option value="press" disabled={axis && firstControl(document.inputDevices, false) === null}>Pressed</option>
        <option value="release" disabled={axis && firstControl(document.inputDevices, false) === null}>Released</option>
        <option value="move" disabled={!axis && firstControl(document.inputDevices, true) === null}>Moved</option>
      </select></label>
      <ConsoleInputPicker document={document} input={on.input} axis={axis} onChange={(input) => void applyConsoleEdit({ type: "setEvent", id: on.id, event: on.event, input })} />
      <button type="button" onClick={() => void applyConsoleItemEdit({ type: "delete", item: { type: "event", id: on.id } })}>Delete event</button>
    </div>
    {on.actions.length === 0 && <p className="object-overview-empty">The event does nothing yet. Add an action to play a clip, add a mark or set a live automation.</p>}
    {on.actions.map((action, index) => <div key={index} className="console-item">
      <div className="inspector-inline-row">
        <label>Action<select value={action.type} onChange={(event) => {
          const next = defaultAction(document, event.currentTarget.value as ActionKind);
          if (next !== null) set(on.actions.map((other, i) => i === index ? next : other));
        }}>
          {ACTION_KINDS.map(({ kind, label }) => <option key={kind} value={kind} disabled={kind !== action.type && !offered(kind)}>{label}</option>)}
        </select></label>
        <ActionFields document={document} action={action} onChange={(next) => { set(on.actions.map((other, i) => i === index ? next : other)); }} />
        <button type="button" disabled={index === 0} onClick={() => { set(moved(on.actions, index, -1)); }}>Move up</button>
        <button type="button" disabled={index === on.actions.length - 1} onClick={() => { set(moved(on.actions, index, 1)); }}>Move down</button>
        <button type="button" onClick={() => { set(on.actions.filter((_, i) => i !== index)); }}>Remove</button>
      </div>
      {action.type === "play" && <PlayLengthFields document={document} length={action.length} onChange={(length) => { set(on.actions.map((other, i) => i === index ? { ...action, length } : other)); }} />}
      {(action.type === "play" || action.type === "start" || action.type === "toggle") && on.event === "press" && <PlayedValues document={document} clipId={action.clipId} values={action.values} notes={notes}
        onChange={(values) => { set(on.actions.map((other, i) => i === index ? { ...action, values } : other)); }} />}
    </div>)}
    <button type="button" disabled={firstOffered === undefined} onClick={() => {
      const next = firstOffered === undefined ? null : defaultAction(document, firstOffered.kind);
      if (next !== null) set([...on.actions, next]);
    }}>Add action</button>
  </div>;
}

/** The clip, collection or automation an action names, and a `Play`'s copies. */
function ActionFields({ document, action, onChange }: { document: ConsoleGuiDocument; action: ConsoleAction; onChange: (action: ConsoleAction) => void }) {
  switch (action.type) {
    case "play":
    case "start":
    case "stop":
    case "toggle": {
      // A clip is played by `Play` actions or held by the others, not both.
      const fits = (clipId: number) => {
        const use = clipUse(document, clipId);
        return action.type === "stop" || clipId === action.clipId || use === null || use === (action.type === "play" ? "played" : "held");
      };
      return <>
        <label>Clip<select value={action.clipId} onChange={(event) => {
          // Played values name the old clip's parameters.
          const clipId = Number(event.currentTarget.value);
          onChange(action.type === "stop" ? { ...action, clipId } : { ...action, clipId, values: [] });
        }}>
          {!document.clips.some((clip) => clip.id === action.clipId) && <option value={action.clipId} disabled>Missing clip</option>}
          {document.clips.map((clip) => <option key={clip.id} value={clip.id} disabled={!fits(clip.id)}>{clip.name}</option>)}
        </select></label>
        {action.type === "play" && <label>Copies<input key={action.copies} type="number" min={1} max={document.maxCopies} step={1} defaultValue={action.copies} onBlur={(event) => {
          const copies = Number(event.currentTarget.value);
          if (copies !== action.copies) onChange({ ...action, copies });
        }} /></label>}
      </>;
    }
    case "mark":
      return <label>Live marks<select value={action.into} onChange={(event) => { onChange({ type: "mark", into: event.currentTarget.value }); }}>
        {!document.liveMarks.some((marks) => marks.key === action.into) && <option value={action.into} disabled>{action.into}</option>}
        {document.liveMarks.map((marks) => <option key={marks.key} value={marks.key}>{marks.key}</option>)}
      </select></label>;
    case "set":
      return <label>Live automation<select value={action.into} onChange={(event) => { onChange({ type: "set", into: Number(event.currentTarget.value) }); }}>
        {!document.automations.some((automation) => automation.id === action.into) && <option value={action.into} disabled>Missing automation</option>}
        {document.automations.map((automation) => <option key={automation.id} value={automation.id}>{automation.name}</option>)}
      </select></label>;
  }
}

/** A seconds field that commits a changed value when it loses focus. */
function SecondsField({ label, seconds, onChange }: { label: string; seconds: number; onChange: (seconds: number) => void }) {
  return <label>{label}<input key={seconds} type="number" min={0} step="any" defaultValue={seconds} onBlur={(event) => {
    const next = Number(event.currentTarget.value);
    if (next !== seconds) onChange(next);
  }} /></label>;
}

/** How long a `Play`'s copy plays: a fixed length, or one scaled by the
 * press's velocity or a live automation between the shortest and longest. */
function PlayLengthFields({ document, length, onChange }: { document: ConsoleGuiDocument; length: ConsolePlayLength; onChange: (length: ConsolePlayLength) => void }) {
  const by = length.type === "scaled" ? (length.by.type === "velocity" ? "velocity" : `${length.by.automationId}`) : "";
  return <div className="inspector-inline-row">
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
    {length.type === "scaled" && <>
      <label>By<select value={by} onChange={(event) => {
        const value = event.currentTarget.value;
        onChange({ ...length, by: value === "velocity" ? { type: "velocity" } : { type: "automation", automationId: Number(value) } });
      }}>
        <option value="velocity">Velocity</option>
        {document.automations.map((automation) => <option key={automation.id} value={`${automation.id}`}>{automation.name}</option>)}
      </select></label>
      <SecondsField label="Shortest" seconds={length.shortestSeconds} onChange={(shortestSeconds) => { onChange({ ...length, shortestSeconds }); }} />
      <SecondsField label="Longest" seconds={length.longestSeconds} onChange={(longestSeconds) => { onChange({ ...length, longestSeconds }); }} />
    </>}
  </div>;
}

/** Parameters a press sets on the copy it starts, from how it was played.
 * A parameter a live automation sets is not played. */
function PlayedValues({ document, clipId, values, notes, onChange }: {
  document: ConsoleGuiDocument;
  clipId: number;
  values: ConsolePlayedValue[];
  notes: boolean;
  onChange: (values: ConsolePlayedValue[]) => void;
}) {
  const params = (document.clips.find((clip) => clip.id === clipId)?.params ?? [])
    .filter((param) => param.supportsAutomation && param.automation === null)
    .map((param) => param.name);
  const free = params.find((param) => !values.some((value) => value.param === param));
  return <div className="setup-section">
    <h5>Played values</h5>
    {values.length === 0 && <p className="object-overview-empty">Each press starts the clip as set. Add a played value to set a parameter from how hard or which note was played.</p>}
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

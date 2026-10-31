import { useMemo, useState } from "react";
import type { ConsoleAction, ConsoleGuiDocument, ConsoleOn } from "../../../types";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { InspectorScrollArea } from "../InspectorScrollArea";
import { LayerList } from "../sequence/sequenceLayers";
import type { GuiFocus } from "../shared";
import { PlayFields } from "./ConsoleActionFields";
import { EffectFields } from "./ConsoleEffectFields";
import { applyConsoleEdit, applyConsoleItemEdit, consoleComposition, marksReaders, targetLabel } from "./consoleEditing";

type Tab = "selection" | "layers";

const TABS: { id: Tab; label: string }[] = [
  { id: "selection", label: "Selection" },
  { id: "layers", label: "Layers" }
];

/** The side panel of a console, as a sequence's: the selected action and its effect, or the layers. */
export function ConsoleInspector({ document, selected, setSelected }: {
  document: ConsoleGuiDocument;
  selected: GuiFocus;
  setSelected: (focus: GuiFocus) => void;
}) {
  const [tab, setTab] = useState<Tab>("selection");
  const composition = useMemo(() => consoleComposition(document), [document]);
  const footer = <div className="sequence-inspector-tabs" role="tablist" aria-label="Console inspector sections">
    {TABS.map((entry) => <button key={entry.id} type="button" role="tab" aria-selected={tab === entry.id} className={tab === entry.id ? "active" : ""} onClick={() => { setTab(entry.id); }}>{entry.label}</button>)}
  </div>;
  return <InspectorScrollArea footer={footer}>
    {tab === "selection" ? <Selection document={document} selected={selected} setSelected={setSelected} />
      : <><h2>Layers</h2><LayerList composition={composition} /></>}
  </InspectorScrollArea>;
}

function Selection({ document, selected, setSelected }: { document: ConsoleGuiDocument; selected: GuiFocus; setSelected: (focus: GuiFocus) => void }) {
  if (selected?.type === "consoleAction") {
    const on = document.events.find((candidate) => candidate.id === selected.on);
    const action = on?.actions[selected.index];
    if (on !== undefined && action !== undefined) return <ActionPanel document={document} on={on} index={selected.index} action={action} setSelected={setSelected} />;
  }
  if (selected?.type === "consoleEffect") {
    const clip = document.clips.find((candidate) => candidate.id === selected.id);
    if (clip !== undefined) {
      const always = document.always.includes(clip.id);
      return <>
        <h2>Effect</h2>
        <label><input type="checkbox" checked={always} disabled={!always && document.events.some((on) => on.actions.some((action) => action.type === "play" && action.clipId === clip.id))}
          onChange={() => void applyConsoleEdit({ type: "setAlways", clipIds: always ? document.always.filter((id) => id !== clip.id) : [...document.always, clip.id] })} /> Plays for the whole session</label>
        <EffectFields document={document} clip={clip} />
        <button type="button" className="danger" onClick={() => { void applyConsoleItemEdit({ type: "delete", item: { type: "clip", id: clip.id } }).then(() => { setSelected(null); }); }}>Delete effect</button>
      </>;
    }
  }
  return <>
    <h2>Console</h2>
    <p>Select an action to edit it and the effect it plays.</p>
  </>;
}

const EVENT_LABELS = { press: "pressed", release: "released", move: "moved" } as const;

/** One action of a control: how it plays, the effect or parameter it names, and removing it. */
function ActionPanel({ document, on, index, action, setSelected }: {
  document: ConsoleGuiDocument;
  on: ConsoleOn;
  index: number;
  action: ConsoleAction;
  setSelected: (focus: GuiFocus) => void;
}) {
  const device = document.inputDevices.find((candidate) => guiObjectKey(candidate.device) === guiObjectKey(on.input.control.device));
  const notes = device?.buttons.find((button) => button.control === on.input.control.control)?.notes ?? false;
  const set = (next: ConsoleAction) => void applyConsoleEdit({ type: "setActions", id: on.id, actions: on.actions.map((other, i) => i === index ? next : other) });
  const clipId = action.type === "mark" ? undefined : action.type === "set" ? (action.target.type === "clipParam" ? action.target.clipId : undefined) : action.clipId;
  const clip = clipId === undefined ? undefined : document.clips.find((candidate) => candidate.id === clipId);
  const plays = action.type !== "mark" && action.type !== "set";
  const uses = clip === undefined || !plays ? 0 : document.events.reduce((total, other) => total + other.actions.filter((candidate) => candidate.type !== "mark" && candidate.type !== "set" && candidate.clipId === clip.id).length, 0);
  return <>
    <h2>{on.input.controlName} {EVENT_LABELS[on.event]}</h2>
    {action.type === "mark" && <>
      <label>Live marks<select value={action.into} onChange={(event) => { set({ type: "mark", into: event.currentTarget.value }); }}>
        {!document.liveMarks.some((marks) => marks.key === action.into) && <option value={action.into} disabled>{action.into}</option>}
        {document.liveMarks.map((marks) => <option key={marks.key} value={marks.key}>{marks.key}</option>)}
      </select></label>
      <p className="clip-param-description">{marksReaders(document, action.into).length === 0 ? "No effect reads these marks." : `Read by ${marksReaders(document, action.into).join(", ")}.`}</p>
    </>}
    {action.type === "set" && <p className="clip-param-description">
      Sets {targetLabel(document, action.target)}, across its range. It keeps its own value until {on.input.controlName} first {on.event === "move" ? "moves" : "is pressed"}.
      {clip === undefined && " Its starting value is its value in the layers and operators editor."}
    </p>}
    <PlayFields document={document} action={action} press={on.event === "press"} notes={notes} onChange={set} />
    {clip !== undefined && <>
      <div className="inspector-section-divider" />
      <h2>Effect</h2>
      {uses > 1 && <p className="clip-param-description">Also played by {uses - 1} other action{uses > 2 ? "s" : ""}; changes here apply to all of them.</p>}
      <EffectFields document={document} clip={clip} />
    </>}
    <div className="inspector-section-divider" />
    <button type="button" className="danger" onClick={() => { void applyConsoleEdit({ type: "removeAction", on: on.id, index }).then(() => { setSelected(null); }); }}>Remove from {on.input.controlName}</button>
  </>;
}

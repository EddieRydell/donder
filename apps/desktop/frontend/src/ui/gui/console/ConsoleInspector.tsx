import { Link2, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import type { ConsoleAction, ConsoleGuiDocument, ConsoleOn, GuiClip } from "../../../types";
import { Badge, Button, Field, Select, Tabs, Toggle } from "../../kit";
import { InspectorScrollArea } from "../InspectorScrollArea";
import { LayerList } from "../sequence/sequenceLayers";
import { ClipFields, EnvelopeFields } from "../clip/ClipFields";
import { focusedClipId, setDocumentUi, useDocumentUi } from "../documentUi";
import { useSequenceClipDetails } from "../sequence/sequenceClipDetails";
import { PlayFields, RepeatField } from "./ConsoleActionFields";
import { MarksReadersBadge } from "./ConsoleEditor";
import { consoleClipFields, consoleComposition } from "./consoleComposition";
import { putActions, putAlways, targetLabel } from "./consoleEditing";
import { remove, site } from "../edits";

type Tab = "selection" | "layers";

const TABS: { value: Tab; label: string }[] = [
  { value: "selection", label: "Selection" },
  { value: "layers", label: "Layers" }
];

/** The side panel of a console, as a sequence's: the selected action and its effect, or the layers. */
export function ConsoleInspector({ document }: { document: ConsoleGuiDocument }) {
  const [tab, setTab] = useState<Tab>("selection");
  const composition = useMemo(() => consoleComposition(document), [document]);
  const footer = <Tabs name="Console inspector sections" value={tab} options={TABS} onChange={setTab} />;
  return <InspectorScrollArea footer={footer}>
    {tab === "selection" ? <Selection document={document} />
      : <LayerList composition={composition} />}
  </InspectorScrollArea>;
}

function Selection({ document }: { document: ConsoleGuiDocument }) {
  const focus = useDocumentUi((ui) => ui.focus);
  if (focus?.type === "consoleAction") {
    const on = document.events.find((candidate) => candidate.id === focus.on);
    const action = on?.actions[focus.index];
    if (on !== undefined && action !== undefined) return <ActionPanel document={document} on={on} index={focus.index} action={action} />;
  }
  const id = focusedClipId(focus);
  const clip = document.clips.find((candidate) => candidate.id === id);
  if (clip === undefined) return <h2>Console</h2>;
  const deleteClip = () => { void remove(site({ type: "clip", id: clip.id })).then((update) => { if (update !== null) setDocumentUi({ focus: null }); }); };
  if (clip.content.type === "envelope") return <><h2>Envelope</h2><EnvelopeFields clip={clip} /><Button variant="danger" icon={Trash2} onClick={deleteClip}>Delete envelope</Button></>;
  const always = clip.alwaysLengthSeconds;
  return <>
    <h2>Effect</h2>
    <Field label="Always" tip="Plays from the moment you perform">
      <Toggle name="Plays from the moment you perform" checked={always !== null} disabled={always === null && document.events.some((on) => on.actions.some((action) => action.type === "play" && action.clipId === clip.id))}
        onChange={() => void putAlways(clip.id, always === null ? document.heldLengthSeconds : null)} />
    </Field>
    <RepeatField member={{ type: "clip", id: clip.id }} seconds={always ?? document.heldLengthSeconds} disabled={always === null} onChange={(lengthSeconds) => void putAlways(clip.id, lengthSeconds)} />
    <EffectFields document={document} clip={clip} />
    <Button variant="danger" icon={Trash2} onClick={deleteClip}>Delete effect</Button>
  </>;
}

const EVENT_LABELS = { press: "pressed", release: "released", move: "moved" } as const;

/** One action of a control: how it plays, the effect or parameter it names, and removing it. */
function ActionPanel({ document, on, index, action }: {
  document: ConsoleGuiDocument;
  on: ConsoleOn;
  index: number;
  action: ConsoleAction;
}) {
  const set = (next: ConsoleAction) => void putActions(on, on.actions.map((other, i) => i === index ? next : other));
  const clipId = action.type === "mark" ? undefined : action.type === "set" ? (action.target.type === "clipParam" ? action.target.clipId : undefined) : action.clipId;
  const clip = clipId === undefined ? undefined : document.clips.find((candidate) => candidate.id === clipId);
  const plays = action.type !== "mark" && action.type !== "set";
  const uses = clip === undefined || !plays ? 0 : document.events.reduce((total, other) => total + other.actions.filter((candidate) => candidate.type !== "mark" && candidate.type !== "set" && candidate.clipId === clip.id).length, 0);
  return <>
    <h2>{on.input.controlName} {EVENT_LABELS[on.event]}</h2>
    {action.type === "mark" && <Field label="Live marks">
      <Select name="Live marks" value={String(action.into)} onChange={(into) => { set({ type: "mark", into: Number(into) }); }} options={[
        ...(document.markCollections.some((marks) => marks.id === action.into) ? [] : [{ value: String(action.into), label: "Missing live marks", disabled: true }]),
        ...document.markCollections.map((marks) => ({ value: String(marks.id), label: marks.name }))
      ]} />
      <MarksReadersBadge document={document} id={action.into} />
    </Field>}
    {action.type === "set" && <Field label="Sets" tip={`Across its range. It rests at the low end until ${on.input.controlName} first ${on.event === "move" ? "moves" : "is pressed"}.`}>
      <span>{targetLabel(document, action.target)}</span>
    </Field>}
    <PlayFields document={document} on={on.id} action={action} onChange={set} />
    {clip !== undefined && <>
      <div className="inspector-section-divider" />
      <h2>Effect {uses > 1 && <Badge icon={Link2} tip={`Played by ${uses} actions; changes here apply to all of them`}>{uses}</Badge>}</h2>
      <EffectFields document={document} clip={clip} />
    </>}
    <div className="inspector-section-divider" />
    <Button variant="danger" icon={Trash2} tip={`Remove from ${on.input.controlName}`} onClick={() => { void putActions(on, on.actions.filter((_, i) => i !== index)).then((update) => { if (update !== null) setDocumentUi({ focus: null }); }); }}>Remove action</Button>
  </>;
}

/** An effect the console plays: its fields, with parameters linked to knobs and presses. */
function EffectFields({ document, clip }: { document: ConsoleGuiDocument; clip: GuiClip }) {
  const details = useSequenceClipDetails(document, [clip.id]);
  const adapter = useMemo(() => consoleClipFields(document), [document]);
  return <ClipFields clips={[clip]} details={details} adapter={adapter} />;
}

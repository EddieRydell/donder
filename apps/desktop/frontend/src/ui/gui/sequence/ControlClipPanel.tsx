import { useState } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import type { GuiDocumentRequest, SequenceControlClip, SequenceEditorDocument } from "../../../types";
import { NumberField } from "../setup/PatchInputs";
import { ControlValueInput, controlValueLabel, initialControlValue } from "./ControlValueInput";

import { sameControlChannel } from "./sequenceTargets";
import { roundToNanosecond } from "../shared";

type ControlDraft = Omit<SequenceControlClip, "id" | "targetLabel"> & { id: number | null; origin: GuiDocumentRequest };


export function ControlClipPanel({ document, selectedId = null }: { document: SequenceEditorDocument; selectedId?: number | null }) {
  const [draft, setDraft] = useState<ControlDraft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const pending = useAppStore((state) => state.guiEditPending);
  const ready = request !== null && request.projectRevision === revision && !pending;
  const stale = draft !== null && draft.origin !== request;
  const channelIndex = draft === null ? -1 : document.controlChannels.findIndex((channel) => sameControlChannel(channel.target, draft.target));
  const channel = document.controlChannels[channelIndex];
  const begin = (clip?: SequenceControlClip, duplicate = false) => {
    if (!ready) return;
    setError(null);
    if (clip !== undefined) {
      setDraft({ ...structuredClone(clip), id: duplicate ? null : clip.id, startSeconds: duplicate ? clip.startSeconds + clip.durationSeconds : clip.startSeconds, origin: request });
    } else {
      const channel = document.controlChannels[0];
      if (channel === undefined) return;
      setDraft({ id: null, origin: request, target: structuredClone(channel.target), value: initialControlValue(channel.options), startSeconds: 0, durationSeconds: Math.min(1, document.durationSeconds) });
    }
  };
  return <>
    <h2>Typed Controls</h2>
    {document.controlChannels.length === 0 && <p>Add scalar, indexed, or fixture instances in Display Setup to create controls.</p>}
    <button type="button" disabled={!ready || draft !== null || document.controlChannels.length === 0} onClick={() => { begin(); }}>Add control clip</button>
    {error !== null && <p role="alert">{error}</p>}
    {draft !== null && <form className="control-clip-inspector" onSubmit={(event) => {
      event.preventDefault();
      void runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, { type: "upsertControlClip", id: draft.id, startSeconds: draft.startSeconds, durationSeconds: draft.durationSeconds, target: draft.target, value: draft.value }), draft.origin)
        .then(() => { setDraft(null); setError(null); }).catch((error: unknown) => { setError(String(error)); });
    }}>
      <strong>{draft.id === null ? "New control clip" : `Edit control ${draft.id}`}</strong>
      {stale && <p role="alert">The project changed. Discard this draft and reopen the current clip before applying changes.</p>}
      <fieldset disabled={!ready || stale}>
        <label>Control target<select value={channelIndex} onChange={(event) => {
          const channel = document.controlChannels[Number(event.target.value)];
          if (channel !== undefined) setDraft({ ...draft, target: structuredClone(channel.target), value: initialControlValue(channel.options) });
        }}>
          {channelIndex < 0 && <option value={-1} disabled>Target is unavailable</option>}
          {document.controlChannels.map((channel, index) => <option key={index} value={index}>{channel.label}</option>)}
        </select></label>
        <NumberField label="Start (seconds)" value={draft.startSeconds} max={document.durationSeconds} step="any" onChange={(startSeconds) => { setDraft({ ...draft, startSeconds }); }} />
        <NumberField label="Duration (seconds)" value={draft.durationSeconds} min={0.000001} max={document.durationSeconds} step="any" onChange={(durationSeconds) => { setDraft({ ...draft, durationSeconds }); }} />
        {channel !== undefined && channel.cellCount !== null && <label><input type="checkbox" checked={draft.target.cells !== null} onChange={(event) => { const count = channel.cellCount; if (count !== null) setDraft({ ...draft, target: { ...draft.target, cells: event.target.checked ? { start: 0, count } : null } }); }} />Select specific cells</label>}
        {draft.target.cells !== null && <>
          <NumberField label="First cell" min={1} value={draft.target.cells.start + 1} onChange={(start) => { const cells = draft.target.cells; if (cells !== null) setDraft({ ...draft, target: { ...draft.target, cells: { ...cells, start: start - 1 } } }); }} />
          <NumberField label="Cell count" min={1} value={draft.target.cells.count} onChange={(count) => { const cells = draft.target.cells; if (cells !== null) setDraft({ ...draft, target: { ...draft.target, cells: { ...cells, count } } }); }} />
        </>}
        {channel !== undefined && <ControlValueInput options={channel.options} value={draft.value} disabled={!ready || stale} onChange={(value) => { setDraft({ ...draft, value }); }} />}
        <p>Curve and gradient positions run from 0 to 1 across the clip. Apply saves timing, target, and value together.</p>
        <button type="submit" disabled={channel === undefined}>Apply control clip</button>
      </fieldset>
      <button type="button" disabled={pending} onClick={() => { setDraft(null); setError(null); }}>Discard draft</button>
    </form>}
    {document.controlClips.filter((clip) => selectedId === null || clip.id === selectedId).map((clip) => <div className="control-clip-inspector" key={clip.id}>
      <strong>{document.controlChannels.find((channel) => sameControlChannel(channel.target, clip.target))?.label ?? clip.targetLabel}</strong>
      <span>{clip.startSeconds}s – {roundToNanosecond(clip.startSeconds + clip.durationSeconds)}s · {controlValueLabel(clip.value)}</span>
      {clip.target.cells !== null && <span>Cells {clip.target.cells.start + 1}–{clip.target.cells.start + clip.target.cells.count}</span>}
      <div className="control-clip-actions">
        <button type="button" disabled={!ready || draft !== null} onClick={() => { begin(clip); }}>Edit</button>
        <button type="button" disabled={!ready || draft !== null} onClick={() => { begin(clip, true); }}>Duplicate</button>
        <button type="button" disabled={!ready || draft !== null} onClick={() => void runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, { type: "deleteControlClip", id: clip.id })).catch((error: unknown) => { setError(String(error)); })}>Delete</button>
      </div>
    </div>)}
  </>;
}

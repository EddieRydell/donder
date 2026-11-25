import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import { useMemo, useState } from "react";

import { Circle, CircleDot, Eye, EyeOff, Plus, Trash2 } from "lucide-react";

import type {
  SequenceGuiDocument,
  SequenceMarkCollection,
  SequenceMarkRef,
  SequenceSelection
} from "../../../types";
import { describe, put, recolor, remove, rename, site } from "../edits";

/** Write the marks of `collection` with the mark at `index` changed. */
function putMark(collection: SequenceMarkCollection, index: number, change: { durationSeconds?: number; label?: string | null }) {
  const marks = collection.marksSeconds.map((timeSeconds, at) => ({
    timeSeconds,
    durationSeconds: at === index && change.durationSeconds !== undefined ? change.durationSeconds : collection.markDurationsSeconds[at] ?? 0,
    label: at === index && change.label !== undefined ? change.label : collection.markLabels[at] ?? null
  }));
  return put(site({ type: "marks", id: collection.id }, { type: "points" }), { type: "marks", marks });
}
import { ColorPicker } from "../../ColorPicker";
import { AddRow, Button, IconButton, NumberField, Problem, Section, Select, Tabs, TextField } from "../../kit";
import { DescriptionField } from "../DescriptionField";
import { useFieldProblem } from "../problems";
import { InspectorScrollArea, Readout } from "../InspectorScrollArea";
import { roundToNanosecond } from "../shared";
import { sequenceComposition } from "./composition";
import { activeMarkCollection, defaultMarkColor } from "./marks";
import { BeatDetectionDialog } from "./BeatDetectionDialog";
import { MarkSubdivision } from "./MarkSubdivision";
import { selectionCompatibleWithFocusedItem, selectionCount, useSequenceSelection } from "./sequenceSelection";
import { useSequenceClipDetails } from "./sequenceClipDetails";
import { targetsEqual } from "./sequenceTargets";
import { LayerList } from "./sequenceLayers";
import { ClipFields, EnvelopeFields, type ClipFieldsAdapter } from "../clip/ClipFields";
import { focusedClipId, setDocumentUi, showMarks, useDocumentUi, type GuiFocus } from "../documentUi";

type SequenceInspectorTab = "clip" | "layers" | "marks";

type SelectedMarkEntry = {
  ref: SequenceMarkRef;
  collection: SequenceMarkCollection;
  timeSeconds: number;
  durationSeconds: number;
  label: string | null;
};

const SEQUENCE_INSPECTOR_TABS: { value: SequenceInspectorTab; label: string }[] = [
  { value: "clip", label: "Clip" },
  { value: "layers", label: "Layers" },
  { value: "marks", label: "Marks" }
];

/** The step a mark time scrubs by, in seconds. */
const TIME_STEP_SECONDS = 0.01;

export function SequenceInspector({ document }: { document: SequenceGuiDocument }) {
  const [activeTab, setActiveTab] = useState<SequenceInspectorTab>("clip");
  const footer = <Tabs name="Sequence inspector sections" value={activeTab} options={SEQUENCE_INSPECTOR_TABS} onChange={setActiveTab} />;

  return (
    <InspectorScrollArea footer={footer}>
      {activeTab === "clip" && <ClipInspectorPanel document={document} />}
      {activeTab === "layers" && <LayerInspectorPanel document={document} />}
      {activeTab === "marks" && <MarkInspectorPanel document={document} />}
    </InspectorScrollArea>
  );
}

function DeleteButton({ onDelete }: { onDelete: () => void }) {
  return <Button variant="danger" icon={Trash2} tip="Delete" command="edit.delete" onClick={onDelete}>Delete</Button>;
}

/** A sequence's clips, whose parameters bind to its envelopes. */
function sequenceClipFields(document: SequenceGuiDocument): ClipFieldsAdapter {
  const envelopes = document.clips.filter((clip) => clip.content.type === "envelope");
  return {
    document,
    paramControl: (clip, param) => ({ automation: {
      target: { type: "clipParam", clipId: clip.id, param: param.name },
      envelopes,
      canCreate: document.targets.some((lane) => targetsEqual(lane.target, clip.target))
    } })
  };
}

/** The focused clip, or the selected clips edited together. */
function ClipInspectorPanel({ document }: { document: SequenceGuiDocument }) {
  const focus = useDocumentUi((ui) => ui.focus);
  const selection = useSequenceSelection(document);
  const adapter = useMemo(() => sequenceClipFields(document), [document]);
  const several = selection?.type === "clips" && selectionCount(selection) > 1 && selectionCompatibleWithFocusedItem(selection, focus);
  const focused = focusedClipId(focus);
  const ids = several ? selection.ids : focused === null ? [] : [focused];
  const clips = document.clips.filter((clip) => ids.includes(clip.id));
  const details = useSequenceClipDetails(document, clips.filter((clip) => clip.content.type === "effect").map((clip) => clip.id));
  const deleteClips = () => {
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: { type: "deleteSelection", selection: { type: "clips", ids } } })
    ).then((result) => {
      if (result !== null) setDocumentUi({ focus: null, selection: null });
    });
  };
  const [clip] = clips;
  if (clip === undefined) return <h2>Clip</h2>;
  if (several) {
    return <>
      <h2>Clips</h2>
      <div className="inspector-readout-grid"><Readout label="Selected" value={String(clips.length)} /></div>
      {clips.every((candidate) => candidate.content.type === "effect") && <ClipFields clips={clips} details={details} adapter={adapter} />}
      <DeleteButton onDelete={deleteClips} />
    </>;
  }
  return <>
    <h2>{clip.content.type === "envelope" ? "Envelope" : "Clip"}</h2>
    {clip.content.type === "envelope" ? <EnvelopeFields clip={clip} /> : <ClipFields clips={clips} details={details} adapter={adapter} />}
    <DeleteButton onDelete={deleteClips} />
  </>;
}

function LayerInspectorPanel({ document }: { document: SequenceGuiDocument }) {
  const composition = useMemo(() => sequenceComposition(document, null), [document]);
  return <LayerList composition={composition} />;
}

function MarkInspectorPanel({ document }: { document: SequenceGuiDocument }) {
  const selected = useDocumentUi((ui) => ui.focus);
  const sequenceSelection = useSequenceSelection(document);
  const activeMarkCollectionKey = useDocumentUi((ui) => ui.activeMarks);
  const visibleMarkCollectionKeys = useDocumentUi((ui) => ui.visibleMarks);
  const setVisibleMarkCollectionKeys = (visibleMarks: ReadonlySet<string>) => { setDocumentUi({ visibleMarks }); };
  const setActiveMarkCollectionKey = (activeMarks: string | null) => { setDocumentUi({ activeMarks }); };
  const selectedMark = selected?.type === "mark" ? { collectionId: selected.collectionId, index: selected.index } : null;
  const activeCollection = activeMarkCollection(document.markCollections, activeMarkCollectionKey);
  const selectedMarks = selectedMarkEntries(document, selected, sequenceSelection);
  const collectionOptions = document.markCollections.map((collection) => ({ value: String(collection.id), label: collection.name }));

  const createCollection = () => {
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: { type: "composition", edit: {
        type: "addMarkCollections",
        collections: [{ name: "Marks", color: defaultMarkColor(document.markCollections.length), marksSeconds: [] }]
      } } })
    ).then((result) => {
      if (result !== null) showMarks(result.created, true);
    });
  };

  /** Renames, carrying the active and visible state to the new name; a rejected name reverts. */
  const renameCollection = (collection: SequenceMarkCollection, value: string) => {
    const key = collection.name;
    const name = value.trim();
    if (name === "" || name === key) return;
    void rename({ type: "marks", id: collection.id }, name).then((result) => {
      if (result === null) return;
      const renamed = result.created[0] ?? key;
      if (activeMarkCollectionKey === key) setActiveMarkCollectionKey(renamed);
      if (visibleMarkCollectionKeys.has(key)) {
        setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys].map((visible) => (visible === key ? renamed : visible))));
      }
    });
  };

  const deleteCollection = (collection: SequenceMarkCollection) => {
    void remove(site({ type: "marks", id: collection.id })).then((result) => {
      if (result === null) return;
      if (selectedMark?.collectionId === collection.id) setDocumentUi({ focus: null });
      if (activeCollection?.name === collection.name) {
        setActiveMarkCollectionKey(document.markCollections.find((candidate) => candidate.name !== collection.name)?.name ?? null);
      }
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys].filter((key) => key !== collection.name)));
    });
  };

  const setSelectedMarkRefs = (refs: SequenceMarkRef[]) => {
    const validRefs = refs.filter((ref) => markEntry(document, ref) !== null);
    const firstRef = validRefs[0];
    setDocumentUi(firstRef === undefined ? { focus: null, selection: null } : {
      focus: { type: "mark", collectionId: firstRef.collectionId, index: firstRef.index },
      selection: { type: "marks", marks: validRefs }
    });
  };

  const moveSelectedMark = (entry: SelectedMarkEntry, timeSeconds: number) => {
    const nextTimeSeconds = roundToNanosecond(Math.max(0, timeSeconds));
    if (!Number.isFinite(nextTimeSeconds) || nextTimeSeconds === entry.timeSeconds) return;
    const nextRefs = selectedRefsAfterMove(selectedMarks.map((mark) => mark.ref), entry.collection, entry.ref, nextTimeSeconds);
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "moveMarks",
        marks: [entry.ref],
        timeDeltaSeconds: nextTimeSeconds - entry.timeSeconds
      } })
    ).then((result) => {
      if (result !== null) setSelectedMarkRefs(nextRefs);
    });
  };

  const reassignSelectedMark = (entry: SelectedMarkEntry, targetCollectionId: number) => {
    if (targetCollectionId === entry.ref.collectionId) return;
    const targetCollection = document.markCollections.find((collection) => collection.id === targetCollectionId);
    if (targetCollection === undefined) return;
    const nextRefs = selectedRefsAfterReassign(selectedMarks.map((mark) => mark.ref), entry.ref, targetCollection, entry.timeSeconds);
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "reassignMarkCollection",
        collectionId: entry.ref.collectionId,
        index: entry.ref.index,
        targetCollectionId
      } })
    ).then((result) => {
      if (result !== null) setSelectedMarkRefs(nextRefs);
    });
  };

  const spanSelectedMark = (entry: SelectedMarkEntry, durationSeconds: number) => {
    const next = roundToNanosecond(Math.max(0, durationSeconds));
    if (!Number.isFinite(next) || next === entry.durationSeconds) return;
    const collection = document.markCollections.find((candidate) => candidate.id === entry.ref.collectionId);
    if (collection !== undefined) void putMark(collection, entry.ref.index, { durationSeconds: next });
  };

  const labelSelectedMark = (entry: SelectedMarkEntry, label: string) => {
    const next = label.trim() === "" ? null : label.trim();
    if (next === entry.label) return;
    const collection = document.markCollections.find((candidate) => candidate.id === entry.ref.collectionId);
    if (collection !== undefined) void putMark(collection, entry.ref.index, { label: next });
  };

  const deleteSelectedMark = (entry: SelectedMarkEntry) => {
    const nextRefs = selectedRefsAfterDelete(selectedMarks.map((mark) => mark.ref), entry.ref);
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "deleteSelection",
        selection: { type: "marks", marks: [entry.ref] }
      } })
    ).then((result) => {
      if (result !== null) setSelectedMarkRefs(nextRefs);
    });
  };

  return (
    <>
      <h2>Marks</h2>
      <Section title="Collections" actions={<>
        <IconButton icon={Plus} label="Add collection" onClick={createCollection} />
        <BeatDetectionDialog document={document} />
      </>}>
        {document.markCollections.length === 0 && <AddRow label="Add collection" onAdd={createCollection} />}
        {document.markCollections.length > 0 && (
          <div className="mark-collection-edit-list">
            {document.markCollections.map((collection) => {
              const active = activeCollection?.name === collection.name;
              const visible = visibleMarkCollectionKeys.has(collection.name);
              return <div key={collection.name} className="mark-collection-edit-row">
                <IconButton
                  icon={active ? CircleDot : Circle}
                  pressed={active}
                  label={`Use ${collection.name} for new marks`}
                  onClick={() => { setActiveMarkCollectionKey(collection.name); }}
                />
                <IconButton
                  icon={visible ? Eye : EyeOff}
                  pressed={visible}
                  label={visible ? `Hide ${collection.name}` : `Show ${collection.name}`}
                  onClick={() => {
                    const next = new Set(visibleMarkCollectionKeys);
                    if (visible) next.delete(collection.name);
                    else next.add(collection.name);
                    setVisibleMarkCollectionKeys(next);
                  }}
                />
                <ColorPicker
                  value={collection.color}
                  label={`${collection.name} color`}
                  commit={(color) =>
                    recolor({ type: "marks", id: collection.id }, color).then(() => undefined)
                  }
                />
                <MarkCollectionName id={collection.id} name={collection.name} onCommit={(name) => { renameCollection(collection, name); }} />
                <IconButton icon={Trash2} label={`Delete ${collection.name}`} danger onClick={() => { deleteCollection(collection); }} />
              </div>;
            })}
          </div>
        )}
        {activeCollection !== null && (
          <DescriptionField description={activeCollection.description} member={{ type: "marks", id: activeCollection.id }} onCommit={(description) => describe({ type: "marks", id: activeCollection.id }, description)} />
        )}
      </Section>
        {selectedMarks.length >= 2 && (
          <MarkSubdivision document={document} selectedSeconds={selectedMarks.map((entry) => entry.timeSeconds)} />
        )}
      {selectedMarks.length > 0 && (
        <Section title={`Selected · ${selectedMarks.length}`}>
          <div className="mark-selected-list">
            {selectedMarks.map((entry, index) => (
              <div key={`${entry.ref.collectionId}:${entry.ref.index}:${index}`} className="mark-selected-row">
                <Select
                  name="Mark collection"
                  value={String(entry.ref.collectionId)}
                  options={collectionOptions}
                  onChange={(key) => { reassignSelectedMark(entry, Number(key)); }}
                />
                <NumberField
                  name="Mark time"
                  value={entry.timeSeconds}
                  min={0}
                  step={TIME_STEP_SECONDS}
                  unit="s"
                  onCommit={(timeSeconds) => { moveSelectedMark(entry, timeSeconds); }}
                />
                <NumberField
                  name="Mark duration"
                  value={entry.durationSeconds}
                  min={0}
                  step={TIME_STEP_SECONDS}
                  unit="s"
                  onCommit={(durationSeconds) => { spanSelectedMark(entry, durationSeconds); }}
                />
                <IconButton icon={Trash2} label="Delete mark" danger onClick={() => { deleteSelectedMark(entry); }} />
                <TextField
                  className="mark-label-input"
                  name="Mark label"
                  placeholder="Label"
                  value={entry.label ?? ""}
                  onCommit={(label) => { labelSelectedMark(entry, label); }}
                />
              </div>
            ))}
          </div>
        </Section>
      )}
    </>
  );
}

/** A mark collection's name in its row, marked when a rename is refused. */
function MarkCollectionName({ id, name, onCommit }: { id: number; name: string; onCommit: (name: string) => void }) {
  const problem = useFieldProblem({ type: "marks", id }, { type: "name" });
  return <Problem error={problem}><TextField name={`${name} name`} value={name} invalid={problem !== null} onCommit={onCommit} /></Problem>;
}

function markIndexAfterInsert(collection: SequenceMarkCollection, timeSeconds: number) {
  return collection.marksSeconds.filter((markTimeSeconds) => markTimeSeconds <= timeSeconds).length;
}

function selectedMarkEntries(
  document: SequenceGuiDocument,
  selected: GuiFocus,
  sequenceSelection: SequenceSelection | null
): SelectedMarkEntry[] {
  const refs =
    sequenceSelection?.type === "marks" && sequenceSelection.marks.length > 0
      ? sequenceSelection.marks
      : selected?.type === "mark"
        ? [{ collectionId: selected.collectionId, index: selected.index }]
        : [];
  const seen = new Set<string>();
  const entries: SelectedMarkEntry[] = [];
  for (const ref of refs) {
    const key = markRefKey(ref);
    if (seen.has(key)) continue;
    seen.add(key);
    const entry = markEntry(document, ref);
    if (entry !== null) entries.push(entry);
  }
  return entries;
}

function markEntry(document: SequenceGuiDocument, ref: SequenceMarkRef): SelectedMarkEntry | null {
  const collection = document.markCollections.find((candidate) => candidate.id === ref.collectionId);
  const timeSeconds = collection?.marksSeconds[ref.index];
  const durationSeconds = collection?.markDurationsSeconds[ref.index];
  if (collection === undefined || timeSeconds === undefined || durationSeconds === undefined) return null;
  return { ref, collection, timeSeconds, durationSeconds, label: collection.markLabels[ref.index] ?? null };
}

function selectedRefsAfterMove(
  refs: SequenceMarkRef[],
  collection: SequenceMarkCollection,
  movedRef: SequenceMarkRef,
  timeSeconds: number
) {
  const sorted = collection.marksSeconds
    .map((markTimeSeconds, markIndex) => ({
      markIndex,
      timeSeconds: markIndex === movedRef.index ? timeSeconds : markTimeSeconds
    }))
    .sort((left, right) => left.timeSeconds - right.timeSeconds || left.markIndex - right.markIndex);
  return refs.map((ref) => {
    if (ref.collectionId !== movedRef.collectionId) return ref;
    const nextIndex = sorted.findIndex((mark) => mark.markIndex === ref.index);
    return nextIndex < 0 ? ref : { collectionId: ref.collectionId, index: nextIndex };
  });
}

function selectedRefsAfterReassign(
  refs: SequenceMarkRef[],
  movedRef: SequenceMarkRef,
  targetCollection: SequenceMarkCollection,
  timeSeconds: number
) {
  const targetIndex = markIndexAfterInsert(targetCollection, timeSeconds);
  return refs.map((ref) => {
    if (sameMarkRef(ref, movedRef)) return { collectionId: targetCollection.id, index: targetIndex };
    if (ref.collectionId === movedRef.collectionId && ref.index > movedRef.index) {
      return { collectionId: ref.collectionId, index: ref.index - 1 };
    }
    if (ref.collectionId === targetCollection.id && ref.index >= targetIndex) {
      return { collectionId: ref.collectionId, index: ref.index + 1 };
    }
    return ref;
  });
}

function selectedRefsAfterDelete(refs: SequenceMarkRef[], deletedRef: SequenceMarkRef) {
  return refs
    .filter((ref) => !sameMarkRef(ref, deletedRef))
    .map((ref) => {
      if (ref.collectionId === deletedRef.collectionId && ref.index > deletedRef.index) {
        return { collectionId: ref.collectionId, index: ref.index - 1 };
      }
      return ref;
    });
}

function sameMarkRef(left: SequenceMarkRef, right: SequenceMarkRef) {
  return left.collectionId === right.collectionId && left.index === right.index;
}

function markRefKey(ref: SequenceMarkRef) {
  return `${ref.collectionId}:${ref.index}`;
}


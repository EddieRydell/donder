import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import { useMemo, useState } from "react";

import { CheckCheck, Circle, CircleDot, Eye, EyeOff, Link2, Plus, Trash2, Workflow } from "lucide-react";
import { THEME_COLORS } from "../../../theme";

import type {
  SequenceGuiDocument,
  SequenceAutomationTarget,
  SequenceClip,
  SequenceClipDetails,
  SequenceClipParam,
  SequenceMarkCollection,
  SequenceMarkRef,
  SequenceClipScope,
  SequenceClipCommonEdit
} from "../../../types";
import { ColorPicker } from "../../ColorPicker";
import { AddRow, Button, Field, IconButton, NumberField, Section, Select, Tabs, TextField, type SelectOption } from "../../kit";
import { EffectDefinitionPicker, effectReferencesEqual } from "./definitionMenu";
import { InspectorScrollArea, Readout } from "../InspectorScrollArea";
import { roundToNanosecond, type AutomationClipChooser, type GuiFocus, type SequenceSelection } from "../shared";
import { TypedParamInput } from "./params/TypedParamInput";
import { sequenceComposition } from "./composition";
import { activeMarkCollection, defaultMarkColor } from "./marks";
import { BeatDetectionDialog } from "./BeatDetectionDialog";
import { MarkSubdivision } from "./MarkSubdivision";
import { MIN_CLIP_DURATION_SECONDS, clipIdsOfKind, clipRefs, selectedClipId, selectionCompatibleWithFocusedItem, selectionCount } from "./sequenceSelection";
import { useSequenceClipDetails } from "./sequenceClipDetails";
import { targetsEqual } from "./sequenceTargets";
import { detachedAutomationTargetLabel } from "./sequenceAutomationLayout";
import { LayerList } from "./sequenceLayers";
import { requestOpenLayerGraph } from "../../uiEvents";

type SequenceInspectorTab = "clip" | "layers" | "marks";

type SelectedMarkEntry = {
  ref: SequenceMarkRef;
  collection: SequenceMarkCollection;
  timeSeconds: number;
  label: string | null;
};

const SEQUENCE_INSPECTOR_TABS: { value: SequenceInspectorTab; label: string }[] = [
  { value: "clip", label: "Clip" },
  { value: "layers", label: "Layers" },
  { value: "marks", label: "Marks" }
];

/** The select value shown while the selected clips differ; never a real layer id or scope. */
const MIXED = "";
const SCOPE_OPTIONS: SelectOption<SequenceClipScope>[] = [
  { value: "perFixture", label: "Per fixture" },
  { value: "wholeTarget", label: "Whole target" }
];
/** The step a clip or mark time scrubs by, in seconds. */
const TIME_STEP_SECONDS = 0.01;

function supportsAutomation(document: SequenceGuiDocument, details: Map<number, SequenceClipDetails> | null, target: SequenceAutomationTarget) {
  if (target.type === "clipParam") {
    return details?.get(target.clipId)?.params
      .find((param) => param.name === target.param)?.supportsAutomation === true;
  }
  const node = document.compositionGraph.nodes.find((node) => node.id === target.nodeId);
  return node?.kind.type === "operator" && node.kind.params
    .find((param) => param.name === target.param)?.supportsAutomation === true;
}

function layerOptions(document: SequenceGuiDocument): SelectOption<string>[] {
  return document.layers.map((layer) => ({ value: String(layer.id), label: layer.name }));
}

function TimeField({ name, value, min, mixed = false, onCommit }: { name: string; value: number; min: number; mixed?: boolean; onCommit: (seconds: number) => void }) {
  return <Field label={name}>
    <NumberField
      name={name}
      value={value}
      min={min}
      step={TIME_STEP_SECONDS}
      unit="s"
      {...(mixed ? { format: () => "Multiple" } : {})}
      onCommit={(seconds) => { onCommit(Math.max(min, roundToNanosecond(seconds))); }}
    />
  </Field>;
}

export function SequenceInspector({
  document,
  selected,
  setSelected,
  sequenceSelection,
  setSequenceSelection,
  automationClipChooser,
  setAutomationClipChooser,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  document: SequenceGuiDocument;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  const [activeTab, setActiveTab] = useState<SequenceInspectorTab>("clip");
  const footer = <Tabs name="Sequence inspector sections" value={activeTab} options={SEQUENCE_INSPECTOR_TABS} onChange={setActiveTab} />;

  return (
    <InspectorScrollArea footer={footer}>
      {activeTab === "clip" && (
        <ClipInspectorPanel
          document={document}
          selected={selected}
          setSelected={setSelected}
          sequenceSelection={sequenceSelection}
          automationClipChooser={automationClipChooser}
          setAutomationClipChooser={setAutomationClipChooser}
        />
      )}
      {activeTab === "layers" && <LayerInspectorPanel document={document} />}
      {activeTab === "marks" && (
        <MarkInspectorPanel
          document={document}
          selected={selected}
          setSelected={setSelected}
          sequenceSelection={sequenceSelection}
          setSequenceSelection={setSequenceSelection}
          activeMarkCollectionKey={activeMarkCollectionKey}
          setActiveMarkCollectionKey={setActiveMarkCollectionKey}
          visibleMarkCollectionKeys={visibleMarkCollectionKeys}
          setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
        />
      )}
    </InspectorScrollArea>
  );
}

function DeleteButton({ onDelete }: { onDelete: () => void }) {
  return <Button variant="danger" icon={Trash2} tip="Delete" command="edit.delete" onClick={onDelete}>Delete</Button>;
}

function SelectedClipsInspector({
  document,
  clipIds,
  onDelete
}: {
  document: SequenceGuiDocument;
  clipIds: number[];
  onDelete: () => void;
}) {

  const clips = clipIds
    .map((id) => document.clips.find((clip) => clip.id === id))
    .filter((clip): clip is SequenceClip => clip !== undefined);
  const details = useSequenceClipDetails(document, clips.map((clip) => clip.id));
  const first = clips[0];
  if (first === undefined) return <h2>Clips</h2>;

  const selectedClipIds = clips.map((clip) => clip.id);
  const layerId = commonClipValue(clips, (clip) => clip.layerId);
  const scope = commonClipValue(clips, (clip) => clip.scope);
  const startSeconds = commonClipValue(clips, (clip) => clip.startSeconds);
  const durationSeconds = commonClipValue(clips, (clip) => clip.durationSeconds);
  const commonParams = details === null ? [] : commonEditableClipParams(clips.map((clip) => details.get(clip.id)).filter((clip) => clip !== undefined));
  const applyEdit = (edit: SequenceClipCommonEdit) =>
    runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "editClips",
        clips: clipRefs("clip", selectedClipIds),
        edit
      } })
    );
  const mixedOption = { value: MIXED, label: "Multiple", disabled: true } as const;

  return (
    <>
      <h2>Clips</h2>
      <div className="inspector-readout-grid">
        <Readout label="Selected" value={String(clips.length)} />
      </div>
      <Field label="Layer">
        <Select
          name="Layer"
          value={layerId === null ? MIXED : String(layerId)}
          options={layerId === null ? [mixedOption, ...layerOptions(document)] : layerOptions(document)}
          onChange={(value) => void applyEdit({ type: "layer", layerId: Number(value) })}
        />
      </Field>
      <Field label="Scope">
        <Select<SequenceClipScope | typeof MIXED>
          name="Scope"
          value={scope ?? MIXED}
          options={scope === null ? [mixedOption, ...SCOPE_OPTIONS] : SCOPE_OPTIONS}
          onChange={(value) => { if (value !== MIXED) void applyEdit({ type: "scope", scope: value }); }}
        />
      </Field>
      <TimeField name="Start" min={0} value={startSeconds ?? first.startSeconds} mixed={startSeconds === null}
        onCommit={(value) => void applyEdit({ type: "start", startSeconds: value })} />
      <TimeField name="Duration" min={MIN_CLIP_DURATION_SECONDS} value={durationSeconds ?? first.durationSeconds} mixed={durationSeconds === null}
        onCommit={(value) => void applyEdit({ type: "duration", durationSeconds: value })} />
      {commonParams.length > 0 && (
        <Section title="Shared parameters">
          {commonParams.map(({ param, mixed }) => (
            <SharedClipParamInput
              key={`${selectedClipIds.join(",")}:${param.name}`}
              param={param}
              mixed={mixed}
              commitParam={(name, value) => applyEdit({ type: "param", name, value }).then(() => undefined)}
              document={document}
            />
          ))}
        </Section>
      )}
      <DeleteButton onDelete={onDelete} />
    </>
  );
}

function commonEditableClipParams(clips: SequenceClipDetails[]): { param: SequenceClipParam; mixed: boolean }[] {
  const first = clips[0];
  if (first === undefined) return [];
  return first.params.flatMap((param) => {
    if (!param.editable) return [];
    const matches = clips.slice(1).map((clip) => clip.params.find((candidate) =>
      candidate.name === param.name && candidate.kind === param.kind && candidate.editable
    ));
    if (matches.some((match) => match === undefined)) return [];
    const options = param.kind === "enum"
      ? param.options.filter((option) => matches.every((match) => match !== undefined && match.options.includes(option)))
      : param.options;
    if (param.kind === "enum" && options.length === 0) return [];
    const mixed = matches.some((match) => JSON.stringify(match?.value) !== JSON.stringify(param.value));
    const displayParam = param.value.type === "enum" && !options.includes(param.value.value)
      ? { ...param, options, value: { type: "enum" as const, value: options[0] ?? param.value.value } }
      : { ...param, options };
    return [{ param: displayParam, mixed }];
  });
}

function SharedClipParamInput({
  param,
  mixed,
  commitParam,
  document
}: {
  param: SequenceClipParam;
  mixed: boolean;
  commitParam: (name: string, value: SequenceClipParam["value"]) => Promise<void>;
  document: SequenceGuiDocument;
}) {
  const [editingMixed, setEditingMixed] = useState(false);
  if (mixed && !editingMixed) {
    return (
      <Field label={param.name} tip={param.description ?? undefined}>
        <Button tip="Edit every selected clip's value" onClick={() => { setEditingMixed(true); }}>Multiple</Button>
      </Field>
    );
  }
  return (
    <>
      <TypedParamInput
        param={param}
        commitParam={commitParam}
        curveLibrary={document.curveLibrary}
        gradientLibrary={document.gradientLibrary}
        markCollections={document.markCollections}
      />
      {mixed && <IconButton icon={CheckCheck} label="Apply shown value to all" onClick={() => { void commitParam(param.name, param.value); }} />}
    </>
  );
}

function commonClipValue<T>(clips: SequenceClip[], read: (clip: SequenceClip) => T): T | null {
  const first = clips[0];
  if (first === undefined) return null;
  const value = read(first);
  return clips.every((clip) => Object.is(read(clip), value)) ? value : null;
}

function ClipInspectorPanel({
  document,
  selected,
  setSelected,
  sequenceSelection,
  automationClipChooser,
  setAutomationClipChooser
}: {
  document: SequenceGuiDocument;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  sequenceSelection: SequenceSelection;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
}) {
  const inspectedAutomationClip = selected?.type === "automationClip"
    ? document.automationClips.find((automationClip) => automationClip.id === selected.id) ?? null
    : null;
  const inspectedClipIds = inspectedAutomationClip !== null
    ? inspectedAutomationClip.detachedBindings.flatMap((binding) => binding.target.type === "clipParam" ? [binding.target.clipId] : [])
    : [selectedClipId(selected)].filter((id) => id !== null);
  const details = useSequenceClipDetails(document, inspectedClipIds);

  if (sequenceSelection !== null && selectionCount(sequenceSelection) > 1 && selectionCompatibleWithFocusedItem(sequenceSelection, selected)) {
    if (sequenceSelection.type !== "clips") return <h2>Clip</h2>;
    const deleteSelection = () => {
      void runGuiEditCommand((request) =>
        commands.applyGuiEdit(request, { type: "sequence", edit: { type: "deleteSelection", selection: sequenceSelection } })
      ).then(() => {
        setSelected(null);
      });
    };
    if (sequenceSelection.clips.every((clip) => clip.type === "clip")) {
      return <SelectedClipsInspector document={document} clipIds={clipIdsOfKind(sequenceSelection.clips, "clip")} onDelete={deleteSelection} />;
    }
    return (
      <>
        <h2>Clips</h2>
        <div className="inspector-readout-grid">
          <Readout label="Selected" value={String(selectionCount(sequenceSelection))} />
        </div>
        <DeleteButton onDelete={deleteSelection} />
      </>
    );
  }

  const automationClip = selected?.type === "automationClip"
    ? document.automationClips.find((automationClip) => automationClip.id === selected.id) ?? null
    : null;
  if (automationClip !== null) {
    return (
      <>
        <h2>Automation Clip</h2>
        <div className="inspector-readout-grid">
          <Readout label="Start" value={`${automationClip.startSeconds.toFixed(3)}s`} />
          <Readout label="Duration" value={`${automationClip.durationSeconds.toFixed(3)}s`} />
          <Readout label="Bindings" value={String(automationClip.bindings.length)} />
          <Readout label="Detached" value={String(automationClip.detachedBindings.length)} />
        </div>
        {automationClip.detachedBindings.map((binding, index) => (
          <Field key={`${binding.target.type}-${index}`} label={detachedAutomationTargetLabel(binding.target)} tip="Detached">
            <IconButton
              icon={Link2}
              label="Rebind"
              disabled={!supportsAutomation(document, details, binding.target)}
              onClick={() => void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "sequence", edit: {
                type: "rebindDetachedAutomation",
                automationClipId: automationClip.id,
                detachedIndex: index,
                target: binding.target
              } }))}
            />
            <IconButton
              icon={Trash2}
              label="Discard provenance"
              danger
              onClick={() => void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "sequence", edit: {
                type: "discardDetachedAutomation",
                automationClipId: automationClip.id,
                detachedIndex: index
              } }))}
            />
          </Field>
        ))}
        <DeleteButton onDelete={() =>
          void runGuiEditCommand((request) =>
            commands.applyGuiEdit(request, { type: "sequence", edit: {
              type: "deleteSelection",
              selection: { type: "clips", clips: [{ type: "automation", id: automationClip.id }] }
            } })
          ).then(() => {
            setSelected(null);
          })
        } />
      </>
    );
  }
  const id = selectedClipId(selected);
  const clip = document.clips.find((candidate) => candidate.id === id);

  if (clip === undefined) return <h2>Clip</h2>;

  const clipDetails = details?.get(clip.id);
  const currentDefinition = clipDetails === undefined ? undefined : document.effectDefinitions.find((definition) =>
    effectReferencesEqual(definition.effect, clipDetails.effectReference)
  );
  const editClip = (edit: SequenceClipCommonEdit) =>
    runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: { type: "editClips", clips: [{ type: "clip", id: clip.id }], edit } })
    );
  const item = { type: "clip", id: clip.id } as const;

  return (
    <>
      <h2>Clip</h2>
      <Field label="Name">
        <TextField
          name="Name"
          value={clip.name}
          onCommit={(value) => {
            const name = value.trim();
            if (name !== "" && name !== clip.name) void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "rename", item, name }));
          }}
        />
      </Field>
      <Field label="Description">
        <TextField
          name="Description"
          multiline
          value={clip.description ?? ""}
          onCommit={(value) => {
            const description = value.trim() === "" ? null : value.trim();
            if (description !== clip.description) void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setDescription", item, description }));
          }}
        />
      </Field>
      <Field label="Effect" tip={currentDefinition?.description ?? undefined}>
        <EffectDefinitionPicker
          definitions={document.effectDefinitions}
          label={currentDefinition?.name ?? clip.effect}
          onSelect={(definition) =>
            void runGuiEditCommand((request) =>
              commands.applyGuiEdit(request, { type: "sequence", edit: {
                type: "changeEffectDefinition",
                initialColor: THEME_COLORS.white,
                id: clip.id,
                effect: definition.effect
              } })
            )
          }
        />
      </Field>
      <Field label="Layer">
        <Select
          name="Layer"
          value={String(clip.layerId)}
          options={layerOptions(document)}
          onChange={(value) => void editClip({ type: "layer", layerId: Number(value) })}
        />
      </Field>
      <TimeField name="Start" min={0} value={clip.startSeconds}
        onCommit={(startSeconds) => void editClip({ type: "start", startSeconds })} />
      <TimeField name="Duration" min={MIN_CLIP_DURATION_SECONDS} value={clip.durationSeconds}
        onCommit={(durationSeconds) => void editClip({ type: "duration", durationSeconds })} />
      <Field label="Scope">
        <Select name="Scope" value={clip.scope} options={SCOPE_OPTIONS} onChange={(scope) => void editClip({ type: "scope", scope })} />
      </Field>
      {clipDetails !== undefined && clipDetails.params.length > 0 && (
        <Section title="Parameters">
          {clipDetails.params.map((param) => (
            <TypedParamInput
              key={`${clip.id}:${param.name}`}
              param={param}
              commitParam={(name, value) =>
                editClip({ type: "param", name, value }).then(() => undefined)
              }
              curveLibrary={document.curveLibrary}
              gradientLibrary={document.gradientLibrary}
              markCollections={document.markCollections}
              automation={{
                target: { type: "clipParam", clipId: clip.id, param: param.name },
                automationClips: document.automationClips,
                canCreateAutomationClip: document.lanes.some((lane) => targetsEqual(lane.target, clip.target)),
                automationClipChooser,
                setAutomationClipChooser
              }}
            />
          ))}
        </Section>
      )}
    </>
  );
}

function LayerInspectorPanel({ document }: { document: SequenceGuiDocument }) {
  const composition = useMemo(() => sequenceComposition(document, null), [document]);
  return <>
    <h2>Layers</h2>
    <Button icon={Workflow} tip="Open layer graph" command="view.openLayerGraph" onClick={requestOpenLayerGraph}>Layer graph</Button>
    <LayerList composition={composition} />
  </>;
}

function MarkInspectorPanel({
  document,
  selected,
  setSelected,
  sequenceSelection,
  setSequenceSelection,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  document: SequenceGuiDocument;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {

  const selectedMark = selected?.type === "mark" ? { collectionKey: selected.collectionKey, index: selected.index } : null;
  const activeCollection = activeMarkCollection(document.markCollections, activeMarkCollectionKey);
  const selectedMarks = selectedMarkEntries(document, selected, sequenceSelection);
  const collectionOptions = document.markCollections.map((collection) => ({ value: collection.key, label: collection.key }));

  const createCollection = () => {
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "createMarkCollections",
        collections: [{ name: "Marks", color: defaultMarkColor(document.markCollections.length), marksSeconds: [] }]
      } })
    ).then(({ created }) => {
      setActiveMarkCollectionKey(created[0] ?? null);
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, ...created]));
    });
  };

  /** Renames, carrying the active and visible state to the new key; a rejected name reverts. */
  const renameCollection = (key: string, value: string) => {
    const name = value.trim();
    if (name === "" || name === key) return;
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "rename", item: { type: "markCollection", key }, name })
    ).then((result) => {
      const renamed = result.created[0] ?? key;
      if (activeMarkCollectionKey === key) setActiveMarkCollectionKey(renamed);
      if (visibleMarkCollectionKeys.has(key)) {
        setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys].map((visible) => (visible === key ? renamed : visible))));
      }
    });
  };

  const deleteCollection = (collection: SequenceMarkCollection) => {
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "delete", item: { type: "markCollection", key: collection.key } })
    ).then(() => {
      if (selectedMark?.collectionKey === collection.key) setSelected(null);
      if (activeCollection?.key === collection.key) {
        setActiveMarkCollectionKey(document.markCollections.find((candidate) => candidate.key !== collection.key)?.key ?? null);
      }
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys].filter((key) => key !== collection.key)));
    });
  };

  const setSelectedMarkRefs = (refs: SequenceMarkRef[]) => {
    const validRefs = refs.filter((ref) => markEntry(document, ref) !== null);
    if (validRefs.length === 0) {
      setSelected(null);
      setSequenceSelection(null);
      return;
    }
    const firstRef = validRefs[0];
    if (firstRef === undefined) return;
    setSelected({ type: "mark", collectionKey: firstRef.collectionKey, index: firstRef.index });
    setSequenceSelection({ type: "marks", marks: validRefs });
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
    ).then(() => {
      setSelectedMarkRefs(nextRefs);
    });
  };

  const reassignSelectedMark = (entry: SelectedMarkEntry, targetCollectionKey: string) => {
    if (targetCollectionKey === entry.ref.collectionKey) return;
    const targetCollection = document.markCollections.find((collection) => collection.key === targetCollectionKey);
    if (targetCollection === undefined) return;
    const nextRefs = selectedRefsAfterReassign(selectedMarks.map((mark) => mark.ref), entry.ref, targetCollection, entry.timeSeconds);
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "reassignMarkCollection",
        collectionKey: entry.ref.collectionKey,
        index: entry.ref.index,
        targetCollectionKey
      } })
    ).then(() => {
      setSelectedMarkRefs(nextRefs);
    });
  };

  const labelSelectedMark = (entry: SelectedMarkEntry, label: string) => {
    const next = label.trim() === "" ? null : label.trim();
    if (next === entry.label) return;
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "setMarkLabel",
        collectionKey: entry.ref.collectionKey,
        index: entry.ref.index,
        label: next
      } })
    );
  };

  const deleteSelectedMark = (entry: SelectedMarkEntry) => {
    const nextRefs = selectedRefsAfterDelete(selectedMarks.map((mark) => mark.ref), entry.ref);
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "deleteSelection",
        selection: { type: "marks", marks: [entry.ref] }
      } })
    ).then(() => {
      setSelectedMarkRefs(nextRefs);
    });
  };

  return (
    <>
      <h2>Marks</h2>
      <Section title="Collections" actions={<>
        <IconButton icon={Plus} label="Add collection" onClick={createCollection} />
        <BeatDetectionDialog
          document={document}
          setActiveMarkCollectionKey={setActiveMarkCollectionKey}
          visibleMarkCollectionKeys={visibleMarkCollectionKeys}
          setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
        />
      </>}>
        {document.markCollections.length === 0 && <AddRow label="Add collection" onAdd={createCollection} />}
        {document.markCollections.length > 0 && (
          <div className="mark-collection-edit-list">
            {document.markCollections.map((collection) => {
              const active = activeCollection?.key === collection.key;
              const visible = visibleMarkCollectionKeys.has(collection.key);
              return <div key={collection.key} className="mark-collection-edit-row">
                <IconButton
                  icon={active ? CircleDot : Circle}
                  pressed={active}
                  label={`Use ${collection.key} for new marks`}
                  onClick={() => { setActiveMarkCollectionKey(collection.key); }}
                />
                <IconButton
                  icon={visible ? Eye : EyeOff}
                  pressed={visible}
                  label={visible ? `Hide ${collection.key}` : `Show ${collection.key}`}
                  onClick={() => {
                    const next = new Set(visibleMarkCollectionKeys);
                    if (visible) next.delete(collection.key);
                    else next.add(collection.key);
                    setVisibleMarkCollectionKeys(next);
                  }}
                />
                <ColorPicker
                  value={collection.color}
                  label={`${collection.key} color`}
                  commit={(color) =>
                    runGuiEditCommand((request) =>
                      commands.applyGuiEdit(request, { type: "setColor", item: { type: "markCollection", key: collection.key }, color })
                    ).then(() => undefined)
                  }
                />
                <TextField name={`${collection.key} name`} value={collection.key} onCommit={(name) => { renameCollection(collection.key, name); }} />
                <IconButton icon={Trash2} label={`Delete ${collection.key}`} danger onClick={() => { deleteCollection(collection); }} />
              </div>;
            })}
          </div>
        )}
        {activeCollection !== null && (
          <Field label="Description">
            <TextField
              name={`${activeCollection.key} description`}
              multiline
              value={activeCollection.description ?? ""}
              onCommit={(value) => {
                const description = value.trim() === "" ? null : value.trim();
                if (description === activeCollection.description) return;
                void runGuiEditCommand((request) =>
                  commands.applyGuiEdit(request, { type: "setDescription", item: { type: "markCollection", key: activeCollection.key }, description })
                );
              }}
            />
          </Field>
        )}
      </Section>
        {selectedMarks.length >= 2 && (
          <MarkSubdivision
            document={document}
            selectedSeconds={selectedMarks.map((entry) => entry.timeSeconds)}
            visibleMarkCollectionKeys={visibleMarkCollectionKeys}
            setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
            clearMarkSelection={() => {
              setSelected(null);
              setSequenceSelection(null);
            }}
          />
        )}
      {selectedMarks.length > 0 && (
        <Section title={`Selected · ${selectedMarks.length}`}>
          <div className="mark-selected-list">
            {selectedMarks.map((entry, index) => (
              <div key={`${entry.ref.collectionKey}:${entry.ref.index}:${index}`} className="mark-selected-row">
                <Select
                  name="Mark collection"
                  value={entry.ref.collectionKey}
                  options={collectionOptions}
                  onChange={(key) => { reassignSelectedMark(entry, key); }}
                />
                <NumberField
                  name="Mark time"
                  value={entry.timeSeconds}
                  min={0}
                  step={TIME_STEP_SECONDS}
                  unit="s"
                  onCommit={(timeSeconds) => { moveSelectedMark(entry, timeSeconds); }}
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

function markIndexAfterInsert(collection: SequenceMarkCollection, timeSeconds: number) {
  return collection.marksSeconds.filter((markTimeSeconds) => markTimeSeconds <= timeSeconds).length;
}

function selectedMarkEntries(
  document: SequenceGuiDocument,
  selected: GuiFocus,
  sequenceSelection: SequenceSelection
): SelectedMarkEntry[] {
  const refs =
    sequenceSelection?.type === "marks" && sequenceSelection.marks.length > 0
      ? sequenceSelection.marks
      : selected?.type === "mark"
        ? [{ collectionKey: selected.collectionKey, index: selected.index }]
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
  const collection = document.markCollections.find((candidate) => candidate.key === ref.collectionKey);
  const timeSeconds = collection?.marksSeconds[ref.index];
  if (collection === undefined || timeSeconds === undefined) return null;
  return { ref, collection, timeSeconds, label: collection.markLabels[ref.index] ?? null };
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
    if (ref.collectionKey !== movedRef.collectionKey) return ref;
    const nextIndex = sorted.findIndex((mark) => mark.markIndex === ref.index);
    return nextIndex < 0 ? ref : { collectionKey: ref.collectionKey, index: nextIndex };
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
    if (sameMarkRef(ref, movedRef)) return { collectionKey: targetCollection.key, index: targetIndex };
    if (ref.collectionKey === movedRef.collectionKey && ref.index > movedRef.index) {
      return { collectionKey: ref.collectionKey, index: ref.index - 1 };
    }
    if (ref.collectionKey === targetCollection.key && ref.index >= targetIndex) {
      return { collectionKey: ref.collectionKey, index: ref.index + 1 };
    }
    return ref;
  });
}

function selectedRefsAfterDelete(refs: SequenceMarkRef[], deletedRef: SequenceMarkRef) {
  return refs
    .filter((ref) => !sameMarkRef(ref, deletedRef))
    .map((ref) => {
      if (ref.collectionKey === deletedRef.collectionKey && ref.index > deletedRef.index) {
        return { collectionKey: ref.collectionKey, index: ref.index - 1 };
      }
      return ref;
    });
}

function sameMarkRef(left: SequenceMarkRef, right: SequenceMarkRef) {
  return left.collectionKey === right.collectionKey && left.index === right.index;
}

function markRefKey(ref: SequenceMarkRef) {
  return `${ref.collectionKey}:${ref.index}`;
}


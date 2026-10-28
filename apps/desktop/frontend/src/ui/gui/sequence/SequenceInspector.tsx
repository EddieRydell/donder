import { useSequenceEditorHost } from "../../../editor/host";
import { useMemo, useState } from "react";

import { Plus, Trash2 } from "lucide-react";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";

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
} from "../../../editor/types";
import { ColorPicker } from "../../ColorPicker";
import { EffectDefinitionPicker, effectReferencesEqual } from "./definitionMenu";
import { InspectorScrollArea, Readout } from "../InspectorScrollArea";
import { roundToNanosecond, type AutomationClipChooser, type GuiFocus, type SequenceSelection } from "../shared";
import { TypedParamInput } from "./params/TypedParamInput";
import { sequenceComposition } from "./composition";
import { NameField } from "./NameField";
import { DescriptionField } from "../DescriptionField";
import { activeMarkCollection, defaultMarkColor } from "./marks";
import { BeatDetectionDialog } from "./BeatDetectionDialog";
import { MarkSubdivision } from "./MarkSubdivision";
import { clipIdsOfKind, clipRefs, selectedClipId, selectionCompatibleWithFocusedItem, selectionCount } from "./sequenceSelection";
import { useSequenceClipDetails } from "./sequenceClipDetails";
import { targetsEqual } from "./sequenceTargets";
import { detachedAutomationTargetLabel } from "./sequenceAutomationLayout";
import { useConfirmDialog } from "../../ConfirmDialog";
import { LayerList } from "./sequenceLayers";

type SequenceInspectorTab = "clip" | "layers" | "marks";

type SelectedMarkEntry = {
  ref: SequenceMarkRef;
  collection: SequenceMarkCollection;
  timeSeconds: number;
  label: string | null;
};

const SEQUENCE_INSPECTOR_TABS: { id: SequenceInspectorTab; label: string }[] = [
  { id: "clip", label: "Clip" },
  { id: "layers", label: "Layers" },
  { id: "marks", label: "Marks" },
];

function supportsAutomation(document: SequenceGuiDocument, details: Map<number, SequenceClipDetails> | null, target: SequenceAutomationTarget) {
  if (target.type === "clipParam") {
    return details?.get(target.clipId)?.params
      .find((param) => param.name === target.param)?.supportsAutomation === true;
  }
  const node = document.compositionGraph.nodes.find((node) => node.id === target.nodeId);
  return node?.kind.type === "operator" && node.kind.params
    .find((param) => param.name === target.param)?.supportsAutomation === true;
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
  const [tab, setActiveTab] = useState<SequenceInspectorTab>("clip");
  const activeTab = tab;

  const footer = (
    <div className="sequence-inspector-tabs" role="tablist" aria-label="Sequence inspector sections">
      {SEQUENCE_INSPECTOR_TABS.map((tab) => (
        <button
          key={tab.id}
          type="button"
          role="tab"
          aria-selected={activeTab === tab.id}
          className={activeTab === tab.id ? "active" : ""}
          onClick={() => {
            setActiveTab(tab.id);
          }}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );

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

function SelectedClipsInspector({
  document,
  clipIds,
  onDelete
}: {
  document: SequenceGuiDocument;
  clipIds: number[];
  onDelete: () => void;
}) {
  const host = useSequenceEditorHost();
  const { commands, runGuiEditCommand } = host;

  const clips = clipIds
    .map((id) => document.clips.find((clip) => clip.id === id))
    .filter((clip): clip is SequenceClip => clip !== undefined);
  const details = useSequenceClipDetails(document, clips.map((clip) => clip.id));
  if (clips.length === 0) {
    return <><h2>Clips</h2><p>Select a clip on the timeline.</p></>;
  }

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

  return (
    <>
      <h2>Clips</h2>
      <div className="inspector-readout-grid">
        <Readout label="Selected" value={String(clips.length)} />
      </div>
      <div className="clip-inspector-fields">
        <label>
          Layer
          <select
            value={layerId === null ? "" : String(layerId)}
            onChange={(event) => {
              const nextLayerId = Number(event.currentTarget.value);
              if (!Number.isInteger(nextLayerId)) return;
              void applyEdit({ type: "layer", layerId: nextLayerId });
            }}
          >
            {layerId === null && <option value="" disabled>Multiple</option>}
            {document.layers.map((layer) => (
              <option key={layer.id} value={String(layer.id)}>
                {layer.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Scope
          <select
            value={scope ?? ""}
            onChange={(event) => {
              const nextScope = event.currentTarget.value;
              if (nextScope !== "perFixture" && nextScope !== "wholeTarget") return;
              void applyEdit({ type: "scope", scope: nextScope });
            }}
          >
            {scope === null && <option value="" disabled>Multiple</option>}
            <option value="perFixture">Per fixture</option>
            <option value="wholeTarget">Whole target</option>
          </select>
        </label>
        <div className="inspector-inline-row">
          <MultiClipNumberField
            clipIds={selectedClipIds}
            field="start"
            label="Start"
            min={0}
            value={startSeconds}
            onCommit={(value) => void applyEdit({ type: "start", startSeconds: value })}
          />
          <MultiClipNumberField
            clipIds={selectedClipIds}
            field="duration"
            label="Duration"
            min={0.000000001}
            value={durationSeconds}
            onCommit={(value) => void applyEdit({ type: "duration", durationSeconds: value })}
          />
        </div>
      </div>
      {commonParams.length > 0 && (
        <div className="clip-param-section">
          <h3>Shared parameters</h3>
          {commonParams.map(({ param, mixed }, index) => (
            <div
              key={`${selectedClipIds.join(",")}:${param.name}`}
              className={`clip-param-row ${index % 2 === 0 ? "clip-param-row-even" : "clip-param-row-odd"}`}
            >
              <SharedClipParamInput
                param={param}
                mixed={mixed}
                commitParam={(name, value) => applyEdit({ type: "param", name, value }).then(() => undefined)}
                document={document}
              />
            </div>
          ))}
        </div>
      )}
      <button type="button" onClick={onDelete}>Delete</button>
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
      <div className="clip-param-group">
        <div className="clip-param-name">{param.name}</div>
        <button type="button" className="neutral-button" onClick={() => { setEditingMixed(true); }}>
          Multiple values · Edit all
        </button>
      </div>
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
      {mixed && (
        <button type="button" className="neutral-button" onClick={() => { void commitParam(param.name, param.value); }}>
          Apply shown value to all
        </button>
      )}
    </>
  );
}

function MultiClipNumberField({
  clipIds,
  field,
  label,
  min,
  value,
  onCommit
}: {
  clipIds: number[];
  field: "start" | "duration";
  label: string;
  min: number;
  value: number | null;
  onCommit: (value: number) => void;
}) {
  return (
    <label>
      {label}
      <input
        key={`${clipIds.join(",")}:${field}:${value ?? "multiple"}`}
        type="number"
        min={min}
        step="any"
        placeholder={value === null ? "Multiple" : undefined}
        defaultValue={value ?? ""}
        onBlur={(event) => {
          const rawValue = event.currentTarget.value;
          if (rawValue === "") return;
          const nextValue = Number(rawValue);
          if (!Number.isFinite(nextValue)) return;
          const normalizedValue = roundToNanosecond(Math.max(min, nextValue));
          if (value === normalizedValue) return;
          onCommit(normalizedValue);
        }}
      />
    </label>
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
  const host = useSequenceEditorHost();
  const { commands, runGuiEditCommand } = host;
  const inspectedAutomationClip = selected?.type === "automationClip"
    ? document.automationClips.find((automationClip) => automationClip.id === selected.id) ?? null
    : null;
  const inspectedClipIds = inspectedAutomationClip !== null
    ? inspectedAutomationClip.detachedBindings.flatMap((binding) => binding.target.type === "clipParam" ? [binding.target.clipId] : [])
    : [selectedClipId(selected)].filter((id) => id !== null);
  const details = useSequenceClipDetails(document, inspectedClipIds);

  if (sequenceSelection !== null && selectionCount(sequenceSelection) > 1 && selectionCompatibleWithFocusedItem(sequenceSelection, selected)) {
    if (sequenceSelection.type !== "clips") {
      return (
        <>
          <h2>Clip</h2>
          <p>Select a clip on the timeline.</p>
        </>
      );
    }
    if (sequenceSelection.clips.every((clip) => clip.type === "clip")) {
      return (
        <SelectedClipsInspector
          document={document}
          clipIds={clipIdsOfKind(sequenceSelection.clips, "clip")}
          onDelete={() => {
            void runGuiEditCommand((request) =>
              commands.applyGuiEdit(request, { type: "sequence", edit: { type: "deleteSelection", selection: sequenceSelection } })
            ).then(() => {
              setSelected(null);
            });
          }}
        />
      );
    }
    return (
      <>
        <h2>Clips</h2>
        <div className="inspector-readout-grid">
          <Readout label="Selected" value={String(selectionCount(sequenceSelection))} />
        </div>
        <button
          type="button"
          onClick={() =>
            void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "sequence", edit: { type: "deleteSelection", selection: sequenceSelection } })).then(() => {
              setSelected(null);
            })
          }
        >
          Delete
        </button>
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
          <div key={`${binding.target.type}-${index}`}>
            <span className="status-warning">Detached</span>
            <p>{detachedAutomationTargetLabel(binding.target)}</p>
            <div className="clip-param-actions">
              <button
                type="button"
                disabled={!supportsAutomation(document, details, binding.target)}
                onClick={() => void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "sequence", edit: {
                  type: "rebindDetachedAutomation",
                  automationClipId: automationClip.id,
                  detachedIndex: index,
                  target: binding.target
                } }))}
              >
                Rebind
              </button>
              <button
                type="button"
                className="danger"
                onClick={() => void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "sequence", edit: {
                  type: "discardDetachedAutomation",
                  automationClipId: automationClip.id,
                  detachedIndex: index
                } }))}
              >
                Discard provenance
              </button>
            </div>
          </div>
        ))}
        <button
          type="button"
          onClick={() =>
            void runGuiEditCommand((request) =>
              commands.applyGuiEdit(request, { type: "sequence", edit: {
                type: "deleteSelection",
                selection: { type: "clips", clips: [{ type: "automation", id: automationClip.id }] }
              } })
            ).then(() => {
              setSelected(null);
            })
          }
        >
          <Trash2 size={THEME_METRICS.iconSizeExtraSmall} /> Delete automation clip
        </button>
      </>
    );
  }
  const id = selectedClipId(selected);
  const clip = document.clips.find((candidate) => candidate.id === id);


  if (clip === undefined) {
    return (
      <>
        <h2>Clip</h2>
        <p>Select a clip on the timeline.</p>
      </>
    );
  }

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
      <div className="clip-inspector-fields">
        <NameField
          name={clip.name}
          label="Name"
          commit={(name) =>
            runGuiEditCommand((request) =>
              commands.applyGuiEdit(request, { type: "rename", item, name })
            )
          }
        />
        <DescriptionField
          description={clip.description}
          onCommit={(description) =>
            runGuiEditCommand((request) =>
              commands.applyGuiEdit(request, { type: "setDescription", item, description })
            )
          }
        />
        <label>
          Layer
          <select
            value={String(clip.layerId)}
            onChange={(event) =>
              void editClip({ type: "layer", layerId: Number(event.currentTarget.value) })
            }
          >
            {document.layers.map((layer) => (
              <option key={layer.id} value={String(layer.id)}>
                {layer.name}
              </option>
            ))}
          </select>
        </label>
        <div className="inspector-inline-row">
          <label>
            Start
            <input
              key={`${clip.id}:start:${clip.startSeconds}`}
              type="number"
              min={0}
              step="any"
              defaultValue={clip.startSeconds}
              onBlur={(event) => {
                const nextStartSeconds = Number(event.currentTarget.value);
                if (!Number.isFinite(nextStartSeconds) || roundToNanosecond(nextStartSeconds) === clip.startSeconds) return;
                void editClip({ type: "start", startSeconds: Math.max(0, roundToNanosecond(nextStartSeconds)) });
              }}
            />
          </label>
          <label>
            Duration
            <input
              key={`${clip.id}:duration:${clip.durationSeconds}`}
              type="number"
              min={0.000000001}
              step="any"
              defaultValue={clip.durationSeconds}
              onBlur={(event) => {
                const nextDurationSeconds = Number(event.currentTarget.value);
                if (!Number.isFinite(nextDurationSeconds) || roundToNanosecond(nextDurationSeconds) === clip.durationSeconds) return;
                void editClip({ type: "duration", durationSeconds: Math.max(0.000000001, roundToNanosecond(nextDurationSeconds)) });
              }}
            />
          </label>
        </div>
        <label>
          Effect type
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
        </label>
        {currentDefinition !== undefined && currentDefinition.description !== null && (
          <p className="clip-param-description">{currentDefinition.description}</p>
        )}
      </div>
      <label>
        Scope
        <select
          value={clip.scope}
          onChange={(event) =>
            void editClip({ type: "scope", scope: event.currentTarget.value as SequenceClipScope })
          }
        >
          <option value="perFixture">Per fixture</option>
          <option value="wholeTarget">Whole target</option>
        </select>
      </label>
      {clipDetails !== undefined && clipDetails.params.length > 0 && (
        <>
          <div className="inspector-section-divider" />
          <div className="clip-param-section">
            <h3>Parameters</h3>
            {clipDetails.params.map((param, index) => (
              <div
                key={`${clip.id}:${param.name}`}
                className={`clip-param-row ${index % 2 === 0 ? "clip-param-row-even" : "clip-param-row-odd"}`}
              >
                <TypedParamInput
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
              </div>
            ))}
          </div>
        </>
      )}
    </>
  );
}

function LayerInspectorPanel({ document }: { document: SequenceGuiDocument }) {
  const host = useSequenceEditorHost();
  const composition = useMemo(() => sequenceComposition(host, document, null), [host, document]);
  return <><h2>Layers</h2><LayerList composition={composition} /></>;
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
  const host = useSequenceEditorHost();
  const { commands, runGuiEditCommand } = host;
  const { confirm, dialog: confirmDialog } = useConfirmDialog();

  const selectedMark = selected?.type === "mark" ? { collectionKey: selected.collectionKey, index: selected.index } : null;
  const activeCollection = activeMarkCollection(document.markCollections, activeMarkCollectionKey);
  const selectedMarks = selectedMarkEntries(document, selected, sequenceSelection);

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

  /** Renames on commit, carrying the active and visible state to the new key; a rejected name reverts. */
  const renameCollection = (key: string, input: HTMLInputElement) => {
    const name = input.value.trim();
    if (name === "" || name === key) {
      input.value = key;
      return;
    }
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "rename", item: { type: "markCollection", key }, name })
    ).then(
      (result) => {
        const renamed = result.created[0] ?? key;
        if (activeMarkCollectionKey === key) setActiveMarkCollectionKey(renamed);
        if (visibleMarkCollectionKeys.has(key)) {
          setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys].map((visible) => (visible === key ? renamed : visible))));
        }
      },
      () => {
        input.value = key;
      }
    );
  };

  const deleteCollection = async (collection: SequenceMarkCollection) => {
    if (collection.marksSeconds.length > 0 && !await confirm({
      title: `Delete ${collection.key}?`,
      description: `Its ${collection.marksSeconds.length} marks are deleted too. Clips using it will have no marks.`,
      confirmLabel: "Delete",
      danger: true
    })) return;
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
      <label>
        Active collection
        <select
          value={activeCollection?.key ?? ""}
          onChange={(event) => {
            setActiveMarkCollectionKey(event.currentTarget.value || null);
          }}
        >
          {document.markCollections.map((collection) => (
            <option key={collection.key} value={collection.key}>{collection.key}</option>
          ))}
        </select>
      </label>
      {activeCollection !== null && (
        <DescriptionField
          label={`${activeCollection.key} description`}
          description={activeCollection.description}
          onCommit={(description) =>
            runGuiEditCommand((request) =>
              commands.applyGuiEdit(request, {
                type: "setDescription",
                item: { type: "markCollection", key: activeCollection.key },
                description
              })
            )
          }
        />
      )}
      <div className="mark-section">
        <h3>Collections</h3>
        <button type="button" className="neutral-button icon-text-button" onClick={createCollection}>
          <Plus size={THEME_METRICS.iconSizeSmall} />
          Add collection
        </button>
        <BeatDetectionDialog
          document={document}
          setActiveMarkCollectionKey={setActiveMarkCollectionKey}
          visibleMarkCollectionKeys={visibleMarkCollectionKeys}
          setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
        />
        {document.markCollections.length > 0 && (
          <div className="mark-collection-edit-list">
            {document.markCollections.map((collection) => (
              <div key={collection.key} className="mark-collection-edit-row">
                <input
                  type="radio"
                  name="active-mark-collection"
                  checked={activeCollection?.key === collection.key}
                  aria-label={`Use ${collection.key} for new marks`}
                  onChange={() => {
                    setActiveMarkCollectionKey(collection.key);
                  }}
                />
                <input
                  type="checkbox"
                  checked={visibleMarkCollectionKeys.has(collection.key)}
                  aria-label={`Show ${collection.key}`}
                  onChange={(event) => {
                    const next = new Set(visibleMarkCollectionKeys);
                    if (event.currentTarget.checked) {
                      next.add(collection.key);
                    } else {
                      next.delete(collection.key);
                    }
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
                <input
                  key={collection.key}
                  type="text"
                  defaultValue={collection.key}
                  aria-label={`${collection.key} name`}
                  onFocus={() => {
                    setActiveMarkCollectionKey(collection.key);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") event.currentTarget.blur();
                    if (event.key === "Escape") {
                      event.stopPropagation();
                      event.currentTarget.value = collection.key;
                      event.currentTarget.blur();
                    }
                  }}
                  onBlur={(event) => {
                    renameCollection(collection.key, event.currentTarget);
                  }}
                />
                <button
                  type="button"
                  className="icon-button danger-icon-button"
                  title="Delete collection"
                  aria-label={`Delete ${collection.key}`}
                  onClick={() => {
                    void deleteCollection(collection);
                  }}
                >
                  <Trash2 size={THEME_METRICS.iconSizeSmall} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
      {selectedMarks.length > 0 ? (
        <div className="mark-section">
          <h3>{selectedMarks.length === 1 ? "Selected Mark" : "Selected Marks"}</h3>
          <div className="mark-selected-list">
            {selectedMarks.map((entry, index) => (
              <div key={`${entry.ref.collectionKey}:${entry.ref.index}:${index}`} className="mark-selected-row">
                <select
                  value={entry.ref.collectionKey}
                  aria-label="Selected mark collection"
                  onChange={(event) => {
                    reassignSelectedMark(entry, event.currentTarget.value);
                  }}
                >
                  {document.markCollections.map((collection) => (
                    <option key={collection.key} value={collection.key}>{collection.key}</option>
                  ))}
                </select>
                <input
                  key={`${entry.ref.collectionKey}:${entry.ref.index}:${entry.timeSeconds}`}
                  type="number"
                  min={0}
                  step="any"
                  defaultValue={formatMarkTimeInput(entry.timeSeconds)}
                  aria-label="Selected mark time"
                  onBlur={(event) => {
                    moveSelectedMark(entry, Number(event.currentTarget.value));
                  }}
                />
                <input
                  key={`${entry.ref.collectionKey}:${entry.ref.index}:label:${entry.label ?? ""}`}
                  type="text"
                  className="mark-label-input"
                  placeholder="Label"
                  defaultValue={entry.label ?? ""}
                  aria-label="Selected mark label"
                  onBlur={(event) => {
                    labelSelectedMark(entry, event.currentTarget.value);
                  }}
                />
                <button
                  type="button"
                  className="icon-button danger-icon-button"
                  title="Delete mark"
                  aria-label="Delete mark"
                  onClick={() => {
                    deleteSelectedMark(entry);
                  }}
                >
                  <Trash2 size={THEME_METRICS.iconSizeSmall} />
                </button>
              </div>
            ))}
          </div>
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
        </div>
      ) : (
        <p>Select a mark on the timeline.</p>
      )}
      {confirmDialog}
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

function formatMarkTimeInput(timeSeconds: number) {
  return timeSeconds.toFixed(9).replace(/\.?0+$/, "");
}

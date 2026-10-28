import { useSequenceEditorHost } from "../../../editor/host";
import { useMemo, useState } from "react";

import { Plus, Trash2 } from "lucide-react";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";

import type {
  SequenceGuiDocument,
  SequenceAutomationTarget,
  SequenceEffect,
  SequenceEffectDetails,
  SequenceEffectParam,
  SequenceMarkCollection,
  SequenceMarkRef,
  SequenceEffectScope,
  SequenceEffectCommonEdit
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
import { clipIdsOfKind, clipRefs, selectedEffectId, selectionCompatibleWithFocusedItem, selectionCount } from "./sequenceSelection";
import { useSequenceEffectDetails } from "./sequenceEffectDetails";
import { targetsEqual } from "./sequenceTargets";
import { detachedAutomationTargetLabel } from "./sequenceAutomationLayout";
import { useConfirmDialog } from "../../ConfirmDialog";
import { LayerList } from "./sequenceLayers";

type SequenceInspectorTab = "effect" | "layers" | "marks";

type SelectedMarkEntry = {
  ref: SequenceMarkRef;
  collection: SequenceMarkCollection;
  timeSeconds: number;
  label: string | null;
};

const SEQUENCE_INSPECTOR_TABS: { id: SequenceInspectorTab; label: string }[] = [
  { id: "effect", label: "Effect" },
  { id: "layers", label: "Layers" },
  { id: "marks", label: "Marks" },
];

function supportsAutomation(document: SequenceGuiDocument, details: Map<number, SequenceEffectDetails> | null, target: SequenceAutomationTarget) {
  if (target.type === "effectParam") {
    return details?.get(target.effectId)?.params
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
  const [tab, setActiveTab] = useState<SequenceInspectorTab>("effect");
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
      {activeTab === "effect" && (
        <EffectInspectorPanel
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

function SelectedEffectsInspector({
  document,
  effectIds,
  onDelete
}: {
  document: SequenceGuiDocument;
  effectIds: number[];
  onDelete: () => void;
}) {
  const host = useSequenceEditorHost();
  const { commands, runGuiEditCommand } = host;

  const effects = effectIds
    .map((id) => document.effects.find((effect) => effect.id === id))
    .filter((effect): effect is SequenceEffect => effect !== undefined);
  const details = useSequenceEffectDetails(document, effects.map((effect) => effect.id));
  if (effects.length === 0) {
    return <><h2>Effects</h2><p>Select an effect on the timeline.</p></>;
  }

  const selectedEffectIds = effects.map((effect) => effect.id);
  const layerId = commonEffectValue(effects, (effect) => effect.layerId);
  const scope = commonEffectValue(effects, (effect) => effect.scope);
  const startSeconds = commonEffectValue(effects, (effect) => effect.startSeconds);
  const durationSeconds = commonEffectValue(effects, (effect) => effect.durationSeconds);
  const commonParams = details === null ? [] : commonEditableEffectParams(effects.map((effect) => details.get(effect.id)).filter((effect) => effect !== undefined));
  const applyEdit = (edit: SequenceEffectCommonEdit) =>
    runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "editEffects",
        clips: clipRefs("effect", selectedEffectIds),
        edit
      } })
    );

  return (
    <>
      <h2>Effects</h2>
      <div className="inspector-readout-grid">
        <Readout label="Selected" value={String(effects.length)} />
      </div>
      <div className="effect-inspector-fields">
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
          <MultiEffectNumberField
            effectIds={selectedEffectIds}
            field="start"
            label="Start"
            min={0}
            value={startSeconds}
            onCommit={(value) => void applyEdit({ type: "start", startSeconds: value })}
          />
          <MultiEffectNumberField
            effectIds={selectedEffectIds}
            field="duration"
            label="Duration"
            min={0.000000001}
            value={durationSeconds}
            onCommit={(value) => void applyEdit({ type: "duration", durationSeconds: value })}
          />
        </div>
      </div>
      {commonParams.length > 0 && (
        <div className="effect-param-section">
          <h3>Shared parameters</h3>
          {commonParams.map(({ param, mixed }, index) => (
            <div
              key={`${selectedEffectIds.join(",")}:${param.name}`}
              className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}
            >
              <SharedEffectParamInput
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

function commonEditableEffectParams(effects: SequenceEffectDetails[]): { param: SequenceEffectParam; mixed: boolean }[] {
  const first = effects[0];
  if (first === undefined) return [];
  return first.params.flatMap((param) => {
    if (!param.editable) return [];
    const matches = effects.slice(1).map((effect) => effect.params.find((candidate) =>
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

function SharedEffectParamInput({
  param,
  mixed,
  commitParam,
  document
}: {
  param: SequenceEffectParam;
  mixed: boolean;
  commitParam: (name: string, value: SequenceEffectParam["value"]) => Promise<void>;
  document: SequenceGuiDocument;
}) {
  const [editingMixed, setEditingMixed] = useState(false);
  if (mixed && !editingMixed) {
    return (
      <div className="effect-param-group">
        <div className="effect-param-name">{param.name}</div>
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

function MultiEffectNumberField({
  effectIds,
  field,
  label,
  min,
  value,
  onCommit
}: {
  effectIds: number[];
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
        key={`${effectIds.join(",")}:${field}:${value ?? "multiple"}`}
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

function commonEffectValue<T>(effects: SequenceEffect[], read: (effect: SequenceEffect) => T): T | null {
  const first = effects[0];
  if (first === undefined) return null;
  const value = read(first);
  return effects.every((effect) => Object.is(read(effect), value)) ? value : null;
}

function EffectInspectorPanel({
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
    ? document.automationClips.find((clip) => clip.id === selected.id) ?? null
    : null;
  const inspectedEffectIds = inspectedAutomationClip !== null
    ? inspectedAutomationClip.detachedBindings.flatMap((binding) => binding.target.type === "effectParam" ? [binding.target.effectId] : [])
    : [selectedEffectId(selected)].filter((id) => id !== null);
  const details = useSequenceEffectDetails(document, inspectedEffectIds);

  if (sequenceSelection !== null && selectionCount(sequenceSelection) > 1 && selectionCompatibleWithFocusedItem(sequenceSelection, selected)) {
    if (sequenceSelection.type !== "clips") {
      return (
        <>
          <h2>Effect Parameters</h2>
          <p>Select an effect on the timeline.</p>
        </>
      );
    }
    if (sequenceSelection.clips.every((clip) => clip.type === "effect")) {
      return (
        <SelectedEffectsInspector
          document={document}
          effectIds={clipIdsOfKind(sequenceSelection.clips, "effect")}
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
    ? document.automationClips.find((clip) => clip.id === selected.id) ?? null
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
            <div className="effect-param-actions">
              <button
                type="button"
                disabled={!supportsAutomation(document, details, binding.target)}
                onClick={() => void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "sequence", edit: {
                  type: "rebindDetachedAutomation",
                  clipId: automationClip.id,
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
                  clipId: automationClip.id,
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
  const id = selectedEffectId(selected);
  const effect = document.effects.find((candidate) => candidate.id === id);


  if (effect === undefined) {
    return (
      <>
        <h2>Effect Parameters</h2>
        <p>Select an effect on the timeline.</p>
      </>
    );
  }

  const effectDetails = details?.get(effect.id);
  const currentDefinition = effectDetails === undefined ? undefined : document.effectDefinitions.find((definition) =>
    effectReferencesEqual(definition.effect, effectDetails.effectReference)
  );
  const editEffect = (edit: SequenceEffectCommonEdit) =>
    runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: { type: "editEffects", clips: [{ type: "effect", id: effect.id }], edit } })
    );
  const item = { type: "clip", id: effect.id } as const;

  return (
    <>
      <h2>Effect Parameters</h2>
      <div className="effect-inspector-fields">
        <NameField
          name={effect.name}
          label="Name"
          commit={(name) =>
            runGuiEditCommand((request) =>
              commands.applyGuiEdit(request, { type: "rename", item, name })
            )
          }
        />
        <DescriptionField
          description={effect.description}
          onCommit={(description) =>
            runGuiEditCommand((request) =>
              commands.applyGuiEdit(request, { type: "setDescription", item, description })
            )
          }
        />
        <label>
          Layer
          <select
            value={String(effect.layerId)}
            onChange={(event) =>
              void editEffect({ type: "layer", layerId: Number(event.currentTarget.value) })
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
              key={`${effect.id}:start:${effect.startSeconds}`}
              type="number"
              min={0}
              step="any"
              defaultValue={effect.startSeconds}
              onBlur={(event) => {
                const nextStartSeconds = Number(event.currentTarget.value);
                if (!Number.isFinite(nextStartSeconds) || roundToNanosecond(nextStartSeconds) === effect.startSeconds) return;
                void editEffect({ type: "start", startSeconds: Math.max(0, roundToNanosecond(nextStartSeconds)) });
              }}
            />
          </label>
          <label>
            Duration
            <input
              key={`${effect.id}:duration:${effect.durationSeconds}`}
              type="number"
              min={0.000000001}
              step="any"
              defaultValue={effect.durationSeconds}
              onBlur={(event) => {
                const nextDurationSeconds = Number(event.currentTarget.value);
                if (!Number.isFinite(nextDurationSeconds) || roundToNanosecond(nextDurationSeconds) === effect.durationSeconds) return;
                void editEffect({ type: "duration", durationSeconds: Math.max(0.000000001, roundToNanosecond(nextDurationSeconds)) });
              }}
            />
          </label>
        </div>
        <label>
          Effect type
          <EffectDefinitionPicker
            definitions={document.effectDefinitions}
            label={currentDefinition?.name ?? effect.effect}
            onSelect={(definition) =>
              void runGuiEditCommand((request) =>
                commands.applyGuiEdit(request, { type: "sequence", edit: {
                  type: "changeEffectDefinition",
                  initialColor: THEME_COLORS.white,
                  id: effect.id,
                  effect: definition.effect
                } })
              )
            }
          />
        </label>
        {currentDefinition !== undefined && currentDefinition.description !== null && (
          <p className="effect-param-description">{currentDefinition.description}</p>
        )}
      </div>
      <label>
        Scope
        <select
          value={effect.scope}
          onChange={(event) =>
            void editEffect({ type: "scope", scope: event.currentTarget.value as SequenceEffectScope })
          }
        >
          <option value="perFixture">Per fixture</option>
          <option value="wholeTarget">Whole target</option>
        </select>
      </label>
      {effectDetails !== undefined && effectDetails.params.length > 0 && (
        <>
          <div className="inspector-section-divider" />
          <div className="effect-param-section">
            <h3>Parameters</h3>
            {effectDetails.params.map((param, index) => (
              <div
                key={`${effect.id}:${param.name}`}
                className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}
              >
                <TypedParamInput
                  param={param}
                  commitParam={(name, value) =>
                    editEffect({ type: "param", name, value }).then(() => undefined)
                  }
                  curveLibrary={document.curveLibrary}
                  gradientLibrary={document.gradientLibrary}
                  markCollections={document.markCollections}
                  automation={{
                    target: { type: "effectParam", effectId: effect.id, param: param.name },
                    automationClips: document.automationClips,
                    canCreateAutomationClip: document.lanes.some((lane) => targetsEqual(lane.target, effect.target)),
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
      description: `Its ${collection.marksSeconds.length} marks are deleted too. Effects using it will have no marks.`,
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

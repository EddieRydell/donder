import { THEME_COLORS } from "../../../theme";
import type { RigActionChoice, RigControlTarget, RigGuiDocument, RigMapping, SequenceEffectParam } from "../../../types";
import { DescriptionField } from "../DescriptionField";
import { NameField } from "../sequence/NameField";
import { RigEffectFields, RigInputPicker } from "./RigFields";
import { applyRigEdit } from "./rigEditing";

type ActionKind = RigActionChoice["type"];

/** What a `Control` mapping can drive: voices and the other mappings' triggers. */
function controlTargets(document: RigGuiDocument, mapping: RigMapping): { key: string; label: string; target: RigControlTarget; params: string[] }[] {
  const choices: { key: string; label: string; target: RigControlTarget; params: SequenceEffectParam[] }[] = [
    ...document.voices.map((voice) => ({ key: `voice:${voice.id}`, label: `Voice ${voice.name}`, target: { type: "voice" as const, voiceId: voice.id }, params: voice.effect.params })),
    ...document.mappings.flatMap((other) => other.id === mapping.id || other.action.type !== "trigger" ? [] : [{
      key: `trigger:${other.id}`, label: `Trigger ${other.name}`, target: { type: "trigger" as const, mappingId: other.id }, params: other.action.effect.params
    }])
  ];
  return choices.map((choice) => ({ ...choice, params: choice.params.filter((param) => param.supportsAutomation).map((param) => param.name) }));
}

function controlKey(target: RigControlTarget) {
  return target.type === "voice" ? `voice:${target.voiceId}` : `trigger:${target.mappingId}`;
}

/** A new action of `kind` from the rig's first choices, or `null` when the rig has none. */
function defaultAction(document: RigGuiDocument, mapping: RigMapping, kind: ActionKind): RigActionChoice | null {
  switch (kind) {
    case "trigger": {
      const definition = document.effectDefinitions[0];
      const target = document.targets[0];
      return definition === undefined || target === undefined ? null
        : { type: "trigger", initialColor: THEME_COLORS.white, effect: definition.effect, target: target.target };
    }
    case "mark": {
      const marks = document.liveMarks[0];
      return marks === undefined ? null : { type: "mark", marks: marks.key };
    }
    case "control": {
      const choice = controlTargets(document, mapping).find((candidate) => candidate.params.length > 0);
      const param = choice?.params[0];
      return choice === undefined || param === undefined ? null : { type: "control", target: choice.target, param };
    }
  }
}

export function RigMappingCard({ document, mapping }: { document: RigGuiDocument; mapping: RigMapping }) {
  const item = { type: "mapping", id: mapping.id } as const;
  const when = mapping.when === null ? "" : `${mapping.when.type}:${mapping.when.switchId}`;
  const action = mapping.action;
  const targets = controlTargets(document, mapping);
  return <div className="setup-section rig-item">
    <div className="inspector-inline-row">
      <NameField name={mapping.name} label="Name" commit={(name) => applyRigEdit({ type: "rename", item, name })} />
      <button type="button" onClick={() => void applyRigEdit({ type: "delete", item })}>Delete mapping</button>
    </div>
    <DescriptionField description={mapping.description} onCommit={(description) => applyRigEdit({ type: "setDescription", item, description })} />
    <RigInputPicker document={document} input={mapping.input} dials={mapping.action.type === "control"} onChange={(input) => void applyRigEdit({ type: "setMappingInput", id: mapping.id, input })} />
    <div className="inspector-inline-row">
      <label>When<select value={when} onChange={(event) => {
        const [type, id] = event.currentTarget.value.split(":");
        void applyRigEdit({ type: "setMappingWhen", id: mapping.id, when: type === "on" || type === "off" ? { type, switchId: Number(id) } : null });
      }}>
        <option value="">Always</option>
        {document.switches.flatMap((item) => [
          <option key={`on:${item.id}`} value={`on:${item.id}`}>While {item.name} is on</option>,
          <option key={`off:${item.id}`} value={`off:${item.id}`}>While {item.name} is off</option>
        ])}
      </select></label>
      <label>Action<select value={action.type} onChange={(event) => {
        const next = defaultAction(document, mapping, event.currentTarget.value as ActionKind);
        if (next !== null) void applyRigEdit({ type: "setMappingAction", id: mapping.id, action: next });
      }}>
        <option value="trigger" disabled={defaultAction(document, mapping, "trigger") === null}>Trigger an effect</option>
        <option value="mark" disabled={defaultAction(document, mapping, "mark") === null}>Add a live mark</option>
        <option value="control" disabled={defaultAction(document, mapping, "control") === null}>Control a parameter</option>
      </select></label>
    </div>
    {action.type === "trigger" && <>
      <div className="inspector-inline-row">
        <label>Length<select value={action.length.type} onChange={(event) => {
          const type = event.currentTarget.value;
          void applyRigEdit({ type: "setTriggerLength", mappingId: mapping.id, length: type === "held" || type === "latched" ? { type } : { type: "fixed", durationSeconds: 1 } });
        }}>
          <option value="fixed">Fixed</option>
          <option value="held">While held</option>
          <option value="latched">Latched</option>
        </select></label>
        {action.length.type === "fixed" && <label>Seconds<input key={action.length.durationSeconds} type="number" min={0} step="any" defaultValue={action.length.durationSeconds} onBlur={(event) => {
          const durationSeconds = Number(event.currentTarget.value);
          if (action.length.type === "fixed" && durationSeconds !== action.length.durationSeconds) void applyRigEdit({ type: "setTriggerLength", mappingId: mapping.id, length: { type: "fixed", durationSeconds } });
        }} /></label>}
        <label>Voices<select value={action.voices.type} onChange={(event) => {
          const type = event.currentTarget.value;
          void applyRigEdit({ type: "setTriggerVoices", mappingId: mapping.id, voices: type === "retrigger" || type === "mono" ? { type } : { type: "stack", count: 2 } });
        }}>
          <option value="retrigger">Retrigger</option>
          <option value="stack">Stack</option>
          <option value="mono">Mono</option>
        </select></label>
        {action.voices.type === "stack" && <label>Count<input key={action.voices.count} type="number" min={1} max={document.maxStackedVoices} step={1} defaultValue={action.voices.count} onBlur={(event) => {
          const count = Number(event.currentTarget.value);
          if (action.voices.type === "stack" && count !== action.voices.count) void applyRigEdit({ type: "setTriggerVoices", mappingId: mapping.id, voices: { type: "stack", count } });
        }} /></label>}
      </div>
      <RigEffectFields document={document} owner={{ type: "trigger", mappingId: mapping.id }} effect={action.effect} />
    </>}
    {action.type === "mark" && <label>Live marks<select value={action.marks} onChange={(event) =>
      void applyRigEdit({ type: "setMappingAction", id: mapping.id, action: { type: "mark", marks: event.currentTarget.value } })}>
      {!document.liveMarks.some((marks) => marks.key === action.marks) && <option value={action.marks} disabled>{action.marks}</option>}
      {document.liveMarks.map((marks) => <option key={marks.key} value={marks.key}>{marks.key}</option>)}
    </select></label>}
    {action.type === "control" && <div className="inspector-inline-row">
      <label>Controls<select value={controlKey(action.target)} onChange={(event) => {
        const choice = targets.find((candidate) => candidate.key === event.currentTarget.value);
        const param = choice?.params[0];
        if (choice !== undefined && param !== undefined) void applyRigEdit({ type: "setMappingAction", id: mapping.id, action: { type: "control", target: choice.target, param } });
      }}>
        {targets.map((choice) => <option key={choice.key} value={choice.key} disabled={choice.params.length === 0}>{choice.label}</option>)}
      </select></label>
      <label>Parameter<select value={action.param} onChange={(event) =>
        void applyRigEdit({ type: "setMappingAction", id: mapping.id, action: { type: "control", target: action.target, param: event.currentTarget.value } })}>
        {(targets.find((choice) => choice.key === controlKey(action.target))?.params ?? [action.param]).map((param) => <option key={param} value={param}>{param}</option>)}
      </select></label>
    </div>}
  </div>;
}

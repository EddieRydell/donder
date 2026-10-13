import { guiObjectKey } from "../../../workspace/guiIdentity";
import { THEME_COLORS } from "../../../theme";
import type { RigEffect, RigEffectOwner, RigGuiDocument, RigInput, RigButtonRef, SequenceEffectScope } from "../../../types";
import { EffectDefinitionPicker } from "../sequence/definitionMenu";
import { TypedParamInput } from "../sequence/params/TypedParamInput";
import { editRigEffect } from "./rigEditing";

/** A button of the setup's input devices: the device, then its button. */
export function RigInputPicker({ document, input, onChange }: {
  document: RigGuiDocument;
  input: RigInput;
  onChange: (button: RigButtonRef) => void;
}) {
  const currentKey = guiObjectKey(input.button.device);
  const device = document.inputDevices.find((candidate) => guiObjectKey(candidate.device) === currentKey);
  return <div className="inspector-inline-row">
    <label>Device<select value={currentKey} onChange={(event) => {
      const next = document.inputDevices.find((candidate) => guiObjectKey(candidate.device) === event.currentTarget.value);
      const button = next?.buttons[0];
      if (next !== undefined && button !== undefined) onChange({ device: next.device, control: button.control });
    }}>
      {device === undefined && <option value={currentKey} disabled>{input.deviceName} (not in the setup)</option>}
      {document.inputDevices.map((candidate) => <option key={guiObjectKey(candidate.device)} value={guiObjectKey(candidate.device)} disabled={candidate.buttons.length === 0}>{candidate.name}</option>)}
    </select></label>
    <label>Button<select value={input.button.control} onChange={(event) => {
      if (device !== undefined) onChange({ device: device.device, control: Number(event.currentTarget.value) });
    }}>
      {device?.buttons.some((button) => button.control === input.button.control) !== true && <option value={input.button.control} disabled>{input.controlName}</option>}
      {device?.buttons.map((button) => <option key={button.control} value={button.control}>{button.name}</option>)}
    </select></label>
  </div>;
}

/** A voice's or trigger's effect: what it plays, where, and its parameters. */
export function RigEffectFields({ document, owner, effect }: { document: RigGuiDocument; owner: RigEffectOwner; effect: RigEffect }) {
  return <>
    <div className="inspector-inline-row">
      <label>Layer<select value={effect.layerId} onChange={(event) => void editRigEffect(owner, { type: "layer", layerId: Number(event.currentTarget.value) })}>
        {document.layers.map((layer) => <option key={layer.id} value={layer.id}>{layer.name}</option>)}
      </select></label>
      <label>Target<select value={effect.target.fixture} onChange={(event) => void editRigEffect(owner, { type: "target", target: { fixture: Number(event.currentTarget.value) } })}>
        {!document.targets.some((lane) => lane.target.fixture === effect.target.fixture) && <option value={effect.target.fixture} disabled>{effect.targetLabel}</option>}
        {document.targets.map((lane) => <option key={lane.target.fixture} value={lane.target.fixture}>{"  ".repeat(lane.depth)}{lane.label}</option>)}
      </select></label>
      <label>Scope<select value={effect.scope} onChange={(event) => void editRigEffect(owner, { type: "scope", scope: event.currentTarget.value as SequenceEffectScope })}>
        <option value="perFixture">Per fixture</option>
        <option value="wholeTarget">Whole target</option>
      </select></label>
    </div>
    <label>Effect type<EffectDefinitionPicker definitions={document.effectDefinitions} label={effect.effectName} onSelect={(definition) =>
      void editRigEffect(owner, { type: "definition", initialColor: THEME_COLORS.white, effect: definition.effect })} /></label>
    {effect.params.length > 0 && <div className="effect-param-section">
      {effect.params.map((param, index) => <div key={param.name} className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}>
        <TypedParamInput param={param} commitParam={(name, value) => editRigEffect(owner, { type: "param", name, value })}
          curveLibrary={document.curveLibrary} gradientLibrary={document.gradientLibrary} markCollections={document.liveMarks} />
      </div>)}
    </div>}
  </>;
}

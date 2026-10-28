import { guiObjectKey } from "../../../workspace/guiIdentity";
import { THEME_COLORS } from "../../../theme";
import type { RigClip, RigClipOwner, RigGuiDocument, RigInput, RigInputDevice, RigControlRef, RigMappingAction, SequenceClipScope } from "../../../types";
import { EffectDefinitionPicker } from "../sequence/definitionMenu";
import { TypedParamInput } from "../sequence/params/TypedParamInput";
import { editRigClip } from "./rigEditing";

/** What plays from an input: a switch, or a mapping's action. */
export type RigInputUse = "switch" | RigMappingAction["type"];

/** A control of the setup's input devices that `use` can play from: the
 * device, then its control. Switches play from buttons, `Control` mappings
 * from buttons and dials, and triggers and marks from buttons and keys. */
export function RigInputPicker({ document, input, use, onChange }: {
  document: RigGuiDocument;
  input: RigInput;
  use: RigInputUse;
  onChange: (control: RigControlRef) => void;
}) {
  const currentKey = guiObjectKey(input.control.device);
  const device = document.inputDevices.find((candidate) => guiObjectKey(candidate.device) === currentKey);
  const choices = (candidate: RigInputDevice) => [
    ...candidate.buttons,
    ...use === "control" ? candidate.dials : [],
    ...use === "trigger" || use === "mark" ? candidate.keys : []
  ];
  const controls = device === undefined ? [] : choices(device);
  return <div className="inspector-inline-row">
    <label>Device<select value={currentKey} onChange={(event) => {
      const next = document.inputDevices.find((candidate) => guiObjectKey(candidate.device) === event.currentTarget.value);
      const control = next === undefined ? undefined : choices(next)[0];
      if (next !== undefined && control !== undefined) onChange({ device: next.device, control: control.control });
    }}>
      {device === undefined && <option value={currentKey} disabled>{input.deviceName} (not in the setup)</option>}
      {document.inputDevices.map((candidate) => <option key={guiObjectKey(candidate.device)} value={guiObjectKey(candidate.device)} disabled={choices(candidate).length === 0}>{candidate.name}</option>)}
    </select></label>
    <label>{use === "switch" ? "Button" : "Control"}<select value={input.control.control} onChange={(event) => {
      if (device !== undefined) onChange({ device: device.device, control: Number(event.currentTarget.value) });
    }}>
      {!controls.some((control) => control.control === input.control.control) && <option value={input.control.control} disabled>{input.controlName}</option>}
      {device?.buttons.map((button) => <option key={button.control} value={button.control}>{button.name}</option>)}
      {use === "control" && device?.dials.map((dial) => <option key={dial.control} value={dial.control}>{dial.name} (dial)</option>)}
      {(use === "trigger" || use === "mark") && device?.keys.map((keys) => <option key={keys.control} value={keys.control}>{keys.name} (keys)</option>)}
    </select></label>
  </div>;
}

/** A voice's or trigger's clip: what it plays, where, and its parameters. */
export function RigClipFields({ document, owner, clip }: { document: RigGuiDocument; owner: RigClipOwner; clip: RigClip }) {
  return <>
    <div className="inspector-inline-row">
      <label>Layer<select value={clip.layerId} onChange={(event) => void editRigClip(owner, { type: "layer", layerId: Number(event.currentTarget.value) })}>
        {document.layers.map((layer) => <option key={layer.id} value={layer.id}>{layer.name}</option>)}
      </select></label>
      <label>Target<select value={clip.target.fixture} onChange={(event) => void editRigClip(owner, { type: "target", target: { fixture: Number(event.currentTarget.value) } })}>
        {!document.targets.some((lane) => lane.target.fixture === clip.target.fixture) && <option value={clip.target.fixture} disabled>{clip.targetLabel}</option>}
        {document.targets.map((lane) => <option key={lane.target.fixture} value={lane.target.fixture}>{"  ".repeat(lane.depth)}{lane.label}</option>)}
      </select></label>
      <label>Scope<select value={clip.scope} onChange={(event) => void editRigClip(owner, { type: "scope", scope: event.currentTarget.value as SequenceClipScope })}>
        <option value="perFixture">Per fixture</option>
        <option value="wholeTarget">Whole target</option>
      </select></label>
    </div>
    <label>Effect type<EffectDefinitionPicker definitions={document.effectDefinitions} label={clip.effectName} onSelect={(definition) =>
      void editRigClip(owner, { type: "definition", initialColor: THEME_COLORS.white, effect: definition.effect })} /></label>
    {clip.params.length > 0 && <div className="effect-param-section">
      {clip.params.map((param, index) => <div key={param.name} className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}>
        <TypedParamInput param={param} commitParam={(name, value) => editRigClip(owner, { type: "param", name, value })}
          curveLibrary={document.curveLibrary} gradientLibrary={document.gradientLibrary} markCollections={document.liveMarks} />
      </div>)}
    </div>}
  </>;
}

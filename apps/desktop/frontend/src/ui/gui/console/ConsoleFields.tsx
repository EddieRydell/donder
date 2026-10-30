import { guiObjectKey } from "../../../workspace/guiIdentity";
import { THEME_COLORS } from "../../../theme";
import type { ConsoleClip, ConsoleControlRef, ConsoleGuiDocument, ConsoleInput, ConsoleInputDevice, SequenceClipScope } from "../../../types";
import { EffectDefinitionPicker } from "../sequence/definitionMenu";
import { TypedParamInput } from "../sequence/params/TypedParamInput";
import { editConsoleClip } from "./consoleEditing";

/** A control of the setup's input devices an event reads: the device, then
 * one of its buttons, or of its axes for a move. */
export function ConsoleInputPicker({ document, input, axis, onChange }: {
  document: ConsoleGuiDocument;
  input: ConsoleInput;
  axis: boolean;
  onChange: (control: ConsoleControlRef) => void;
}) {
  const currentKey = guiObjectKey(input.control.device);
  const device = document.inputDevices.find((candidate) => guiObjectKey(candidate.device) === currentKey);
  const choices = (candidate: ConsoleInputDevice) => axis ? candidate.axes : candidate.buttons;
  const controls = device === undefined ? [] : choices(device);
  return <>
    <label>Device<select value={currentKey} onChange={(event) => {
      const next = document.inputDevices.find((candidate) => guiObjectKey(candidate.device) === event.currentTarget.value);
      const control = next === undefined ? undefined : choices(next)[0];
      if (next !== undefined && control !== undefined) onChange({ device: next.device, control: control.control });
    }}>
      {device === undefined && <option value={currentKey} disabled>{input.deviceName} (not in the setup)</option>}
      {document.inputDevices.map((candidate) => <option key={guiObjectKey(candidate.device)} value={guiObjectKey(candidate.device)} disabled={choices(candidate).length === 0}>{candidate.name}</option>)}
    </select></label>
    <label>{axis ? "Axis" : "Button"}<select value={input.control.control} onChange={(event) => {
      if (device !== undefined) onChange({ device: device.device, control: Number(event.currentTarget.value) });
    }}>
      {!controls.some((control) => control.control === input.control.control) && <option value={input.control.control} disabled>{input.controlName}</option>}
      {controls.map((control) => <option key={control.control} value={control.control}>{control.name}</option>)}
    </select></label>
  </>;
}

/** A clip's layer, target, scope, effect and parameters. */
export function ConsoleClipFields({ document, clip }: { document: ConsoleGuiDocument; clip: ConsoleClip }) {
  return <>
    <div className="inspector-inline-row">
      <label>Layer<select value={clip.layerId} onChange={(event) => void editConsoleClip(clip.id, { type: "layer", layerId: Number(event.currentTarget.value) })}>
        {document.layers.map((layer) => <option key={layer.id} value={layer.id}>{layer.name}</option>)}
      </select></label>
      <label>Target<select value={clip.target.fixture} onChange={(event) => void editConsoleClip(clip.id, { type: "target", target: { fixture: Number(event.currentTarget.value) } })}>
        {!document.targets.some((lane) => lane.target.fixture === clip.target.fixture) && <option value={clip.target.fixture} disabled>{clip.targetLabel}</option>}
        {document.targets.map((lane) => <option key={lane.target.fixture} value={lane.target.fixture}>{"  ".repeat(lane.depth)}{lane.label}</option>)}
      </select></label>
      <label>Scope<select value={clip.scope} onChange={(event) => void editConsoleClip(clip.id, { type: "scope", scope: event.currentTarget.value as SequenceClipScope })}>
        <option value="perFixture">Per fixture</option>
        <option value="wholeTarget">Whole target</option>
      </select></label>
    </div>
    <label>Effect type<EffectDefinitionPicker definitions={document.effectDefinitions} label={clip.effectName} onSelect={(definition) =>
      void editConsoleClip(clip.id, { type: "definition", initialColor: THEME_COLORS.white, effect: definition.effect })} /></label>
    {clip.params.length > 0 && <div className="effect-param-section">
      {clip.params.map((param, index) => <div key={param.name} className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}>
        <TypedParamInput param={param} commitParam={(name, value) => editConsoleClip(clip.id, { type: "param", name, value })}
          curveLibrary={document.curveLibrary} gradientLibrary={document.gradientLibrary} markCollections={document.liveMarks} />
      </div>)}
    </div>}
  </>;
}

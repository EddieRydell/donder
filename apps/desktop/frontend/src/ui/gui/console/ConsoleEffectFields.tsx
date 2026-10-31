import { THEME_COLORS } from "../../../theme";
import type { ConsoleClip, ConsoleGuiDocument, SequenceClipParam, SequenceClipScope } from "../../../types";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { DescriptionField } from "../DescriptionField";
import { NameField } from "../sequence/NameField";
import { EffectDefinitionPicker, effectReferencesEqual } from "../sequence/definitionMenu";
import { TypedParamInput } from "../sequence/params/TypedParamInput";
import { applyConsoleEdit, applyConsoleItemEdit, controllerOf, controlKey, controlRef, editConsoleEffect } from "./consoleEditing";

/** Which control sets a parameter: none, or any button or axis of the setup's devices. */
function ParamControl({ document, clipId, param }: { document: ConsoleGuiDocument; clipId: number; param: SequenceClipParam }) {
  const target = { type: "clipParam", clipId, param: param.name } as const;
  const current = controllerOf(document, target);
  const value = current === undefined ? "" : controlKey(current.input.control);
  const controls = document.inputDevices.map((device) => ({ device, controls: [...device.buttons, ...device.axes] }));
  const listed = controls.some(({ device, controls: all }) => all.some((control) => controlKey(controlRef(device, control)) === value));
  return <select aria-label={`Control ${param.name} from`} value={value} title="Set this from a control: an axis by its position, a button by its velocity" onChange={(event) => {
    const key = event.currentTarget.value;
    const chosen = controls.flatMap(({ device, controls: all }) => all.map((control) => controlRef(device, control))).find((ref) => controlKey(ref) === key);
    void applyConsoleEdit({ type: "controlParam", target, control: chosen ?? null });
  }}>
    <option value="">Not controlled</option>
    {current !== undefined && !listed && <option value={value} disabled>{current.input.controlName}</option>}
    {controls.map(({ device, controls: all }) => <optgroup key={guiObjectKey(device.device)} label={device.name}>
      {all.map((control) => <option key={control.control} value={controlKey(controlRef(device, control))}>{control.name}</option>)}
    </optgroup>)}
  </select>;
}

/** An effect the console plays: its name, layer, target, scope, effect type and parameters. */
export function EffectFields({ document, clip }: { document: ConsoleGuiDocument; clip: ConsoleClip }) {
  const item = { type: "clip", id: clip.id } as const;
  const definition = document.effectDefinitions.find((candidate) => effectReferencesEqual(candidate.effect, clip.effect));
  return <>
    <div className="clip-inspector-fields">
      <NameField name={clip.name} label="Name" commit={(name) => applyConsoleItemEdit({ type: "rename", item, name })} />
      <DescriptionField description={clip.description} onCommit={(description) => applyConsoleItemEdit({ type: "setDescription", item, description })} />
      <label>Layer<select value={clip.layerId} onChange={(event) => void editConsoleEffect(clip.id, { type: "layer", layerId: Number(event.currentTarget.value) })}>
        {document.layers.map((layer) => <option key={layer.id} value={layer.id}>{layer.name}</option>)}
      </select></label>
      <label>Target<select value={clip.target.fixture} onChange={(event) => void editConsoleEffect(clip.id, { type: "target", target: { fixture: Number(event.currentTarget.value) } })}>
        {!document.targets.some((lane) => lane.target.fixture === clip.target.fixture) && <option value={clip.target.fixture} disabled>{clip.targetLabel}</option>}
        {document.targets.map((lane) => <option key={lane.target.fixture} value={lane.target.fixture}>{"  ".repeat(lane.depth)}{lane.label}</option>)}
      </select></label>
      <label>Effect type<EffectDefinitionPicker definitions={document.effectDefinitions} label={definition?.name ?? clip.effectName} onSelect={(next) =>
        void editConsoleEffect(clip.id, { type: "definition", initialColor: THEME_COLORS.white, effect: next.effect })} /></label>
      {definition?.description !== null && definition?.description !== undefined && <p className="clip-param-description">{definition.description}</p>}
      <label>Scope<select value={clip.scope} onChange={(event) => void editConsoleEffect(clip.id, { type: "scope", scope: event.currentTarget.value as SequenceClipScope })}>
        <option value="perFixture">Per fixture</option>
        <option value="wholeTarget">Whole target</option>
      </select></label>
    </div>
    {clip.params.length > 0 && <>
      <div className="inspector-section-divider" />
      <div className="clip-param-section">
        <h3>Parameters</h3>
        {clip.params.map((param, index) => <div key={param.name} className={`clip-param-row ${index % 2 === 0 ? "clip-param-row-even" : "clip-param-row-odd"}`}>
          <TypedParamInput param={param} commitParam={(name, value) => editConsoleEffect(clip.id, { type: "param", name, value }).then(() => undefined)}
            curveLibrary={document.curveLibrary} gradientLibrary={document.gradientLibrary} markCollections={document.liveMarks}
            automationControl={param.supportsAutomation ? <ParamControl document={document} clipId={clip.id} param={param} /> : null} />
        </div>)}
      </div>
    </>}
  </>;
}

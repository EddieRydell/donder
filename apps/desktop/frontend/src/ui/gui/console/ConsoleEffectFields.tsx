import { THEME_COLORS } from "../../../theme";
import type { ConsoleClip, ConsoleGuiDocument, SequenceClipScope } from "../../../types";
import { DescriptionField } from "../DescriptionField";
import { NameField } from "../sequence/NameField";
import { EffectDefinitionPicker, effectReferencesEqual } from "../sequence/definitionMenu";
import { TypedParamInput } from "../sequence/params/TypedParamInput";
import { applyConsoleItemEdit, editConsoleEffect } from "./consoleEditing";
import { KnobSelect } from "./KnobSelect";

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
            automationControl={param.supportsAutomation ? <KnobSelect document={document} target={{ type: "clipParam", clipId: clip.id, param: param.name }} label={param.name} /> : null} />
        </div>)}
      </div>
    </>}
  </>;
}

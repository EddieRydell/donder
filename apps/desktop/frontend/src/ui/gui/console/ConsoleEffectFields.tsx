import { THEME_COLORS } from "../../../theme";
import type { ConsoleClip, ConsoleGuiDocument, SequenceClipScope } from "../../../types";
import { Field, Section, Select } from "../../kit";
import { useFieldProblem } from "../problems";
import { DescriptionField } from "../DescriptionField";
import { NameField } from "../sequence/NameField";
import { EffectDefinitionPicker, effectReferencesEqual } from "../sequence/definitionMenu";
import { TypedParamInput } from "../sequence/params/TypedParamInput";
import { applyConsoleItemEdit, editConsoleEffect, isLinked } from "./consoleEditing";
import { ParamSourceSelect } from "./ParamSourceSelect";

const SCOPES: { value: SequenceClipScope; label: string }[] = [
  { value: "perFixture", label: "Per fixture" },
  { value: "wholeTarget", label: "Whole target" }
];

/** Indents a target lane under its group in a menu, where plain spaces collapse. */
const INDENT = "  ";

/** An effect the console plays: its name, layer, target, scope, effect type and parameters. */
export function EffectFields({ document, clip }: { document: ConsoleGuiDocument; clip: ConsoleClip }) {
  const item = { type: "clip", id: clip.id } as const;
  const problems = {
    layer: useFieldProblem(item, { type: "layer" }),
    target: useFieldProblem(item, { type: "target" }),
    effect: useFieldProblem(item, { type: "effect" }),
    scope: useFieldProblem(item, { type: "scope" })
  };
  const definition = document.effectDefinitions.find((candidate) => effectReferencesEqual(candidate.effect, clip.effect));
  const description = definition?.description ?? null;
  return <>
    <div className="clip-inspector-fields">
      <NameField name={clip.name} label="Name" member={item} commit={(name) => applyConsoleItemEdit({ type: "rename", item, name })} />
      <DescriptionField description={clip.description} member={item} onCommit={(description) => applyConsoleItemEdit({ type: "setDescription", item, description })} />
      <Field label="Layer" error={problems.layer}>
        <Select name="Layer" value={String(clip.layerId)} invalid={problems.layer !== null} onChange={(layerId) => void editConsoleEffect(clip.id, { type: "layer", layerId: Number(layerId) })}
          options={document.layers.map((layer) => ({ value: String(layer.id), label: layer.name }))} />
      </Field>
      <Field label="Target" error={problems.target}>
        <Select name="Target" value={String(clip.target.fixture)} invalid={problems.target !== null} onChange={(fixture) => void editConsoleEffect(clip.id, { type: "target", target: { fixture: Number(fixture) } })} options={[
          ...(document.targets.some((lane) => lane.target.fixture === clip.target.fixture) ? [] : [{ value: String(clip.target.fixture), label: clip.targetLabel, disabled: true }]),
          ...document.targets.map((lane) => ({ value: String(lane.target.fixture), label: `${INDENT.repeat(lane.depth)}${lane.label}` }))
        ]} />
      </Field>
      <Field label="Effect" error={problems.effect} {...(description === null ? {} : { tip: description })}>
        <EffectDefinitionPicker definitions={document.effectDefinitions} label={definition?.name ?? clip.effectName} onSelect={(next) =>
          void editConsoleEffect(clip.id, { type: "definition", initialColor: THEME_COLORS.white, effect: next.effect })} />
      </Field>
      <Field label="Scope" error={problems.scope}>
        <Select name="Scope" value={clip.scope} options={SCOPES} invalid={problems.scope !== null} onChange={(scope) => void editConsoleEffect(clip.id, { type: "scope", scope })} />
      </Field>
    </div>
    {clip.params.length > 0 && <Section title="Parameters">
      {clip.params.map((param, index) => <div key={param.name} className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}>
        <TypedParamInput param={param} commitParam={(name, value) => editConsoleEffect(clip.id, { type: "param", name, value }).then(() => undefined)}
          curveLibrary={document.curveLibrary} gradientLibrary={document.gradientLibrary} markCollections={document.liveMarks}
          automationControl={param.supportsAutomation ? <ParamSourceSelect document={document} target={{ type: "clipParam", clipId: clip.id, param: param.name }} clip={clip} label={param.name} range={param.range} /> : null}
          linked={isLinked(document, { type: "clipParam", clipId: clip.id, param: param.name }, clip)} member={item} />
      </div>)}
    </Section>}
  </>;
}

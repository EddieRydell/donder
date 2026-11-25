import { CheckCheck } from "lucide-react";
import { useState, type ComponentProps } from "react";
import { THEME_COLORS } from "../../../theme";
import type { GuiClip, GuiField, GuiMember, GuiValue, SequenceClipDetails, SequenceClipParam, SequenceClipScope, SequenceGuiDocument, SequenceMarkCollection } from "../../../types";
import { Button, Field, IconButton, NumberField, Section, Select, type SelectOption } from "../../kit";
import { DescriptionField } from "../DescriptionField";
import { Readout } from "../InspectorScrollArea";
import { describe, put, rename, site } from "../edits";
import { useFieldProblem } from "../problems";
import { effectOf, roundToNanosecond } from "../shared";
import { NameField } from "../sequence/NameField";
import { EffectDefinitionPicker, effectReferencesEqual } from "../sequence/definitionMenu";
import { TypedParamInput } from "../sequence/params/TypedParamInput";
import { MIN_CLIP_DURATION_SECONDS } from "../sequence/sequenceSelection";

/** What the clip fields read of the open sequence or console. */
export type ClipDocument = Pick<SequenceGuiDocument, "layers" | "targets" | "effectDefinitions" | "curveLibrary" | "gradientLibrary"> & {
  markCollections: Pick<SequenceMarkCollection, "id" | "name">[];
};

/** How a sequence or a console shows its clips' fields. */
export type ClipFieldsAdapter = {
  document: ClipDocument;
  /** What links a parameter of the one shown clip to what sets it: envelopes in a sequence, controls in a console. */
  paramControl: (clip: GuiClip, param: SequenceClipParam) => Pick<ComponentProps<typeof TypedParamInput>, "automation" | "automationControl" | "linked">;
};

/** The select value shown while the clips differ; never a real layer, target or scope. */
const MIXED = "";
const SCOPES: SelectOption<SequenceClipScope>[] = [
  { value: "perFixture", label: "Per fixture" },
  { value: "wholeTarget", label: "Whole target" }
];
/** Indents a target under its group in a menu, where plain spaces collapse. */
const INDENT = "  ";
/** The step a clip time scrubs by, in seconds. */
const TIME_STEP_SECONDS = 0.01;

/** Write `field` of every clip of `clips` together. */
function putClips(clips: GuiClip[], field: GuiField, value: GuiValue) {
  return put(clips.map((clip) => site({ type: "clip", id: clip.id }, field)), value);
}

/** The value every clip shares, or null while they differ. */
function common<T>(clips: GuiClip[], read: (clip: GuiClip) => T): T | null {
  const first = clips[0];
  if (first === undefined) return null;
  const value = read(first);
  return clips.every((clip) => Object.is(read(clip), value)) ? value : null;
}

/** The effect clips' fields, edited together: one clip's name, description and effect, and every clip's layer,
 * target, scope, timing and parameters, a field the clips differ in showing "Multiple". */
export function ClipFields({ clips, details, adapter }: { clips: GuiClip[]; details: Map<number, SequenceClipDetails> | null; adapter: ClipFieldsAdapter }) {
  const { document } = adapter;
  const members: GuiMember[] = clips.map((clip) => ({ type: "clip", id: clip.id }));
  const problems = {
    effect: useFieldProblem(members, { type: "definition" }),
    layer: useFieldProblem(members, { type: "layer" }),
    target: useFieldProblem(members, { type: "target" }),
    scope: useFieldProblem(members, { type: "scope" }),
    start: useFieldProblem(members, { type: "start" }),
    duration: useFieldProblem(members, { type: "duration" })
  };
  const first = clips[0];
  if (first === undefined) return null;
  const single = clips.length === 1 ? first : null;
  const mixed = { value: MIXED, label: "Multiple", disabled: true } as const;
  const options = <T extends string>(value: T | null, all: SelectOption<T>[]) => value === null ? [mixed, ...all] : all;
  const layerId = common(clips, (clip) => String(effectOf(clip).layerId));
  const fixture = common(clips, (clip) => String(clip.target.fixture));
  const scope = common(clips, (clip) => effectOf(clip).scope);
  const timed = clips.flatMap((clip) => clip.placement.type === "timed" ? [clip.placement] : []);
  const startSeconds = timed.length === clips.length ? common(clips, (clip) => clip.placement.type === "timed" ? clip.placement.startSeconds : 0) : null;
  const durationSeconds = timed.length === clips.length ? common(clips, (clip) => clip.placement.type === "timed" ? clip.placement.durationSeconds : 0) : null;
  const targets: SelectOption<string>[] = [
    ...clips.filter((clip, index) => clips.findIndex((other) => other.target.fixture === clip.target.fixture) === index
      && !document.targets.some((lane) => lane.target.fixture === clip.target.fixture))
      .map((clip) => ({ value: String(clip.target.fixture), label: clip.targetLabel, disabled: true })),
    ...document.targets.filter((lane, index) => document.targets.findIndex((other) => other.target.fixture === lane.target.fixture) === index)
      .map((lane) => ({ value: String(lane.target.fixture), label: `${INDENT.repeat(lane.depth)}${lane.label}` }))
  ];
  const effect = effectOf(first);
  const definition = single === null ? undefined : document.effectDefinitions.find((candidate) => effectReferencesEqual(candidate.effect, effect.effect));
  const effectTip = definition?.description ?? null;
  const params = details === null ? [] : single !== null
    ? (details.get(single.id)?.params ?? []).map((param) => ({ param, mixed: false }))
    : commonEditableParams(clips.map((clip) => details.get(clip.id)).filter((clip) => clip !== undefined));
  const item = single === null ? null : { type: "clip", id: single.id } as const;
  return <>
    {single !== null && item !== null && <>
      <NameField name={single.name} label="Name" member={item} commit={(name) => rename(item, name)} />
      <DescriptionField description={single.description} member={item} onCommit={(description) => describe(item, description)} />
      <Field label="Effect" error={problems.effect} {...(effectTip === null ? {} : { tip: effectTip })}>
        <EffectDefinitionPicker definitions={document.effectDefinitions} label={definition?.name ?? effect.effectName}
          onSelect={(next) => void putClips(clips, { type: "definition" }, { type: "effect", initialColor: THEME_COLORS.white, effect: next.effect })} />
      </Field>
    </>}
    <Field label="Layer" error={problems.layer}>
      <Select name="Layer" invalid={problems.layer !== null} value={layerId ?? MIXED}
        options={options(layerId, document.layers.map((layer) => ({ value: String(layer.id), label: layer.name })))}
        onChange={(value) => { if (value !== MIXED) void putClips(clips, { type: "layer" }, { type: "id", id: Number(value) }); }} />
    </Field>
    <Field label="Target" error={problems.target}>
      <Select name="Target" invalid={problems.target !== null} value={fixture ?? MIXED} options={options(fixture, targets)}
        onChange={(value) => { if (value !== MIXED) void putClips(clips, { type: "target" }, { type: "target", target: { fixture: Number(value) } }); }} />
    </Field>
    <Field label="Scope" error={problems.scope}>
      <Select<SequenceClipScope | typeof MIXED> name="Scope" invalid={problems.scope !== null} value={scope ?? MIXED} options={options(scope, SCOPES)}
        onChange={(value) => { if (value !== MIXED) void putClips(clips, { type: "scope" }, { type: "scope", scope: value }); }} />
    </Field>
    {timed.length === clips.length && <>
      <TimeField name="Start" min={0} value={startSeconds ?? timed[0]?.startSeconds ?? 0} mixed={startSeconds === null} problem={problems.start}
        onCommit={(seconds) => void putClips(clips, { type: "start" }, { type: "seconds", seconds })} />
      <TimeField name="Duration" min={MIN_CLIP_DURATION_SECONDS} value={durationSeconds ?? timed[0]?.durationSeconds ?? 0} mixed={durationSeconds === null} problem={problems.duration}
        onCommit={(seconds) => void putClips(clips, { type: "duration" }, { type: "seconds", seconds })} />
    </>}
    {params.length > 0 && <Section title={single === null ? "Shared parameters" : "Parameters"}>
      {params.map(({ param, mixed: differs }, index) => <div key={`${members.map((member) => member.id).join(",")}:${param.name}`} className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}>
        <ParamInput param={param} mixed={differs} document={document} members={members}
          commitParam={(name, value) => putClips(clips, { type: "param", name }, { type: "param", value }).then(() => undefined)}
          control={single === null ? {} : adapter.paramControl(single, param)} />
      </div>)}
    </Section>}
  </>;
}

function TimeField({ name, value, min, mixed, problem, onCommit }: { name: string; value: number; min: number; mixed: boolean; problem: string | null; onCommit: (seconds: number) => void }) {
  return <Field label={name} error={problem}>
    <NumberField name={name} value={value} min={min} step={TIME_STEP_SECONDS} unit="s" invalid={problem !== null}
      {...(mixed ? { format: () => "Multiple" } : {})}
      onCommit={(seconds) => { onCommit(Math.max(min, roundToNanosecond(seconds))); }} />
  </Field>;
}

/** The parameters every clip has editable with the same kind; an enum offers the options every clip has. */
function commonEditableParams(clips: SequenceClipDetails[]): { param: SequenceClipParam; mixed: boolean }[] {
  const first = clips[0];
  if (first === undefined) return [];
  const kind = (param: SequenceClipParam) => JSON.stringify(param.kind);
  return first.params.flatMap((param) => {
    if (!param.editable) return [];
    const matches = clips.slice(1).map((clip) => clip.params.find((candidate) =>
      candidate.name === param.name && kind(candidate) === kind(param) && candidate.editable
    ));
    if (matches.some((match) => match === undefined)) return [];
    const options = param.kind.type === "enum"
      ? param.options.filter((option) => matches.every((match) => match !== undefined && match.options.some((candidate) => candidate.name === option.name)))
      : param.options;
    const [firstOption] = options;
    if (param.kind.type === "enum" && firstOption === undefined) return [];
    const mixed = matches.some((match) => JSON.stringify(match?.value) !== JSON.stringify(param.value));
    const chosen = param.value.type === "enum" ? param.value.value : null;
    const shown = firstOption !== undefined && chosen !== null && !options.some((option) => option.name === chosen)
      ? { ...param, options, value: { type: "enum" as const, value: firstOption.name, fields: firstOption.fields.map((field) => ({ name: field.name, value: field.value })) } }
      : { ...param, options };
    return [{ param: shown, mixed }];
  });
}

/** One parameter; while the clips' values differ, a button that edits them together. */
function ParamInput({ param, mixed, document, members, commitParam, control }: {
  param: SequenceClipParam;
  mixed: boolean;
  document: ClipDocument;
  members: GuiMember[];
  commitParam: (name: string, value: SequenceClipParam["value"]) => Promise<void>;
  control: ReturnType<ClipFieldsAdapter["paramControl"]>;
}) {
  const [editingMixed, setEditingMixed] = useState(false);
  const problem = useFieldProblem(mixed && !editingMixed ? members : [], { type: "param", name: param.name });
  if (mixed && !editingMixed) {
    return <Field label={param.name} tip={param.description ?? undefined} error={problem}>
      <Button tip="Edit every selected clip's value" onClick={() => { setEditingMixed(true); }}>Multiple</Button>
    </Field>;
  }
  return <>
    <TypedParamInput param={param} commitParam={commitParam} curveLibrary={document.curveLibrary} gradientLibrary={document.gradientLibrary}
      markCollections={document.markCollections} member={members} {...control} />
    {mixed && <IconButton icon={CheckCheck} label="Apply shown value to all" onClick={() => { void commitParam(param.name, param.value); }} />}
  </>;
}

/** An envelope's name, description and timing, and how many parameters it drives; its curve is drawn and edited
 * on a sequence's timeline. */
export function EnvelopeFields({ clip }: { clip: GuiClip }) {
  const item = { type: "clip", id: clip.id } as const;
  const problems = { start: useFieldProblem(item, { type: "start" }), duration: useFieldProblem(item, { type: "duration" }) };
  const bindings = clip.content.type === "envelope" ? clip.content.bindings.length : 0;
  return <>
    <NameField name={clip.name} label="Name" member={item} commit={(name) => rename(item, name)} />
    <DescriptionField description={clip.description} member={item} onCommit={(description) => describe(item, description)} />
    {clip.placement.type === "timed" && <>
      <TimeField name="Start" min={0} value={clip.placement.startSeconds} mixed={false} problem={problems.start}
        onCommit={(seconds) => void putClips([clip], { type: "start" }, { type: "seconds", seconds })} />
      <TimeField name="Duration" min={MIN_CLIP_DURATION_SECONDS} value={clip.placement.durationSeconds} mixed={false} problem={problems.duration}
        onCommit={(seconds) => void putClips([clip], { type: "duration" }, { type: "seconds", seconds })} />
    </>}
    <div className="inspector-readout-grid"><Readout label="Bindings" value={String(bindings)} /></div>
  </>;
}

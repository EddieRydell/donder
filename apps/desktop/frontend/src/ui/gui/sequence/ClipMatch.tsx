import { useEffect, useState } from "react";
import type { SequenceClipMatch, SequenceClipMatchRequest, SequenceClipParamMatch, SequenceClipTimeMatch, SequenceGuiDocument, SequenceClipParam, SequenceParamKind, SequenceSelection } from "../../../types";
import { Button, ContextMenuEntries, Dialog, Field, NumberField, Section, Toggle, type MenuEntry } from "../../kit";
import { useSequenceClipDetails } from "./sequenceClipDetails";
import { clipIdsOfKind } from "./sequenceSelection";

export type ClipMatchSeed = Pick<SequenceClipMatchRequest, "clips">;
export type MatchClips = (seed: ClipMatchSeed, criteria: SequenceClipMatch, params: SequenceClipParamMatch[]) => Promise<SequenceSelection>;

type TimeKey = "start" | "end" | "duration";
const TIME_CRITERIA: { key: TimeKey; label: string }[] = [
  { key: "start", label: "Start" },
  { key: "end", label: "End" },
  { key: "duration", label: "Duration" }
];
type FlagKey = "effectType" | "sameLane" | "automationClips";
const FLAG_CRITERIA: { key: FlagKey; label: string }[] = [
  { key: "effectType", label: "Effect type" },
  { key: "sameLane", label: "Same lane" },
  { key: "automationClips", label: "Automation clips" }
];
const NUMERIC_KINDS = new Set<SequenceParamKind>(["int", "float", "intArray", "floatArray"]);
const MS_PER_SECOND = 1000;
const TIME_TOLERANCE_STEP_MS = 1;
const PARAM_TOLERANCE_STEP = 0.01;

function toleranceLabel(time: SequenceClipTimeMatch) {
  return `±${Number((time.toleranceSeconds * MS_PER_SECOND).toPrecision(6))} ms`;
}

type ClipMatchMenuProps = {
  criteria: SequenceClipMatch;
  onCriteria: (criteria: SequenceClipMatch) => void;
  onSelectMatching: () => void;
  onOptions: () => void;
};

/** The "Select Matching" submenu: run with the saved criteria, toggle them, or open Options. */
export function clipMatchEntries({ criteria, onCriteria, onSelectMatching, onOptions }: ClipMatchMenuProps): MenuEntry[] {
  const flag = (key: FlagKey): MenuEntry => {
    const label = FLAG_CRITERIA.find((item) => item.key === key)?.label ?? key;
    return { type: "check", label, checked: criteria[key], onCheckedChange: (checked) => { onCriteria({ ...criteria, [key]: checked }); } };
  };
  return [
    { type: "action", label: "Select Matching", shortcut: "sequence.selectMatching", onSelect: onSelectMatching },
    { type: "separator" },
    flag("effectType"),
    ...TIME_CRITERIA.map(({ key, label }): MenuEntry => ({
      type: "check",
      label,
      detail: toleranceLabel(criteria[key]),
      checked: criteria[key].enabled,
      onCheckedChange: (enabled) => { onCriteria({ ...criteria, [key]: { ...criteria[key], enabled } }); }
    })),
    flag("sameLane"),
    flag("automationClips"),
    { type: "separator" },
    { type: "action", label: "Options…", onSelect: onOptions }
  ];
}

/** `clipMatchEntries` inside an open context menu. */
export function ClipMatchMenuItems(props: ClipMatchMenuProps) {
  return <ContextMenuEntries entries={clipMatchEntries(props)} />;
}

function paramMatches(params: Map<string, number>): SequenceClipParamMatch[] {
  return [...params].map(([name, tolerance]) => ({ name, tolerance }));
}

/** Each parameter name the seed clips have, first declaration first. */
function seedParams(details: Iterable<{ params: SequenceClipParam[] }>): SequenceClipParam[] {
  const byName = new Map<string, SequenceClipParam>();
  for (const clip of details) {
    for (const param of clip.params) if (!byName.has(param.name)) byName.set(param.name, param);
  }
  return [...byName.values()];
}

/**
 * Every criterion with its tolerance, plus the seed clips' parameters, with a live count of the
 * matching clips. Parameter picks belong to this dialog; the other criteria are saved on Select.
 */
export function ClipMatchDialog({ document, seed, criteria: savedCriteria, match, onSelect, onClose }: {
  document: SequenceGuiDocument;
  seed: ClipMatchSeed;
  criteria: SequenceClipMatch;
  match: MatchClips;
  onSelect: (criteria: SequenceClipMatch, params: SequenceClipParamMatch[]) => void;
  onClose: () => void;
}) {
  const [criteria, setCriteria] = useState(savedCriteria);
  const [params, setParams] = useState<Map<string, number>>(new Map());
  // The count is current only for the criteria it was computed from.
  const [result, setResult] = useState<{ criteria: SequenceClipMatch; params: Map<string, number>; count: number } | null>(null);
  const count = result !== null && result.criteria === criteria && result.params === params ? result.count : null;
  const [error, setError] = useState<string | null>(null);
  const details = useSequenceClipDetails(document, clipIdsOfKind(seed.clips, "clip"));
  const available = details === null ? [] : seedParams(details.values());

  useEffect(() => {
    let cancelled = false;
    match(seed, criteria, paramMatches(params))
      .then((selection) => {
        if (cancelled) return;
        setResult({ criteria, params, count: selection.type === "clips" ? selection.clips.length : 0 });
        setError(null);
      })
      .catch((matchError: unknown) => { if (!cancelled) setError(String(matchError)); });
    return () => { cancelled = true; };
  }, [match, seed, criteria, params]);

  const setParam = (name: string, tolerance: number | null) => {
    const next = new Map(params);
    if (tolerance === null) next.delete(name);
    else next.set(name, tolerance);
    setParams(next);
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => { if (!open) onClose(); }}
      title="Select matching clips"
      size="md"
      actions={<>
        <span className="clip-match-count">{count === null ? "…" : `${count} matching`}</span>
        <Button onClick={onClose}>Cancel</Button>
        <Button variant="primary" disabled={count === null} onClick={() => { onSelect(criteria, paramMatches(params)); }}>Select</Button>
      </>}
    >
      {error !== null && <p role="alert">{error}</p>}
      <Section title="Clip">
        {FLAG_CRITERIA.map(({ key, label }) => (
          <Field key={key} label={label}>
            <Toggle name={label} checked={criteria[key]} onChange={(checked) => { setCriteria({ ...criteria, [key]: checked }); }} />
          </Field>
        ))}
        {TIME_CRITERIA.map(({ key, label }) => (
          <ToleranceRow
            key={key}
            label={label}
            tip={undefined}
            enabled={criteria[key].enabled}
            tolerance={criteria[key].toleranceSeconds * MS_PER_SECOND}
            step={TIME_TOLERANCE_STEP_MS}
            unit="ms"
            onEnabled={(enabled) => { setCriteria({ ...criteria, [key]: { ...criteria[key], enabled } }); }}
            onTolerance={(tolerance) => { setCriteria({ ...criteria, [key]: { ...criteria[key], toleranceSeconds: tolerance / MS_PER_SECOND } }); }}
          />
        ))}
      </Section>
      {available.length > 0 && (
        <Section title="Parameters">
          {available.map((param) => {
            const tolerance = params.get(param.name);
            const toggle = (enabled: boolean) => { setParam(param.name, enabled ? 0 : null); };
            return NUMERIC_KINDS.has(param.kind)
              ? <ToleranceRow key={param.name} label={param.name} tip={param.description ?? undefined} enabled={tolerance !== undefined} tolerance={tolerance ?? 0}
                step={PARAM_TOLERANCE_STEP} unit={undefined} onEnabled={toggle} onTolerance={(value) => { setParam(param.name, value); }} />
              : <Field key={param.name} label={param.name} tip={param.description ?? undefined}>
                <Toggle name={param.name} checked={tolerance !== undefined} onChange={toggle} />
              </Field>;
          })}
        </Section>
      )}
    </Dialog>
  );
}

function ToleranceRow({ label, tip, enabled, tolerance, step, unit, onEnabled, onTolerance }: {
  label: string;
  tip: string | undefined;
  enabled: boolean;
  tolerance: number;
  step: number;
  unit: string | undefined;
  onEnabled: (enabled: boolean) => void;
  onTolerance: (tolerance: number) => void;
}) {
  return <Field label={label} tip={tip}>
    <Toggle name={label} checked={enabled} onChange={onEnabled} />
    <NumberField
      name={`${label} tolerance`}
      label="±"
      value={Number(tolerance.toPrecision(6))}
      min={0}
      step={step}
      {...(unit === undefined ? {} : { unit })}
      disabled={!enabled}
      onCommit={onTolerance}
    />
  </Field>;
}

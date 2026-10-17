import * as ContextMenu from "@radix-ui/react-context-menu";
import * as Dialog from "@radix-ui/react-dialog";
import { Check } from "lucide-react";
import { useEffect, useState } from "react";
import type { SequenceClipMatch, SequenceClipParamMatch, SequenceClipTimeMatch, SequenceEditorDocument, SequenceEffectParam, SequenceEffectParamKind, SequenceSelection } from "../../../editor/types";
import { formatShortcut } from "../../../platform";
import { EDITOR_SHORTCUTS } from "../../../shortcuts";
import { THEME_METRICS } from "../../../theme";
import { useSequenceEffectDetails } from "./sequenceEffectDetails";

export type ClipMatchSeed = { effectIds: number[]; automationIds: number[] };
export type MatchClips = (seed: ClipMatchSeed, criteria: SequenceClipMatch, params: SequenceClipParamMatch[]) => Promise<SequenceSelection>;

type TimeKey = "start" | "end" | "duration";
const TIME_CRITERIA: { key: TimeKey; label: string }[] = [
  { key: "start", label: "Start" },
  { key: "end", label: "End" },
  { key: "duration", label: "Duration" }
];
type FlagKey = "effectType" | "sameLane" | "automationClips";
const FLAG_CRITERIA: Record<FlagKey, string> = {
  effectType: "Effect type",
  sameLane: "Same lane",
  automationClips: "Automation clips"
};
const NUMERIC_KINDS = new Set<SequenceEffectParamKind>(["int", "float", "intArray", "floatArray"]);
const MS_PER_SECOND = 1000;

function toleranceLabel(time: SequenceClipTimeMatch) {
  return `±${Number((time.toleranceSeconds * MS_PER_SECOND).toPrecision(6))} ms`;
}

/** The "Select Matching" submenu: run with the saved criteria, toggle them, or open Options. */
export function ClipMatchMenuItems({ criteria, onCriteria, onSelectMatching, onOptions }: {
  criteria: SequenceClipMatch;
  onCriteria: (criteria: SequenceClipMatch) => void;
  onSelectMatching: () => void;
  onOptions: () => void;
}) {
  // Toggling a criterion keeps the menu open so several can change at once.
  const checkbox = (key: string, label: string, detail: string | null, checked: boolean, next: SequenceClipMatch) => (
    <ContextMenu.CheckboxItem key={key} className="menu-item" checked={checked} onSelect={(event) => { event.preventDefault(); }} onCheckedChange={() => { onCriteria(next); }}>
      <span>{label}</span>
      {detail !== null && <span className="shortcut">{detail}</span>}
      <ContextMenu.ItemIndicator>
        <Check size={THEME_METRICS.iconSizeExtraSmall} />
      </ContextMenu.ItemIndicator>
    </ContextMenu.CheckboxItem>
  );
  return <>
    <ContextMenu.Item className="menu-item" onSelect={onSelectMatching}>
      <span>Select Matching</span>
      <span className="shortcut">{EDITOR_SHORTCUTS["sequence.selectMatching"].map(formatShortcut).join(" / ")}</span>
    </ContextMenu.Item>
    <ContextMenu.Separator className="menu-separator" />
    {checkbox("effectType", FLAG_CRITERIA.effectType, null, criteria.effectType, { ...criteria, effectType: !criteria.effectType })}
    {TIME_CRITERIA.map(({ key, label }) => checkbox(key, label, toleranceLabel(criteria[key]), criteria[key].enabled, { ...criteria, [key]: { ...criteria[key], enabled: !criteria[key].enabled } }))}
    {checkbox("sameLane", FLAG_CRITERIA.sameLane, null, criteria.sameLane, { ...criteria, sameLane: !criteria.sameLane })}
    {checkbox("automationClips", FLAG_CRITERIA.automationClips, null, criteria.automationClips, { ...criteria, automationClips: !criteria.automationClips })}
    <ContextMenu.Separator className="menu-separator" />
    <ContextMenu.Item className="menu-item" onSelect={onOptions}>Options…</ContextMenu.Item>
  </>;
}

function paramMatches(params: Map<string, number>): SequenceClipParamMatch[] {
  return [...params].map(([name, tolerance]) => ({ name, tolerance }));
}

/** Each parameter name the seed effects have, first declaration first. */
function seedParams(details: Iterable<{ params: SequenceEffectParam[] }>): SequenceEffectParam[] {
  const byName = new Map<string, SequenceEffectParam>();
  for (const effect of details) {
    for (const param of effect.params) if (!byName.has(param.name)) byName.set(param.name, param);
  }
  return [...byName.values()];
}

/**
 * Every criterion with its tolerance, plus the seed effects' parameters, with a live count of the
 * matching clips. Parameter picks belong to this dialog; the other criteria are saved on Select.
 */
export function ClipMatchDialog({ document, seed, criteria: savedCriteria, match, onSelect, onClose }: {
  document: SequenceEditorDocument;
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
  const details = useSequenceEffectDetails(document, seed.effectIds);
  const available = details === null ? [] : seedParams(details.values());

  useEffect(() => {
    let cancelled = false;
    match(seed, criteria, paramMatches(params))
      .then((selection) => {
        if (cancelled) return;
        setResult({ criteria, params, count: selection.type === "clips" ? selection.effectIds.length + selection.automationIds.length : 0 });
        setError(null);
      })
      .catch((matchError: unknown) => { if (!cancelled) setError(String(matchError)); });
    return () => { cancelled = true; };
  }, [match, seed, criteria, params]);

  const setTime = (key: TimeKey, time: SequenceClipTimeMatch) => { setCriteria({ ...criteria, [key]: time }); };
  const setFlag = (key: FlagKey, value: boolean) => { setCriteria({ ...criteria, [key]: value }); };
  const setParam = (name: string, tolerance: number | null) => {
    const next = new Map(params);
    if (tolerance === null) next.delete(name);
    else next.set(name, tolerance);
    setParams(next);
  };

  return (
    <Dialog.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content clip-options-dialog">
          <Dialog.Title>Select matching clips</Dialog.Title>
          <Dialog.Description>
            Select every clip that matches a selected clip on each checked property.
          </Dialog.Description>
          {error !== null && <p role="alert">{error}</p>}
          <fieldset className="clip-options">
            <legend>Clip</legend>
            <label><input type="checkbox" checked={criteria.effectType} onChange={(event) => { setFlag("effectType", event.currentTarget.checked); }} />{FLAG_CRITERIA.effectType}</label>
            <span />
            {TIME_CRITERIA.map(({ key, label }) => (
              <ToleranceRow
                key={key}
                label={label}
                enabled={criteria[key].enabled}
                tolerance={criteria[key].toleranceSeconds * MS_PER_SECOND}
                unit="ms"
                onEnabled={(enabled) => { setTime(key, { ...criteria[key], enabled }); }}
                onTolerance={(tolerance) => { setTime(key, { ...criteria[key], toleranceSeconds: tolerance / MS_PER_SECOND }); }}
              />
            ))}
            <label><input type="checkbox" checked={criteria.sameLane} onChange={(event) => { setFlag("sameLane", event.currentTarget.checked); }} />{FLAG_CRITERIA.sameLane}</label>
            <span />
            <label><input type="checkbox" checked={criteria.automationClips} onChange={(event) => { setFlag("automationClips", event.currentTarget.checked); }} />{FLAG_CRITERIA.automationClips}</label>
            <span />
          </fieldset>
          <fieldset className="clip-options">
            <legend>Parameters</legend>
            {details === null && <p>Loading parameters…</p>}
            {details !== null && available.length === 0 && <p>The selected effects have no parameters.</p>}
            {available.map((param) => {
              const tolerance = params.get(param.name);
              const toggle = (enabled: boolean) => { setParam(param.name, enabled ? 0 : null); };
              return NUMERIC_KINDS.has(param.kind)
                ? <ToleranceRow key={param.name} label={param.name} title={param.description ?? undefined} enabled={tolerance !== undefined} tolerance={tolerance ?? 0} unit={null} onEnabled={toggle} onTolerance={(value) => { setParam(param.name, value); }} />
                : <ParamRow key={param.name} param={param} enabled={tolerance !== undefined} onEnabled={toggle} />;
            })}
          </fieldset>
          <div className="dialog-actions">
            <span className="clip-match-count">{count === null ? "Matching…" : `${count} matching clip${count === 1 ? "" : "s"}`}</span>
            <button type="button" onClick={onClose}>Cancel</button>
            <button type="button" disabled={count === null} onClick={() => { onSelect(criteria, paramMatches(params)); }}>Select</button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function ParamRow({ param, enabled, onEnabled }: { param: SequenceEffectParam; enabled: boolean; onEnabled: (enabled: boolean) => void }) {
  return <>
    <label title={param.description ?? undefined}><input type="checkbox" checked={enabled} onChange={(event) => { onEnabled(event.currentTarget.checked); }} />{param.name}</label>
    <span />
  </>;
}

function ToleranceRow({ label, title, enabled, tolerance, unit, onEnabled, onTolerance }: {
  label: string;
  title?: string | undefined;
  enabled: boolean;
  tolerance: number;
  unit: string | null;
  onEnabled: (enabled: boolean) => void;
  onTolerance: (tolerance: number) => void;
}) {
  return <>
    <label title={title}><input type="checkbox" checked={enabled} onChange={(event) => { onEnabled(event.currentTarget.checked); }} />{label}</label>
    <label className="clip-options-value">
      ±
      <input
        type="number"
        aria-label={`${label} tolerance`}
        min={0}
        step="any"
        value={Number(tolerance.toPrecision(6))}
        disabled={!enabled}
        onChange={(event) => {
          const value = event.currentTarget.valueAsNumber;
          if (Number.isFinite(value) && value >= 0) onTolerance(value);
        }}
      />
      {unit}
    </label>
  </>;
}

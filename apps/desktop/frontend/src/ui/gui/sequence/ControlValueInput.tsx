import type { SequenceControlOptions, SequenceControlValue } from "../../../types";
import { THEME_COLORS } from "../../../theme";
import { ColorPicker } from "../../ColorPicker";
import { NumberField } from "../setup/PatchInputs";
import { CurveParam, GradientParam } from "./params/TypedParamInput";

export function initialControlValue(options: SequenceControlOptions): SequenceControlValue {
  switch (options.type) {
    case "normalized": return { type: "constantNormalized", value: 1 };
    case "color": return { type: "constantColor", value: THEME_COLORS.white };
    case "indexed": {
      const option = options.options[0];
      if (option === undefined) throw new Error("Indexed control has no options.");
      return { type: "indexed", option: option.id, rangeCurve: null };
    }
    case "fixtureIndexed": {
      const entry = options.entries[0];
      if (entry === undefined) throw new Error("Fixture control has no entries.");
      return { type: "fixtureIndexed", entry: entry.id, rangeCurve: null };
    }
  }
}

export function ControlValueInput({ options, value, disabled, onChange }: { options: SequenceControlOptions; value: SequenceControlValue; disabled: boolean; onChange: (value: SequenceControlValue) => void }) {
  const change = (next: SequenceControlValue) => { if (!disabled) onChange(next); };
  const commitDraft = (next: SequenceControlValue) => { change(next); return Promise.resolve(); };
  return <div className="control-value-editor">
    {options.type === "normalized" && <label>Value<select value={value.type} onChange={(event) => {
      if (event.target.value === "constantNormalized") change({ type: "constantNormalized", value: 1 });
      if (event.target.value === "normalizedCurve") change({ type: "normalizedCurve", points: [{ time: 0, value: 0 }, { time: 1, value: 1 }] });
    }}><option value="constantNormalized">Constant level</option><option value="normalizedCurve">Level curve</option></select></label>}
    {value.type === "constantNormalized" && <NumberField label="Level (0–1)" value={value.value} max={1} step="any" onChange={(value) => { change({ type: "constantNormalized", value }); }} />}
    {value.type === "normalizedCurve" && <CurveParam name="Level over clip duration" points={value.points} readOnly={disabled} commit={(points) => commitDraft({ ...value, points })} />}
    {options.type === "indexed" && value.type === "indexed" && <label>Option<select value={value.option} onChange={(event) => { change({ ...value, option: Number(event.target.value) }); }}>
      {options.options.map((option) => <option key={option.id} value={option.id}>{option.name} ({option.id})</option>)}
    </select></label>}
    {options.type === "fixtureIndexed" && value.type === "fixtureIndexed" && <>
      <label>Entry<select value={value.entry} onChange={(event) => { change({ type: "fixtureIndexed", entry: Number(event.target.value), rangeCurve: null }); }}>
        {options.entries.map((entry) => <option key={entry.id} value={entry.id}>{entry.name} ({entry.id})</option>)}
      </select></label>
      {options.entries.some((entry) => entry.id === value.entry && entry.rangeControl) && <label><input type="checkbox" checked={value.rangeCurve !== null} onChange={(event) => { change({ ...value, rangeCurve: event.target.checked ? [{ time: 0, value: 0 }, { time: 1, value: 1 }] : null }); }} />Animate within entry range</label>}
      {value.rangeCurve !== null && <CurveParam name="Entry range over clip duration" points={value.rangeCurve} readOnly={disabled} commit={(rangeCurve) => commitDraft({ ...value, rangeCurve })} />}
    </>}
    {options.type === "color" && <label>Value<select value={value.type} onChange={(event) => {
      if (event.target.value === "constantColor") change({ type: "constantColor", value: THEME_COLORS.white });
      if (event.target.value === "gradient") change({ type: "gradient", stops: [{ time: 0, value: THEME_COLORS.white }, { time: 1, value: THEME_COLORS.white }] });
    }}><option value="constantColor">Constant color</option><option value="gradient">Color gradient</option></select></label>}
    {value.type === "constantColor" && (disabled ? <span>{value.value}</span> : <ColorPicker label="Control color" value={value.value} commit={(value) => commitDraft({ type: "constantColor", value })} />)}
    {value.type === "gradient" && <GradientParam name="Color over clip duration" points={value.stops} readOnly={disabled} commit={(stops) => commitDraft({ ...value, stops })} />}
  </div>;
}

export function controlValueLabel(value: SequenceControlValue): string {
  switch (value.type) {
    case "constantNormalized": return `Level ${Math.round(value.value * 100)}%`;
    case "normalizedCurve": return `Level curve (${value.points.length} points)`;
    case "indexed": return `Option ${value.option}`;
    case "fixtureIndexed": return `Entry ${value.entry}${value.rangeCurve === null ? "" : " with range curve"}`;
    case "constantColor": return value.value;
    case "gradient": return `Gradient (${value.stops.length} stops)`;
  }
}

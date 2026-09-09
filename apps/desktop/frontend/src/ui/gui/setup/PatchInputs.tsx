import { navigateToGuiObject } from "../../../workspace/navigation";
import type { GuiColorCapability, GuiDimmingCurve, GuiObjectRef, PatchGuiIndexedEntry } from "../../../types";

export function NumberField({ label, value, onChange, min = 0, max, step = 1 }: { label: string; value: number; onChange: (value: number) => void; min?: number; max?: number; step?: number | "any" }) {
  return <label>{label}<input type="number" min={min} max={max} step={step} required value={value} onChange={(event) => { onChange(Number(event.target.value)); }} /></label>;
}

export function ReferenceInput({ label, value, choices, onChange }: { label: string; value: GuiObjectRef; choices: GuiObjectRef[]; onChange: (value: GuiObjectRef) => void }) {
  const index = choices.findIndex((choice) => choice.moduleId === value.moduleId && choice.path === value.path && choice.objectKey === value.objectKey);
  return <div><label>{label}<select value={index} onChange={(event) => { const next = choices[Number(event.target.value)]; if (next !== undefined) onChange(next); }}>
    {index < 0 && <option value={-1} disabled>{value.path}#{value.objectKey} (unavailable)</option>}
    {choices.map((choice, index) => <option key={index} value={index}>{choice.path}#{choice.objectKey}</option>)}
  </select></label><a href="#" onClick={(event) => { event.preventDefault(); void navigateToGuiObject(value); }}>Edit source</a></div>;
}

export function MappingInput({ value, onChange }: { value: PatchGuiIndexedEntry[]; onChange: (value: PatchGuiIndexedEntry[]) => void }) {
  return <div className="setup-patch-list">
    {value.map((entry, index) => <div className="setup-patch-fields" key={index}>
      <NumberField label="Identifier" value={entry.id} onChange={(id) => { onChange(value.map((entry, i) => i === index ? { ...entry, id } : entry)); }} />
      <NumberField label="Value" value={entry.value} step="any" onChange={(valueAtId) => { onChange(value.map((entry, i) => i === index ? { ...entry, value: valueAtId } : entry)); }} />
      <button type="button" onClick={() => { onChange(value.filter((_, i) => i !== index)); }}>Remove mapping</button>
    </div>)}
    <button type="button" onClick={() => { onChange([...value, { id: Math.max(-1, ...value.map((entry) => entry.id)) + 1, value: 0 }]); }}>Add mapping</button>
  </div>;
}

export function DimmingCurveInput({ value, onChange }: { value: GuiDimmingCurve; onChange: (value: GuiDimmingCurve) => void }) {
  return <div className="setup-patch-fields">
    <label>Curve<select value={value.type} onChange={(event) => {
      switch (event.target.value) {
        case "linear": onChange({ type: "linear" }); break;
        case "gamma": onChange({ type: "gamma", exponent: 2.2 }); break;
        case "custom": onChange({ type: "custom", points: [{ time: 0, value: 0 }, { time: 1, value: 1 }] }); break;
      }
    }}><option value="linear">Linear</option><option value="gamma">Gamma</option><option value="custom">Custom points</option></select></label>
    {value.type === "gamma" && <NumberField label="Gamma exponent" value={value.exponent} step="any" onChange={(exponent) => { onChange({ ...value, exponent }); }} />}
    {value.type === "custom" && <div className="setup-patch-list">
      {value.points.map((point, index) => <div className="setup-patch-fields" key={index}>
        <NumberField label="Input (0–1)" value={point.time} step="any" onChange={(time) => { onChange({ ...value, points: value.points.map((point, i) => i === index ? { ...point, time } : point) }); }} />
        <NumberField label="Output (0–1)" value={point.value} step="any" onChange={(level) => { onChange({ ...value, points: value.points.map((point, i) => i === index ? { ...point, value: level } : point) }); }} />
        <button type="button" onClick={() => { onChange({ ...value, points: value.points.filter((_, i) => i !== index) }); }}>Remove point</button>
      </div>)}
      <button type="button" onClick={() => { onChange({ ...value, points: [...value.points, { time: 1, value: 1 }] }); }}>Add curve point</button>
    </div>}
  </div>;
}

export function ColorCapabilityInput({ value, onChange }: { value: GuiColorCapability; onChange: (value: GuiColorCapability) => void }) {
  return <div className="setup-patch-list">
    <label>Color capability<select value={value.type} onChange={(event) => {
      switch (event.target.value) {
        case "rgb": onChange({ type: "rgb" }); break;
        case "rgbw": onChange({ type: "rgbw" }); break;
        case "discrete": onChange({ type: "discrete", emitters: [], mappings: [] }); break;
      }
    }}><option value="rgb">RGB</option><option value="rgbw">RGBW</option><option value="discrete">Discrete emitters</option></select></label>
    {value.type === "discrete" && <>
      <p>Mappings match colors exactly. Include black for inactive pixels and every color your effects produce. Fades can produce additional colors that need mappings.</p>
      {value.emitters.map((emitter, index) => <div className="setup-patch-fields" key={index}>
        <NumberField label="Emitter ID" value={emitter.id} onChange={(id) => { onChange({ ...value, emitters: value.emitters.map((emitter, i) => i === index ? { ...emitter, id } : emitter) }); }} />
        <label>Name<input value={emitter.name} onChange={(event) => { onChange({ ...value, emitters: value.emitters.map((emitter, i) => i === index ? { ...emitter, name: event.target.value } : emitter) }); }} /></label>
        <button type="button" onClick={() => { onChange({ ...value, emitters: value.emitters.filter((_, i) => i !== index) }); }}>Remove emitter</button>
      </div>)}
      <button type="button" onClick={() => { onChange({ ...value, emitters: [...value.emitters, { id: Math.max(-1, ...value.emitters.map((emitter) => emitter.id)) + 1, name: "New emitter" }] }); }}>Add emitter</button>
      {value.mappings.map((mapping, index) => <div className="setup-patch-list" key={index}>
        <label>Color (hex)<input required value={mapping.color} onChange={(event) => { onChange({ ...value, mappings: value.mappings.map((mapping, i) => i === index ? { ...mapping, color: event.target.value } : mapping) }); }} /></label>
        <MappingInput value={mapping.levels} onChange={(levels) => { onChange({ ...value, mappings: value.mappings.map((mapping, i) => i === index ? { ...mapping, levels } : mapping) }); }} />
        <button type="button" onClick={() => { onChange({ ...value, mappings: value.mappings.filter((_, i) => i !== index) }); }}>Remove color</button>
      </div>)}
      <button type="button" onClick={() => { onChange({ ...value, mappings: [...value.mappings, { color: "", levels: value.emitters.map((emitter) => ({ id: emitter.id, value: 0 })) }] }); }}>Add color mapping</button>
    </>}
  </div>;
}

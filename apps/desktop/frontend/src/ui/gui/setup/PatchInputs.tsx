import { navigateToGuiObject } from "../../../workspace/navigation";
import type { GuiObjectRef } from "../../../types";

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

import { guiObjectKey, sameGuiObject } from "../../../workspace/guiIdentity";
import { navigateToGuiObject } from "../../../workspace/navigation";
import type { GuiObjectRef } from "../../../types";

export function NumberField({ label, value, onChange, min = 0, max, step = 1 }: { label: string; value: number; onChange: (value: number) => void; min?: number; max?: number; step?: number | "any" }) {
  return <label>{label}<input type="number" min={min} max={max} step={step} required value={value} onChange={(event) => { onChange(Number(event.target.value)); }} /></label>;
}

export function ReferenceInput({ label, value, choices, onChange }: { label: string; value: GuiObjectRef; choices: { reference: GuiObjectRef; label: string }[]; onChange: (value: GuiObjectRef) => void }) {
  const index = choices.findIndex((choice) => sameGuiObject(choice.reference, value));
  return <div><label>{label}<select value={index} onChange={(event) => { const next = choices[Number(event.target.value)]; if (next !== undefined) onChange(next.reference); }}>
    {index < 0 && <option value={-1} disabled>{referenceLabel(value)} (unavailable)</option>}
    {choices.map((choice, index) => <option key={guiObjectKey(choice.reference)} value={index}>{choice.label}</option>)}
  </select></label><a href="#" onClick={(event) => { event.preventDefault(); void navigateToGuiObject(value); }}>Open source</a></div>;
}

function referenceLabel(reference: GuiObjectRef): string {
  return reference.ownedPath.length > 0 ? `${reference.kind} in ${reference.path}` : `${reference.objectKey} (${reference.path})`;
}

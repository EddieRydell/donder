import type { Transform } from "../../../types";

export function CoordinateField({ label, value, min, max, step = "any", onChange }: { label: string; value: number; min?: number; max?: number; step?: number | "any"; onChange: (value: number) => void }) {
  return <label>{label}<input key={value} type="number" required step={step} min={min} max={max} defaultValue={value} onKeyDown={(event) => {
    if (event.key === "Enter") event.currentTarget.blur();
    if (event.key === "Escape") { event.currentTarget.value = String(value); event.currentTarget.blur(); }
  }} onBlur={(event) => {
    if (!event.currentTarget.reportValidity()) { event.currentTarget.value = String(value); return; }
    const next = event.currentTarget.valueAsNumber;
    event.currentTarget.value = String(value);
    if (next !== value) onChange(next);
  }} /></label>;
}

export function Placement({ value, onChange }: { value: Transform; onChange: (value: Transform) => void }) {
  return <div className="placement-fields">
    {(["xMeters", "yMeters", "zMeters"] as const).map((axis) => <CoordinateField key={axis} label={axis.charAt(0).toUpperCase() + " (m)"} value={value.position[axis]} min={-2000} max={2000} onChange={(next) => { onChange({ ...value, position: { ...value.position, [axis]: next } }); }} />)}
    {(["xDegrees", "yDegrees", "zDegrees"] as const).map((axis) => <CoordinateField key={axis} label={"Rotate " + axis.charAt(0).toUpperCase() + " (°)"} value={value.rotation[axis]} onChange={(next) => { onChange({ ...value, rotation: { ...value.rotation, [axis]: next } }); }} />)}
    {(["x", "y", "z"] as const).map((axis) => <CoordinateField key={axis} label={"Scale " + axis.toUpperCase()} value={value.scale[axis]} onChange={(next) => { onChange({ ...value, scale: { ...value.scale, [axis]: next } }); }} />)}
  </div>;
}


import type { Transform } from "../../../types";
import { Field, FieldGroup, NumberField } from "../../kit";

const positionAxes = [["xMeters", "X"], ["yMeters", "Y"], ["zMeters", "Z"]] as const;
const rotationAxes = [["xDegrees", "X"], ["yDegrees", "Y"], ["zDegrees", "Z"]] as const;
const scaleAxes = [["x", "X"], ["y", "Y"], ["z", "Z"]] as const;

export function Placement({ value, onChange, disabled = false }: { value: Transform; onChange: (value: Transform) => void; disabled?: boolean }) {
  return <>
    <Field label="Position">
      <FieldGroup>{positionAxes.map(([axis, label]) => <NumberField key={axis} label={label} name={`Position ${label}`} unit="m" step={0.01} defaultValue={0} min={-2000} max={2000} disabled={disabled} value={value.position[axis]} onCommit={(next) => { onChange({ ...value, position: { ...value.position, [axis]: next } }); }} />)}</FieldGroup>
    </Field>
    <Field label="Rotation">
      <FieldGroup>{rotationAxes.map(([axis, label]) => <NumberField key={axis} label={label} name={`Rotation ${label}`} unit="°" step={1} defaultValue={0} disabled={disabled} value={value.rotation[axis]} onCommit={(next) => { onChange({ ...value, rotation: { ...value.rotation, [axis]: next } }); }} />)}</FieldGroup>
    </Field>
    <Field label="Scale">
      <FieldGroup>{scaleAxes.map(([axis, label]) => <NumberField key={axis} label={label} name={`Scale ${label}`} step={0.01} defaultValue={1} disabled={disabled} value={value.scale[axis]} onCommit={(next) => { onChange({ ...value, scale: { ...value.scale, [axis]: next } }); }} />)}</FieldGroup>
    </Field>
  </>;
}

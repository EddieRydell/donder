import type { Transform } from "../../../types";
import { Field, FieldGroup, NumberField } from "../../kit";

const positionAxes = [["xMeters", "X"], ["yMeters", "Y"], ["zMeters", "Z"]] as const;
const rotationAxes = [["xDegrees", "X"], ["yDegrees", "Y"], ["zDegrees", "Z"]] as const;
const scaleAxes = [["x", "X"], ["y", "Y"], ["z", "Z"]] as const;

/** A layout item's position, rotation and scale; `problem` marks them when a placement is refused. */
export function Placement({ value, onChange, disabled = false, problem }: { value: Transform; onChange: (value: Transform) => void; disabled?: boolean; problem: string | null }) {
  return <>
    <Field label="Position" error={problem}>
      <FieldGroup>{positionAxes.map(([axis, label]) => <NumberField key={axis} label={label} name={`Position ${label}`} unit="m" step={0.01} defaultValue={0} min={-2000} max={2000} disabled={disabled} value={value.position[axis]} onCommit={(next) => { onChange({ ...value, position: { ...value.position, [axis]: next } }); }} />)}</FieldGroup>
    </Field>
    <Field label="Rotation" error={problem}>
      <FieldGroup>{rotationAxes.map(([axis, label]) => <NumberField key={axis} label={label} name={`Rotation ${label}`} unit="°" step={1} defaultValue={0} disabled={disabled} value={value.rotation[axis]} onCommit={(next) => { onChange({ ...value, rotation: { ...value.rotation, [axis]: next } }); }} />)}</FieldGroup>
    </Field>
    <Field label="Scale" error={problem}>
      <FieldGroup>{scaleAxes.map(([axis, label]) => <NumberField key={axis} label={label} name={`Scale ${label}`} step={0.01} defaultValue={1} disabled={disabled} value={value.scale[axis]} onCommit={(next) => { onChange({ ...value, scale: { ...value.scale, [axis]: next } }); }} />)}</FieldGroup>
    </Field>
  </>;
}

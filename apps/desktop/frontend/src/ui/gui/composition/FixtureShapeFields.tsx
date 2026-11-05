import { Plus, Trash2 } from "lucide-react";
import type { GuiFixtureShape, GuiGridAxis, GuiGridCorner } from "../../../types";
import { Field, FieldGroup, IconButton, NumberField, Section, Select, Toggle, type SelectOption } from "../../kit";

const corners: SelectOption<GuiGridCorner>[] = [
  { value: "bottomLeft", label: "Bottom left" }, { value: "bottomRight", label: "Bottom right" },
  { value: "topLeft", label: "Top left" }, { value: "topRight", label: "Top right" }
];
const axes: SelectOption<GuiGridAxis>[] = [{ value: "rows", label: "Rows" }, { value: "columns", label: "Columns" }];
const pointAxes = [["xMeters", "X"], ["yMeters", "Y"], ["zMeters", "Z"]] as const;
const MIN_SIZE = 0.000001;

export function FixtureShapeFields({ shape, onChange, disabled }: { shape: GuiFixtureShape; onChange: (shape: GuiFixtureShape) => void; disabled: boolean }) {
  return <>
    {(shape.type === "line" || shape.type === "arc" || shape.type === "polyline") && <Field label="Pixels">
      <NumberField name="Pixels" integer step={1} min={1} max={1000000} disabled={disabled} value={shape.count} onCommit={(count) => { onChange({ ...shape, count }); }} />
    </Field>}
    {shape.type === "line" && <Field label="Length">
      <NumberField name="Length" unit="m" step={0.01} min={MIN_SIZE} max={2000} disabled={disabled} value={shape.length} onCommit={(length) => { onChange({ ...shape, length }); }} />
    </Field>}
    {shape.type === "arc" && <>
      <Field label="Radius">
        <NumberField name="Radius" unit="m" step={0.01} min={MIN_SIZE} max={2000} disabled={disabled} value={shape.radius} onCommit={(radius) => { onChange({ ...shape, radius }); }} />
      </Field>
      <Field label="Start">
        <NumberField name="Start angle" unit="°" step={1} defaultValue={0} disabled={disabled} value={shape.startDegrees} onCommit={(startDegrees) => { onChange({ ...shape, startDegrees }); }} />
      </Field>
      {!shape.closed && <Field label="Sweep">
        <NumberField name="Sweep" unit="°" step={1} defaultValue={180} min={-360} max={360} disabled={disabled} value={shape.sweepDegrees} onCommit={(sweepDegrees) => { onChange({ ...shape, sweepDegrees }); }} />
      </Field>}
      <Field label="Full circle" tip="A full circle has no duplicate endpoint.">
        <Toggle name="Full circle" disabled={disabled} checked={shape.closed} onChange={(closed) => { onChange({ ...shape, closed, sweepDegrees: closed ? 360 : 180 }); }} />
      </Field>
    </>}
    {shape.type === "grid" && <>
      <Field label="Size">
        <FieldGroup>
          <NumberField label="C" name="Columns" integer step={1} min={1} max={1000000} disabled={disabled} value={shape.columns} onCommit={(columns) => { onChange({ ...shape, columns }); }} />
          <NumberField label="R" name="Rows" integer step={1} min={1} max={1000000} disabled={disabled} value={shape.rows} onCommit={(rows) => { onChange({ ...shape, rows }); }} />
        </FieldGroup>
      </Field>
      <Field label="Extent">
        <FieldGroup>
          <NumberField label="W" name="Width" unit="m" step={0.01} min={MIN_SIZE} max={2000} disabled={disabled} value={shape.width} onCommit={(width) => { onChange({ ...shape, width }); }} />
          <NumberField label="H" name="Height" unit="m" step={0.01} min={MIN_SIZE} max={2000} disabled={disabled} value={shape.height} onCommit={(height) => { onChange({ ...shape, height }); }} />
        </FieldGroup>
      </Field>
      <Field label="Start">
        <Select name="Starting corner" options={corners} disabled={disabled} value={shape.corner} onChange={(corner) => { onChange({ ...shape, corner }); }} />
      </Field>
      <Field label="Traverse">
        <Select name="Traverse" options={axes} disabled={disabled} value={shape.axis} onChange={(axis) => { onChange({ ...shape, axis }); }} />
      </Field>
      <Field label="Serpentine" tip="Alternate direction on every row or column.">
        <Toggle name="Serpentine" disabled={disabled} checked={shape.serpentine} onChange={(serpentine) => { onChange({ ...shape, serpentine }); }} />
      </Field>
    </>}
    {shape.type === "polyline" && <Section title={`Points ${shape.points.length}`} defaultOpen={false}>
      {shape.points.map((point, index) => <Field key={index} label={String(index + 1)}>
        <FieldGroup>{pointAxes.map(([axis, label]) => <NumberField key={axis} label={label} name={`Point ${index + 1} ${label}`} unit="m" step={0.01} min={-2000} max={2000} disabled={disabled} value={point[axis]} onCommit={(value) => { onChange({ ...shape, points: shape.points.map((point, i) => i === index ? { ...point, [axis]: value } : point) }); }} />)}</FieldGroup>
        <IconButton icon={Plus} label="Insert point after" disabled={disabled} onClick={() => {
          const next = shape.points[index + 1];
          const added = next === undefined ? { ...point, xMeters: point.xMeters + 0.1 } : { xMeters: (point.xMeters + next.xMeters) / 2, yMeters: (point.yMeters + next.yMeters) / 2, zMeters: (point.zMeters + next.zMeters) / 2 };
          const points = [...shape.points];
          points.splice(index + 1, 0, added);
          onChange({ ...shape, points });
        }} />
        <IconButton icon={Trash2} label="Remove point" danger disabled={disabled || shape.points.length <= 2} onClick={() => { onChange({ ...shape, points: shape.points.filter((_, i) => i !== index) }); }} />
      </Field>)}
    </Section>}
  </>;
}

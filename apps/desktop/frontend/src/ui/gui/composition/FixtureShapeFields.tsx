import type { GuiFixtureShape, GuiGridAxis, GuiGridCorner } from "../../../types";
import { CoordinateField } from "./FixtureFields";

const corners: { value: GuiGridCorner; label: string }[] = [
  { value: "bottomLeft", label: "Bottom left" }, { value: "bottomRight", label: "Bottom right" },
  { value: "topLeft", label: "Top left" }, { value: "topRight", label: "Top right" }
];
const axes: { value: GuiGridAxis; label: string }[] = [{ value: "rows", label: "Rows" }, { value: "columns", label: "Columns" }];

export function FixtureShapeFields({ shape, onChange }: { shape: GuiFixtureShape; onChange: (shape: GuiFixtureShape) => void }) {
  return <>
    {(shape.type === "line" || shape.type === "arc" || shape.type === "polyline") && <CoordinateField label="Pixels" value={shape.count} min={1} max={1000000} step={1} onChange={(count) => { onChange({ ...shape, count }); }} />}
    {shape.type === "line" && <CoordinateField label="Length (m)" value={shape.length} min={0.000001} max={2000} onChange={(length) => { onChange({ ...shape, length }); }} />}
    {shape.type === "arc" && <>
      <CoordinateField label="Radius (m)" value={shape.radius} min={0.000001} max={2000} onChange={(radius) => { onChange({ ...shape, radius }); }} />
      <CoordinateField label="Start angle (degrees)" value={shape.startDegrees} onChange={(startDegrees) => { onChange({ ...shape, startDegrees }); }} />
      {!shape.closed && <CoordinateField label="Sweep (degrees)" value={shape.sweepDegrees} min={-360} max={360} onChange={(sweepDegrees) => { onChange({ ...shape, sweepDegrees }); }} />}
      <label className="fixture-checkbox"><input type="checkbox" checked={shape.closed} onChange={(event) => { onChange({ ...shape, closed: event.target.checked, sweepDegrees: event.target.checked ? 360 : 180 }); }} />Full circle (no duplicate endpoint)</label>
    </>}
    {shape.type === "grid" && <>
      <CoordinateField label="Columns" value={shape.columns} min={1} max={1000000} step={1} onChange={(columns) => { onChange({ ...shape, columns }); }} />
      <CoordinateField label="Rows" value={shape.rows} min={1} max={1000000} step={1} onChange={(rows) => { onChange({ ...shape, rows }); }} />
      <CoordinateField label="Width (m)" value={shape.width} min={0.000001} max={2000} onChange={(width) => { onChange({ ...shape, width }); }} />
      <CoordinateField label="Height (m)" value={shape.height} min={0.000001} max={2000} onChange={(height) => { onChange({ ...shape, height }); }} />
      <label>Starting corner<select value={shape.corner} onChange={(event) => { const corner = corners.find((corner) => corner.value === event.target.value); if (corner !== undefined) onChange({ ...shape, corner: corner.value }); }}>{corners.map((corner) => <option key={corner.value} value={corner.value}>{corner.label}</option>)}</select></label>
      <label>Traverse<select value={shape.axis} onChange={(event) => { const axis = axes.find((axis) => axis.value === event.target.value); if (axis !== undefined) onChange({ ...shape, axis: axis.value }); }}>{axes.map((axis) => <option key={axis.value} value={axis.value}>{axis.label}</option>)}</select></label>
      <label className="fixture-checkbox"><input type="checkbox" checked={shape.serpentine} onChange={(event) => { onChange({ ...shape, serpentine: event.target.checked }); }} />Serpentine (alternate direction)</label>
    </>}
    {shape.type === "polyline" && <details><summary>Control points ({shape.points.length})</summary>
      {shape.points.map((point, index) => <div className="fixture-control-point" key={index}>
        <span>Point {index + 1}</span>
        {(["xMeters", "yMeters", "zMeters"] as const).map((axis) => <CoordinateField key={axis} label={`${axis.charAt(0).toUpperCase()} (m)`} value={point[axis]} min={-2000} max={2000} onChange={(value) => { onChange({ ...shape, points: shape.points.map((point, i) => i === index ? { ...point, [axis]: value } : point) }); }} />)}
        <button type="button" disabled={shape.points.length <= 2} onClick={() => { onChange({ ...shape, points: shape.points.filter((_, i) => i !== index) }); }}>Remove point</button>
        <button type="button" onClick={() => { const next = shape.points[index + 1]; const added = next === undefined ? { ...point, xMeters: point.xMeters + 0.1 } : { xMeters: (point.xMeters + next.xMeters) / 2, yMeters: (point.yMeters + next.yMeters) / 2, zMeters: (point.zMeters + next.zMeters) / 2 }; const points = [...shape.points]; points.splice(index + 1, 0, added); onChange({ ...shape, points }); }}>Insert point after</button>
      </div>)}
    </details>}
  </>;
}

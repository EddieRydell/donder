import type { Geometry, Point3Meters } from "../../../types";

export function FixtureDefinitionFields({ geometry, diameter, onGeometryChange, onDiameterChange }: {
  geometry: Geometry; diameter: number; onGeometryChange: (geometry: Geometry) => void; onDiameterChange: (diameter: number) => void;
}) {
  return <>
    <GeometryEditor value={geometry} onChange={onGeometryChange} />
    <label>Bulb diameter (meters)<input type="number" min={0.001} step="any" value={diameter} onChange={(event) => { onDiameterChange(Number(event.currentTarget.value)); }} /></label>
  </>;
}

const origin: Point3Meters = { xMeters: 0, yMeters: 0, zMeters: 0 };

function GeometryEditor({ value, onChange }: { value: Geometry; onChange: (geometry: Geometry) => void }) {
  const pixels = value.type === "points" ? value.points.length : value.pixels;
  return <>
    <label>Shape<select value={value.type} onChange={(event) => {
      if (event.currentTarget.value === "lines") onChange({ type: "lines", points: [origin, { ...origin, xMeters: 2 }], pixels });
      if (event.currentTarget.value === "arc") onChange({ type: "arc", center: origin, radiusMeters: 1, startDegrees: 0, endDegrees: 180, pixels });
      if (event.currentTarget.value === "points") onChange({ type: "points", points: Array.from({ length: pixels }, (_, index) => ({ ...origin, xMeters: index * 0.02 })) });
    }}><option value="lines">Line / connected lines</option><option value="arc">Arch / ring</option><option value="points">Individual points</option></select></label>
    {value.type !== "points" && <label>Pixels<input type="number" min={1} step={1} value={value.pixels} onChange={(event) => { onChange({ ...value, pixels: Number(event.currentTarget.value) }); }} /></label>}
    {value.type === "arc" ? <>
      <PointEditor label="Center" value={value.center} onChange={(center) => { onChange({ ...value, center }); }} />
      <label>Radius (meters)<input type="number" min={0.001} step="any" value={value.radiusMeters} onChange={(event) => { onChange({ ...value, radiusMeters: Number(event.currentTarget.value) }); }} /></label>
      <label>Start angle<input type="number" step="any" value={value.startDegrees} onChange={(event) => { onChange({ ...value, startDegrees: Number(event.currentTarget.value) }); }} /></label>
      <label>End angle<input type="number" step="any" value={value.endDegrees} onChange={(event) => { onChange({ ...value, endDegrees: Number(event.currentTarget.value) }); }} /></label>
    </> : value.points.map((point, index) => <div className="setup-geometry-point" key={index}><PointEditor label={`Point ${index + 1}`} value={point} onChange={(next) => { onChange({ ...value, points: value.points.map((candidate, candidateIndex) => candidateIndex === index ? next : candidate) }); }} /><button type="button" disabled={value.points.length <= (value.type === "lines" ? 2 : 1)} onClick={() => { onChange({ ...value, points: value.points.filter((_, candidateIndex) => candidateIndex !== index) }); }}>Remove point</button></div>)}
    {value.type !== "arc" && <button type="button" onClick={() => { onChange({ ...value, points: [...value.points, origin] }); }}>Add point</button>}
  </>;
}

function PointEditor({ label, value, onChange }: { label: string; value: Point3Meters; onChange: (point: Point3Meters) => void }) {
  return <div className="setup-point-input"><span>{label}</span><label>X<input type="number" step="any" value={value.xMeters} onChange={(event) => { onChange({ ...value, xMeters: Number(event.currentTarget.value) }); }} /></label><label>Y<input type="number" step="any" value={value.yMeters} onChange={(event) => { onChange({ ...value, yMeters: Number(event.currentTarget.value) }); }} /></label><label>Z<input type="number" step="any" value={value.zMeters} onChange={(event) => { onChange({ ...value, zMeters: Number(event.currentTarget.value) }); }} /></label></div>;
}

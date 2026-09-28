import { useState } from "react";
import { useAppStore } from "../../../store";
import { CoordinateField } from "./FixtureFields";
import { arrange, repeatOffsets, unionBounds, type Arrangement, type SpatialItem, type SpatialMove } from "./spatialSelection";
import { spatialUnits, formatDistance } from "./spatialSnapping";
import type { Point3Meters } from "../../../types";
const actions: { action: Arrangement; label: string }[] = [
  { action: "left", label: "Left" }, { action: "centerX", label: "Center horizontally" }, { action: "right", label: "Right" },
  { action: "top", label: "Top" }, { action: "centerY", label: "Center vertically" }, { action: "bottom", label: "Bottom" },
  { action: "spaceX", label: "Equal horizontal gaps" }, { action: "spaceY", label: "Equal vertical gaps" }
];
export function SpatialSelectionControls({ items, disabled, onMove, onRepeat, onDuplicate, onDelete }: { items: SpatialItem[]; disabled: boolean; onMove: (moves: SpatialMove[]) => void; onRepeat: (offsets: Point3Meters[]) => void; onDuplicate: () => void; onDelete: () => void }) {
  const settings = useAppStore((state) => state.snapshot?.settings.spatialSnap);
  const [rows, setRows] = useState(1); const [columns, setColumns] = useState(2);
  const [stepX, setStepX] = useState<number | null>(null); const [stepY, setStepY] = useState<number | null>(null);
  if (items.length === 0 || settings === undefined) return null;
  const bounds = unionBounds(items.map((item) => item.bounds));
  const unit = spatialUnits[settings.unit];
  const x = stepX ?? bounds.right - bounds.left + settings.spacingMeters;
  const y = stepY ?? bounds.top - bounds.bottom + settings.spacingMeters;
  const copies = (rows * columns - 1) * items.length;
  return <fieldset className="spatial-selection-controls" disabled={disabled}>
    <p>{items.length} selected · {formatDistance(bounds.right - bounds.left, settings.unit)} × {formatDistance(bounds.top - bounds.bottom, settings.unit)}</p>
    <div className="fixture-shape-actions"><button type="button" onClick={onDuplicate}>Duplicate</button><button type="button" onClick={onDelete}>Delete</button></div>
    <details><summary>Align and distribute</summary><div className="spatial-arrange-actions">{actions.map(({ action, label }) => <button type="button" key={action} disabled={items.length < (action === "spaceX" || action === "spaceY" ? 3 : 2)} onClick={() => { onMove(arrange(items, action)); }}>{label}</button>)}</div></details>
    <details><summary>Repeat selection</summary><div className="spatial-repeat-fields">
      <CoordinateField label="Columns" value={columns} min={1} max={1001} step={1} onChange={setColumns} />
      <CoordinateField label="Rows" value={rows} min={1} max={1001} step={1} onChange={setRows} />
      <CoordinateField label={`Horizontal step (${unit.label})`} value={x / unit.meters} onChange={(value) => { setStepX(value * unit.meters); }} />
      <CoordinateField label={`Vertical step (${unit.label})`} value={y / unit.meters} onChange={(value) => { setStepY(value * unit.meters); }} />
      <p>Steps are from each original to its copy. Positive vertical steps go up.</p>
      <button type="button" disabled={copies < 1 || copies > 1000} onClick={() => { onRepeat(repeatOffsets(rows, columns, x, y)); }}>Create {copies} copies</button>
    </div></details>
  </fieldset>;
}

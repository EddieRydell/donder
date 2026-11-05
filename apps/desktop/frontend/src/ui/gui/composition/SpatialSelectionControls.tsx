import { useState } from "react";
import { AlignCenterHorizontal, AlignCenterVertical, AlignEndHorizontal, AlignEndVertical, AlignHorizontalSpaceAround, AlignStartHorizontal, AlignStartVertical, AlignVerticalSpaceAround, Copy, Repeat, Trash2, type LucideIcon } from "lucide-react";
import { useAppStore } from "../../../store";
import { arrange, repeatOffsets, unionBounds, type Arrangement, type SpatialItem, type SpatialMove } from "./spatialSelection";
import { spatialUnits, formatDistance } from "./spatialSnapping";
import type { Point3Meters } from "../../../types";
import { Button, Field, FieldGroup, IconButton, NumberField, Section } from "../../kit";

const actions: { action: Arrangement; label: string; icon: LucideIcon }[] = [
  { action: "left", label: "Align left", icon: AlignStartVertical }, { action: "centerX", label: "Center horizontally", icon: AlignCenterVertical }, { action: "right", label: "Align right", icon: AlignEndVertical },
  { action: "top", label: "Align top", icon: AlignStartHorizontal }, { action: "centerY", label: "Center vertically", icon: AlignCenterHorizontal }, { action: "bottom", label: "Align bottom", icon: AlignEndHorizontal },
  { action: "spaceX", label: "Equal horizontal gaps", icon: AlignHorizontalSpaceAround }, { action: "spaceY", label: "Equal vertical gaps", icon: AlignVerticalSpaceAround }
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
  return <div className="spatial-selection-controls">
    <Field label={`${items.length} selected`} tip="Selection size">
      <span>{formatDistance(bounds.right - bounds.left, settings.unit)} × {formatDistance(bounds.top - bounds.bottom, settings.unit)}</span>
      <IconButton icon={Copy} label="Duplicate" command="edit.duplicate" disabled={disabled} onClick={onDuplicate} />
      <IconButton icon={Trash2} label="Delete" command="edit.delete" danger disabled={disabled} onClick={onDelete} />
    </Field>
    <div className="fixture-shape-actions">{actions.map(({ action, label, icon }) => <IconButton key={action} icon={icon} label={label} disabled={disabled || items.length < (action === "spaceX" || action === "spaceY" ? 3 : 2)} onClick={() => { onMove(arrange(items, action)); }} />)}</div>
    <Section title="Repeat" defaultOpen={false}>
      <Field label="Grid">
        <FieldGroup>
          <NumberField label="C" name="Columns" integer step={1} min={1} max={1001} disabled={disabled} value={columns} onCommit={setColumns} />
          <NumberField label="R" name="Rows" integer step={1} min={1} max={1001} disabled={disabled} value={rows} onCommit={setRows} />
        </FieldGroup>
      </Field>
      <Field label="Step" tip="From each original to its copy. Positive vertical steps go up.">
        <FieldGroup>
          <NumberField label="X" name="Horizontal step" unit={unit.label} step={0.1} disabled={disabled} value={x / unit.meters} onCommit={(value) => { setStepX(value * unit.meters); }} />
          <NumberField label="Y" name="Vertical step" unit={unit.label} step={0.1} disabled={disabled} value={y / unit.meters} onCommit={(value) => { setStepY(value * unit.meters); }} />
        </FieldGroup>
      </Field>
      <Button icon={Repeat} disabled={disabled || copies < 1 || copies > 1000} onClick={() => { onRepeat(repeatOffsets(rows, columns, x, y)); }}>{`Create ${copies}`}</Button>
    </Section>
  </div>;
}

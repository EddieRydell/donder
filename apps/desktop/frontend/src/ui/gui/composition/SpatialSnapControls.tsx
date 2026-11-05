import { useState } from "react";
import { SeparatorHorizontal, SeparatorVertical, Trash2 } from "lucide-react";
import { commands } from "../../../api";
import { runSnapshotCommand, useAppStore } from "../../../store";
import type { SpatialSnapSettings, SpatialUnit, SpatialGuide } from "../../../types";
import { spatialUnits } from "./spatialSnapping";
import { IconButton, NumberField, Section, Select, Tip, Toggle, type SelectOption } from "../../kit";

const unitOptions = Object.entries(spatialUnits).map(([key, unit]): SelectOption<SpatialUnit> => ({ value: key as SpatialUnit, label: unit.label }));

export function SpatialSnapControls({ settings, disabled, guides, onGuides }: { settings: SpatialSnapSettings; disabled: boolean; guides: SpatialGuide[]; onGuides: (guides: SpatialGuide[]) => void }) {
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async (snap: SpatialSnapSettings) => {
    const current = useAppStore.getState().snapshot?.settings;
    if (current === undefined) return;
    setSaving(true);
    try { await runSnapshotCommand(() => commands.updateAppSettings({ ...current, spatialSnap: snap })); setError(null); }
    catch (error) { setError(String(error)); }
    finally { setSaving(false); }
  };
  const unit = spatialUnits[settings.unit];
  const locked = disabled || saving;
  return <div className="spatial-snap-panel">
    <div className="spatial-snap-controls">
      <Tip label="Shift: 45° / square grid · Alt: bypass snap · Space-drag: pan"><span><Toggle name="Snap" label="Snap" disabled={locked} checked={settings.enabled} onChange={(enabled) => { void save({ ...settings, enabled }); }} /></span></Tip>
      <NumberField name="Snap distance" step={0.1} unit={unit.label} disabled={locked} value={Number((settings.spacingMeters / unit.meters).toPrecision(10))} min={0.000001 / unit.meters} max={2000 / unit.meters} onCommit={(value) => { void save({ ...settings, spacingMeters: value * unit.meters }); }} />
      <Select name="Unit" options={unitOptions} disabled={locked} value={settings.unit} onChange={(next) => { void save({ ...settings, unit: next }); }} />
    </div>
    <Section title={`Guides ${guides.length}`} defaultOpen={false} actions={<>
      <IconButton icon={SeparatorVertical} label="Add vertical guide" disabled={disabled || guides.length >= 1000} onClick={() => { onGuides([...guides, { axis: "x", positionMeters: 0 }]); }} />
      <IconButton icon={SeparatorHorizontal} label="Add horizontal guide" disabled={disabled || guides.length >= 1000} onClick={() => { onGuides([...guides, { axis: "y", positionMeters: 0 }]); }} />
    </>}>
      {guides.map((guide, index) => <div className="spatial-guide-row" key={index}>
        <NumberField label={guide.axis === "x" ? "X" : "Y"} name={`Guide ${index + 1}`} step={0.1} unit={unit.label} disabled={disabled} value={guide.positionMeters / unit.meters} min={-2000 / unit.meters} max={2000 / unit.meters} onCommit={(value) => { onGuides(guides.map((item, i) => i === index ? { ...item, positionMeters: value * unit.meters } : item)); }} />
        <IconButton icon={Trash2} label={`Remove guide ${index + 1}`} danger disabled={disabled} onClick={() => { onGuides(guides.filter((_, i) => i !== index)); }} />
      </div>)}
    </Section>
    {error !== null && <p role="alert">{error}</p>}
  </div>;
}

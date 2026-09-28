import { useState } from "react";
import { commands } from "../../../api";
import { runSnapshotCommand, useAppStore } from "../../../store";
import type { SpatialSnapSettings, SpatialUnit, SpatialGuide } from "../../../types";
import { CoordinateField } from "./FixtureFields";
import { spatialUnits } from "./spatialSnapping";

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
  return <div className="spatial-snap-panel">
    <fieldset className="spatial-snap-controls" disabled={disabled || saving}>
      <label className="fixture-checkbox"><input type="checkbox" checked={settings.enabled} onChange={(event) => { void save({ ...settings, enabled: event.target.checked }); }} />Snap</label>
      <CoordinateField label="Distance" value={Number((settings.spacingMeters / unit.meters).toPrecision(10))} min={0.000001 / unit.meters} max={2000 / unit.meters} onChange={(value) => { void save({ ...settings, spacingMeters: value * unit.meters }); }} />
      <label>Unit<select value={settings.unit} onChange={(event) => { void save({ ...settings, unit: event.target.value as SpatialUnit }); }}>
        {Object.entries(spatialUnits).map(([key, unit]) => <option key={key} value={key}>{unit.label}</option>)}
      </select></label>
      <span>Shift: 45° / square grid · Ctrl/Cmd: bypass snap</span>
    </fieldset>
    <details className="spatial-guides"><summary>Guides and shortcuts</summary>
      <fieldset disabled={disabled}>
        <p>Drag empty canvas to box-select. Shift/Ctrl/Cmd-click to add or remove a selection. Alt-drag or middle-drag to pan. Arrow keys nudge; Shift-arrows move ten steps. Home fits the view.</p>
        <p>Snap uses the grid, existing points, and guides. Guides belong to this editor view and are saved with your workspace.</p>
        <div className="fixture-shape-actions"><button type="button" disabled={guides.length >= 1000} onClick={() => { onGuides([...guides, { axis: "x", positionMeters: 0 }]); }}>Add vertical guide</button><button type="button" disabled={guides.length >= 1000} onClick={() => { onGuides([...guides, { axis: "y", positionMeters: 0 }]); }}>Add horizontal guide</button></div>
        {guides.map((guide, index) => <div className="spatial-guide-row" key={index}><CoordinateField label={`${guide.axis === "x" ? "X" : "Y"} (${unit.label})`} value={guide.positionMeters / unit.meters} min={-2000 / unit.meters} max={2000 / unit.meters} onChange={(value) => { onGuides(guides.map((item, i) => i === index ? { ...item, positionMeters: value * unit.meters } : item)); }} /><button type="button" aria-label={`Remove guide ${index + 1}`} onClick={() => { onGuides(guides.filter((_, i) => i !== index)); }}>Remove</button></div>)}
      </fieldset>
    </details>
    {error !== null && <p role="alert">{error}</p>}
  </div>;
}

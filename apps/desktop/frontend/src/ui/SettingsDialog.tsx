import { useEffect, useState } from "react";
import { commands } from "../api";
import { runSnapshotCommand, useAppStore } from "../store";
import type { AppSettings, SequenceFollowMode, SequenceInitialZoomMode } from "../types";
import { KeybindingsSettings } from "./KeybindingsSettings";
import { Dialog, Field, NumberField, Section, Select, Toggle, type SelectOption } from "./kit";

const SETTINGS_EVENT = "donder:settings";

const ZOOM_OPTIONS: SelectOption<SequenceInitialZoomMode>[] = [
  { value: "fitToWidth", label: "Fit width" },
  { value: "fixedPxPerSecond", label: "Fixed scale" }
];
const FOLLOW_OPTIONS: SelectOption<SequenceFollowMode>[] = [
  { value: "off", label: "Off" },
  { value: "page", label: "Page" },
  { value: "continuous", label: "Continuous" }
];
const FFT_OPTIONS: SelectOption<string>[] = [512, 1024, 2048, 4096, 8192, 16384]
  .map((size) => ({ value: String(size), label: String(size) }));

export function SettingsDialog() {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<AppSettings | null>(() => useAppStore.getState().snapshot?.settings ?? null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onSettings = () => {
      setDraft(useAppStore.getState().snapshot?.settings ?? null);
      setError(null);
      setOpen(true);
    };
    window.addEventListener(SETTINGS_EVENT, onSettings);
    return () => {
      window.removeEventListener(SETTINGS_EVENT, onSettings);
    };
  }, []);

  async function update(next: AppSettings) {
    setDraft(next);
    setError(null);
    try {
      await runSnapshotCommand(() => commands.updateAppSettings(next));
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    }
  }

  if (draft === null) return null;
  const raster = draft.clipRaster;

  return <Dialog open={open} onOpenChange={setOpen} title="Settings" size="lg">
    <Section title="General">
      <Toggle label="Reopen project" name="Reopen last project" checked={draft.reopenLastProject} onChange={(reopenLastProject) => void update({ ...draft, reopenLastProject })} />
      <Toggle label="Reopen preview" name="Reopen preview window" checked={draft.reopenPreviewWindow} onChange={(reopenPreviewWindow) => void update({ ...draft, reopenPreviewWindow })} />
      <Toggle label="Autosave" name="Autosave project edits" checked={draft.autosaveProjectEdits} onChange={(autosaveProjectEdits) => void update({ ...draft, autosaveProjectEdits })} />
    </Section>
    <Section title="Sequence">
      <Field label="Zoom" tip="Initial zoom">
        <Select name="Initial zoom" value={draft.sequenceInitialZoomMode} options={ZOOM_OPTIONS} onChange={(sequenceInitialZoomMode) => void update({ ...draft, sequenceInitialZoomMode })} />
      </Field>
      <Field label="Scale" tip="Initial pixels per second">
        <NumberField name="Initial pixels per second" min={20} max={12000} step={5} unit="px/s" value={draft.sequenceInitialPxPerSecond} onCommit={(sequenceInitialPxPerSecond) => void update({ ...draft, sequenceInitialPxPerSecond })} />
      </Field>
      <Field label="Lane height" tip="Initial lane height">
        <NumberField name="Initial lane height" min={24} max={120} step={1} unit="px" value={draft.sequenceInitialLaneHeightPx} onCommit={(sequenceInitialLaneHeightPx) => void update({ ...draft, sequenceInitialLaneHeightPx })} />
      </Field>
      <Field label="Follow" tip="Follow the playhead">
        <Select name="Follow playhead" value={draft.sequenceFollowMode} options={FOLLOW_OPTIONS} onChange={(sequenceFollowMode) => void update({ ...draft, sequenceFollowMode })} />
      </Field>
      <Toggle label="Spectrogram" name="Show spectrogram" checked={draft.sequenceSpectrogramEnabled ?? false} onChange={(sequenceSpectrogramEnabled) => void update({ ...draft, sequenceSpectrogramEnabled })} />
      <Field label="Time step" tip="Spectrogram time resolution">
        <NumberField name="Spectrogram time resolution" min={0.5} max={100} step={0.5} unit="ms" value={draft.sequenceSpectrogramTimeResolutionMs ?? 10} onCommit={(sequenceSpectrogramTimeResolutionMs) => void update({ ...draft, sequenceSpectrogramTimeResolutionMs })} />
      </Field>
      <Field label="FFT size" tip="Spectrogram frequency resolution, in FFT points">
        <Select name="Spectrogram FFT size" value={String(draft.sequenceSpectrogramFftSize ?? 2048)} options={FFT_OPTIONS} onChange={(size) => void update({ ...draft, sequenceSpectrogramFftSize: Number(size) })} />
      </Field>
    </Section>
    <Section title="Raster" defaultOpen={false}>
      <Field label="Columns" tip="Maximum clip raster columns">
        <NumberField name="Max columns" integer min={16} max={1024} step={1} value={raster.maxColumns} onCommit={(maxColumns) => void update({ ...draft, clipRaster: { ...raster, maxColumns } })} />
      </Field>
      <Field label="Rows" tip="Maximum clip raster rows">
        <NumberField name="Max rows" integer min={1} max={200} step={1} value={raster.maxRows} onCommit={(maxRows) => void update({ ...draft, clipRaster: { ...raster, maxRows } })} />
      </Field>
      <Field label="Frame stride" tip="Minimum frames between raster samples">
        <NumberField name="Min frame stride" integer min={1} max={16} step={1} value={raster.minFrameStride} onCommit={(minFrameStride) => void update({ ...draft, clipRaster: { ...raster, minFrameStride } })} />
      </Field>
    </Section>
    <KeybindingsSettings keybindings={draft.keybindings ?? []} onChange={(keybindings) => void update({ ...draft, keybindings })} />
    {error !== null && <div className="kit-status-error" role="alert">{error}</div>}
  </Dialog>;
}

import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { useEffect, useState } from "react";
import { commands } from "../api";
import { runSnapshotCommand, useAppStore } from "../store";
import type { AppSettings } from "../types";

const SETTINGS_EVENT = "donder:settings";

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

  return (
    <AlertDialog.Root open={open} onOpenChange={setOpen}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="dialog-overlay" />
        <AlertDialog.Content className="dialog-content settings-dialog">
          <AlertDialog.Title>Settings</AlertDialog.Title>
          <div className="settings-form">
            <section>
              <h3>General</h3>
              <Checkbox
                label="Reopen last project"
                checked={draft.reopenLastProject}
                onChange={(reopenLastProject) => void update({ ...draft, reopenLastProject })}
              />
              <Checkbox
                label="Reopen preview window"
                checked={draft.reopenPreviewWindow}
                onChange={(reopenPreviewWindow) => void update({ ...draft, reopenPreviewWindow })}
              />
            </section>

            <section>
              <h3>Editor</h3>
              <Checkbox
                label="Autosave project edits"
                checked={draft.autosaveProjectEdits}
                onChange={(autosaveProjectEdits) => void update({ ...draft, autosaveProjectEdits })}
              />
            </section>

            <section>
              <h3>Sequence</h3>
              <Select
                label="Initial zoom"
                value={draft.sequenceInitialZoomMode}
                options={[
                  ["fitToWidth", "Fit to width"],
                  ["fixedPxPerSecond", "Fixed px/sec"]
                ]}
                onChange={(sequenceInitialZoomMode) => void update({ ...draft, sequenceInitialZoomMode })}
              />
              <NumberInput
                label="Initial px/sec"
                min={20}
                max={12000}
                step={1}
                value={draft.sequenceInitialPxPerSecond}
                onChange={(sequenceInitialPxPerSecond) => void update({ ...draft, sequenceInitialPxPerSecond })}
              />
              <NumberInput
                label="Initial lane height"
                min={24}
                max={120}
                step={1}
                value={draft.sequenceInitialLaneHeightPx}
                onChange={(sequenceInitialLaneHeightPx) => void update({ ...draft, sequenceInitialLaneHeightPx })}
              />
            </section>

            <section>
              <h3>Raster</h3>
              <NumberInput
                label="Render scale"
                min={0.25}
                max={2}
                step={0.25}
                value={draft.effectRaster.renderScale}
                onChange={(renderScale) =>
                  void update({ ...draft, effectRaster: { ...draft.effectRaster, renderScale } })
                }
              />
              <NumberInput
                label="Max columns"
                min={16}
                max={1024}
                step={1}
                value={draft.effectRaster.maxColumns}
                onChange={(maxColumns) =>
                  void update({ ...draft, effectRaster: { ...draft.effectRaster, maxColumns } })
                }
              />
              <NumberInput
                label="Max rows"
                min={1}
                max={200}
                step={1}
                value={draft.effectRaster.maxRows}
                onChange={(maxRows) =>
                  void update({ ...draft, effectRaster: { ...draft.effectRaster, maxRows } })
                }
              />
              <NumberInput
                label="Min frame stride"
                min={1}
                max={16}
                step={1}
                value={draft.effectRaster.minFrameStride}
                onChange={(minFrameStride) =>
                  void update({ ...draft, effectRaster: { ...draft.effectRaster, minFrameStride } })
                }
              />
            </section>
            {error !== null && <div className="new-project-error">{error}</div>}
          </div>
          <div className="dialog-actions">
            <AlertDialog.Cancel>Close</AlertDialog.Cancel>
          </div>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}

function Checkbox({ label, checked, onChange }: { label: string; checked: boolean; onChange: (value: boolean) => void }) {
  return (
    <label className="settings-checkbox">
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => {
          onChange(event.target.checked);
        }}
      />
      <span>{label}</span>
    </label>
  );
}

function Select<T extends string>({
  label,
  value,
  options,
  onChange
}: {
  label: string;
  value: T;
  options: Array<[T, string]>;
  onChange: (value: T) => void;
}) {
  return (
    <label>
      <span>{label}</span>
      <select
        value={value}
        onChange={(event) => {
          onChange(event.target.value as T);
        }}
      >
        {options.map(([optionValue, optionLabel]) => (
          <option key={optionValue} value={optionValue}>
            {optionLabel}
          </option>
        ))}
      </select>
    </label>
  );
}

function NumberInput({
  label,
  min,
  max,
  step,
  value,
  onChange
}: {
  label: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <label>
      <span>{label}</span>
      <input
        min={min}
        max={max}
        step={step}
        type="number"
        value={value}
        onChange={(event) => {
          const next = Number(event.target.value);
          if (Number.isFinite(next)) onChange(Math.min(Math.max(next, min), max));
        }}
      />
    </label>
  );
}

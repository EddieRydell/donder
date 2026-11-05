import { Channel } from "@tauri-apps/api/core";
import { Download } from "lucide-react";
import { useState } from "react";
import { commands } from "../../../api";
import { PREVIEW_APPEARANCE } from "../../../previewAppearance";
import { useAppStore } from "../../../store";
import type { GuiDocumentRequest, SequenceExportOptions, VideoExportProgress } from "../../../types";
import { Button, Dialog, Field, IconButton, NumberField, Select, Toggle, type SelectOption } from "../../kit";

/** Common FPP frame steps, offered beside the step closest to the authored rate. */
const FSEQ_PRESET_STEPS = [10, 20, 25, 50];
const FSEQ_MAX_STEP = 255;
const MILLIS_PER_SECOND = 1000;
const CUSTOM_STEP = "custom";
/** Video frame rates offered: 30 for small files, 60 for fast effects. */
const DEFAULT_VIDEO_FRAME_RATE = 30;
const VIDEO_FRAME_RATE_OPTIONS: SelectOption<string>[] = [DEFAULT_VIDEO_FRAME_RATE, 60]
  .map((rate) => ({ value: String(rate), label: `${rate} fps` }));

export function SequenceExportDialog() {
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const editing = useAppStore((state) => state.guiEditPending);
  const [origin, setOrigin] = useState<GuiDocumentRequest | null>(null);
  const [options, setOptions] = useState<SequenceExportOptions | null>(null);
  const [selected, setSelected] = useState<number[]>([]);
  const [stepMillis, setStepMillis] = useState(0);
  const [customStep, setCustomStep] = useState(false);
  const [pending, setPending] = useState<"options" | "dondershow" | "fseq" | "mp4" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [videoProgress, setVideoProgress] = useState<VideoExportProgress | null>(null);
  const [videoFrameRate, setVideoFrameRate] = useState(DEFAULT_VIDEO_FRAME_RATE);
  const stale = origin !== null && origin !== request;
  const ports = options?.ports ?? [];
  const stepOptions: SelectOption<string>[] = options === null ? [] : [
    ...[...new Set([options.fseqStepMillis, ...FSEQ_PRESET_STEPS])].map((step) => ({
      value: String(step),
      label: `${step} ms · ${Math.round(MILLIS_PER_SECOND / step)} fps${step === options.fseqStepMillis ? " (sequence)" : ""}`
    })),
    { value: CUSTOM_STEP, label: "Custom" }
  ];
  const stepValid = Number.isInteger(stepMillis) && stepMillis >= 1 && stepMillis <= FSEQ_MAX_STEP;
  const firstChannels = new Map<number, number>();
  selected.reduce((first, index) => {
    firstChannels.set(index, first);
    return first + (ports.find((port) => port.index === index)?.channels ?? 0);
  }, 1);
  const begin = async () => {
    if (request === null || editing || request.projectRevision !== revision) return;
    setOrigin(request); setOptions(null); setSelected([]); setCustomStep(false); setError(null); setSaved(null); setPending("options");
    try {
      const result = await commands.sequenceExportOptions(request);
      setOptions(result);
      setSelected(result.ports.map((port) => port.index));
      setStepMillis(result.fseqStepMillis);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(null); }
  };
  const save = async (format: "dondershow" | "fseq" | "mp4") => {
    if (origin === null || stale) return;
    setPending(format); setError(null); setSaved(null);
    try {
      setSaved(format === "fseq"
        ? await commands.exportFseqFile(origin, selected, stepMillis)
        : format === "mp4"
          ? await exportVideo(origin)
          : await commands.exportShowFile(origin, selected));
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(null); setVideoProgress(null); }
  };
  const exportVideo = (target: GuiDocumentRequest) => {
    const channel = new Channel<VideoExportProgress>();
    channel.onmessage = setVideoProgress;
    return commands.exportVideoFile(target, PREVIEW_APPEARANCE, videoFrameRate, channel);
  };
  const busy = pending !== null;
  const locked = busy || stale;
  const problem = stale ? "Project changed; reopen export" : error ?? (options !== null && ports.length === 0 ? "No outputs in Display Setup" : null);
  return <>
    <IconButton icon={Download} label="Export sequence" disabled={busy || editing || request === null || request.projectRevision !== revision} onClick={() => { void begin(); }} />
    <Dialog
      open={origin !== null}
      onOpenChange={(open) => { if (!open && !busy) setOrigin(null); }}
      title="Export"
      size="md"
      actions={<>
        <Button disabled={locked} tip="The Preview with the song, ready to share" onClick={() => { void save("mp4"); }}>{pending === "mp4" ? "Exporting…" : "Save .mp4"}</Button>
        <Button disabled={locked || selected.length === 0 || !stepValid} tip="The outputs' channels back to back, for FPP and other players" onClick={() => { void save("fseq"); }}>{pending === "fseq" ? "Preparing…" : "Save .fseq"}</Button>
        <Button variant="primary" disabled={locked || selected.length === 0} tip="The show a Donder controller plays, outputs in order. Controllers in the setup receive it on Play" onClick={() => { void save("dondershow"); }}>{pending === "dondershow" ? "Preparing…" : "Save .dondershow"}</Button>
      </>}
    >
      {pending === "options" && <p role="status">Loading…</p>}
      {options !== null && ports.length > 0 && <div className="sequence-export-ports">
        {ports.map((port) => {
          const first = firstChannels.get(port.index);
          const placement = first === undefined ? "" : ` · #${selected.indexOf(port.index) + 1}, ch ${first}–${first + port.channels - 1}`;
          return <Toggle
            key={port.index}
            name={port.label}
            label={`${port.label} · ${port.channels} ch${placement}`}
            checked={first !== undefined}
            disabled={locked}
            onChange={(checked) => { setSaved(null); setSelected(checked ? [...selected, port.index] : selected.filter((index) => index !== port.index)); }}
          />;
        })}
      </div>}
      {options !== null && <Field label="FSEQ step" tip="Time between FSEQ frames">
        <Select
          name="FSEQ frame step"
          disabled={locked}
          value={customStep ? CUSTOM_STEP : String(stepMillis)}
          options={stepOptions}
          onChange={(value) => {
            setSaved(null);
            if (value === CUSTOM_STEP) { setCustomStep(true); return; }
            setCustomStep(false); setStepMillis(Number(value));
          }}
        />
        {customStep && <NumberField name="Custom FSEQ frame step" integer min={1} max={FSEQ_MAX_STEP} step={1} defaultValue={options.fseqStepMillis} unit="ms" disabled={locked} value={stepMillis} onCommit={(value) => { setSaved(null); setStepMillis(value); }} />}
      </Field>}
      {options !== null && stepValid && stepMillis > options.fseqSmoothStepMillis && <p role="status" className="kit-status-warning">
        Steps over {options.fseqSmoothStepMillis} ms play less smoothly than live output, and dithered routes flicker.
      </p>}
      {options !== null && <Field label="Video">
        <Select name="Video frame rate" disabled={locked} value={String(videoFrameRate)} options={VIDEO_FRAME_RATE_OPTIONS} onChange={(value) => { setSaved(null); setVideoFrameRate(Number(value)); }} />
      </Field>}
      {videoProgress !== null && <div className="sequence-export-progress">
        <span id="sequence-export-progress-label">{videoProgressLabel(videoProgress)}</span>
        <progress aria-labelledby="sequence-export-progress-label" value={videoProgress.stage === "rendering" ? videoProgress.completed : undefined} max={videoProgress.stage === "rendering" ? Math.max(1, videoProgress.total) : undefined} />
      </div>}
      {problem !== null && <div className="kit-status-error" role="alert">{problem}</div>}
      {saved !== null && <p role="status">Saved {saved}</p>}
    </Dialog>
  </>;
}

function videoProgressLabel(progress: VideoExportProgress): string {
  switch (progress.stage) {
    case "preparingAudio": return "Preparing song…";
    case "rendering": return `Rendering ${Math.round(100 * progress.completed / Math.max(1, progress.total))}%`;
    case "saving": return "Saving…";
  }
}

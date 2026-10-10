import * as Dialog from "@radix-ui/react-dialog";
import { Channel } from "@tauri-apps/api/core";
import { Download } from "lucide-react";
import { useState } from "react";
import { commands } from "../../../api";
import { PREVIEW_APPEARANCE } from "../../../previewAppearance";
import { useAppStore } from "../../../store";
import { THEME_METRICS } from "../../../theme";
import type { GuiDocumentRequest, SequenceExportOptions, VideoExportProgress } from "../../../types";

/** Common FPP frame steps, offered beside the step closest to the authored rate. */
const FSEQ_PRESET_STEPS = [20, 25, 50];
const FSEQ_MAX_STEP = 255;
const MILLIS_PER_SECOND = 1000;
/** Video frame rates offered: 30 for small files, 60 for fast effects. */
const DEFAULT_VIDEO_FRAME_RATE = 30;
const VIDEO_FRAME_RATES = [DEFAULT_VIDEO_FRAME_RATE, 60];

export function SequenceExportDialog() {
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const editing = useAppStore((state) => state.guiEditPending);
  const [origin, setOrigin] = useState<GuiDocumentRequest | null>(null);
  const [options, setOptions] = useState<SequenceExportOptions | null>(null);
  const [selected, setSelected] = useState<number[]>([]);
  const [stepMillis, setStepMillis] = useState(0);
  const [customStep, setCustomStep] = useState(false);
  const [pending, setPending] = useState<"options" | "donderseq" | "fseq" | "mp4" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [videoProgress, setVideoProgress] = useState<VideoExportProgress | null>(null);
  const [videoFrameRate, setVideoFrameRate] = useState(DEFAULT_VIDEO_FRAME_RATE);
  const stale = origin !== null && origin !== request;
  const ports = options?.ports ?? [];
  const steps = options === null ? [] : [...new Set([options.fseqStepMillis, ...FSEQ_PRESET_STEPS])];
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
      if (result.status === "error") throw new Error(result.error);
      setOptions(result.data);
      setStepMillis(result.data.fseqStepMillis);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(null); }
  };
  const save = async (format: "donderseq" | "fseq" | "mp4") => {
    if (origin === null || stale) return;
    setPending(format); setError(null); setSaved(null);
    try {
      const result = format === "fseq"
        ? await commands.exportFseqFile(origin, selected, stepMillis)
        : format === "mp4"
          ? await exportVideo(origin)
          : await commands.exportSequenceFile(origin, selected);
      if (result.status === "error") throw new Error(result.error);
      setSaved(result.data);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(null); setVideoProgress(null); }
  };
  const exportVideo = (target: GuiDocumentRequest) => {
    const channel = new Channel<VideoExportProgress>();
    channel.onmessage = setVideoProgress;
    return commands.exportVideoFile(target, PREVIEW_APPEARANCE, videoFrameRate, channel);
  };
  const busy = pending !== null;
  return <>
    <button type="button" title="Export sequence" disabled={busy || editing || request === null || request.projectRevision !== revision} onClick={() => { void begin(); }}><Download size={THEME_METRICS.iconSizeCompact} /></button>
    <Dialog.Root open={origin !== null} onOpenChange={(open) => { if (!open && !busy) setOrigin(null); }}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content sequence-export-dialog">
          <Dialog.Title>Export sequence</Dialog.Title>
          <Dialog.Description>Choose the outputs to include, in order. An FSEQ file packs their channels back to back for FPP and other players. A .donderseq file is a compiled show whose payload outputs follow the same order. Donder controllers in the setup receive the current sequence automatically when you press Play. An .mp4 video shows the Preview with the song, ready to share.</Dialog.Description>
          {stale && <p role="alert">The project changed. Close and reopen export to use the current sequence and outputs.</p>}
          {error !== null && <p role="alert">{error}</p>}
          {saved !== null && <p role="status">Saved {saved}</p>}
          {pending === "options" && <p role="status">Loading export options…</p>}
          {videoProgress !== null && <div className="sequence-export-progress">
            <span id="sequence-export-progress-label">{videoProgressLabel(videoProgress)}</span>
            <progress aria-labelledby="sequence-export-progress-label" value={videoProgress.stage === "rendering" ? videoProgress.completed : undefined} max={videoProgress.stage === "rendering" ? Math.max(1, videoProgress.total) : undefined} />
          </div>}
          <fieldset className="sequence-export-ports" disabled={busy || stale}>
            <legend>Output ports</legend>
            {ports.map((port) => {
              const first = firstChannels.get(port.index);
              return <label key={port.index}>
                <input type="checkbox" checked={first !== undefined} onChange={(event) => { setSaved(null); setSelected(event.target.checked ? [...selected, port.index] : selected.filter((index) => index !== port.index)); }} />
                {port.label} ({port.channels} channels){first !== undefined ? ` · output ${selected.indexOf(port.index) + 1}, channels ${first}–${first + port.channels - 1}` : ""}
              </label>;
            })}
            {!busy && options !== null && ports.length === 0 && <p>Add controller outputs in Display Setup before exporting.</p>}
          </fieldset>
          {options !== null && <fieldset className="sequence-export-step" disabled={busy || stale}>
            <legend>FSEQ frame step</legend>
            <select value={customStep ? "custom" : String(stepMillis)} onChange={(event) => {
              setSaved(null);
              if (event.target.value === "custom") { setCustomStep(true); return; }
              setCustomStep(false); setStepMillis(Number(event.target.value));
            }}>
              {steps.map((step) => <option key={step} value={String(step)}>
                {step} ms ({Math.round(MILLIS_PER_SECOND / step)} fps){step === options.fseqStepMillis ? ", closest to the sequence" : ""}
              </option>)}
              <option value="custom">Custom</option>
            </select>
            {customStep && <label>
              <input type="number" min={1} max={FSEQ_MAX_STEP} step={1} value={stepMillis} onChange={(event) => { setSaved(null); setStepMillis(event.target.valueAsNumber); }} /> ms
            </label>}
          </fieldset>}
          {options !== null && <fieldset className="sequence-export-step" disabled={busy || stale}>
            <legend>Video frame rate</legend>
            <select value={String(videoFrameRate)} onChange={(event) => { setSaved(null); setVideoFrameRate(Number(event.target.value)); }}>
              {VIDEO_FRAME_RATES.map((rate) => <option key={rate} value={String(rate)}>{rate} fps</option>)}
            </select>
          </fieldset>}
          <div className="dialog-actions">
            <button type="button" disabled={busy} onClick={() => { setOrigin(null); }}>Close</button>
            <button type="button" disabled={busy || stale} onClick={() => { void save("mp4"); }}>{pending === "mp4" ? "Exporting video…" : "Save .mp4 video"}</button>
            <button type="button" disabled={busy || stale || selected.length === 0} onClick={() => { void save("donderseq"); }}>{pending === "donderseq" ? "Preparing…" : "Save .donderseq file"}</button>
            <button type="button" disabled={busy || stale || selected.length === 0 || !stepValid} onClick={() => { void save("fseq"); }}>{pending === "fseq" ? "Preparing…" : "Save .fseq file"}</button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  </>;
}

function videoProgressLabel(progress: VideoExportProgress): string {
  switch (progress.stage) {
    case "preparingAudio": return "Preparing the song…";
    case "rendering": return `Rendering video: ${Math.round(100 * progress.completed / Math.max(1, progress.total))}%`;
    case "saving": return "Saving video…";
  }
}

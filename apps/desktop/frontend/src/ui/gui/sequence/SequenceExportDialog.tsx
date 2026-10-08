import * as Dialog from "@radix-ui/react-dialog";
import { Download } from "lucide-react";
import { useState } from "react";
import { commands } from "../../../api";
import { useAppStore } from "../../../store";
import { THEME_METRICS } from "../../../theme";
import type { GuiDocumentRequest, SequenceExportOptions } from "../../../types";

/** Common FPP frame steps, offered beside the step closest to the authored rate. */
const FSEQ_PRESET_STEPS = [20, 25, 50];
const FSEQ_MAX_STEP = 255;
const MILLIS_PER_SECOND = 1000;

export function SequenceExportDialog() {
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const editing = useAppStore((state) => state.guiEditPending);
  const [origin, setOrigin] = useState<GuiDocumentRequest | null>(null);
  const [options, setOptions] = useState<SequenceExportOptions | null>(null);
  const [selected, setSelected] = useState<number[]>([]);
  const [stepMillis, setStepMillis] = useState(0);
  const [customStep, setCustomStep] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
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
    setOrigin(request); setOptions(null); setSelected([]); setCustomStep(false); setError(null); setSaved(null); setPending(true);
    try {
      const result = await commands.sequenceExportOptions(request);
      if (result.status === "error") throw new Error(result.error);
      setOptions(result.data);
      setStepMillis(result.data.fseqStepMillis);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(false); }
  };
  const save = async (format: "donderseq" | "fseq") => {
    if (origin === null || stale) return;
    setPending(true); setError(null); setSaved(null);
    try {
      const result = format === "fseq"
        ? await commands.exportFseqFile(origin, selected, stepMillis)
        : await commands.exportSequenceFile(origin, selected);
      if (result.status === "error") throw new Error(result.error);
      setSaved(result.data);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(false); }
  };
  return <>
    <button type="button" title="Export sequence" disabled={pending || editing || request === null || request.projectRevision !== revision} onClick={() => { void begin(); }}><Download size={THEME_METRICS.iconSizeCompact} /></button>
    <Dialog.Root open={origin !== null} onOpenChange={(open) => { if (!open && !pending) setOrigin(null); }}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content sequence-export-dialog">
          <Dialog.Title>Export sequence</Dialog.Title>
          <Dialog.Description>Choose the outputs to include, in order. An FSEQ file packs their channels back to back for FPP and other players. A .donderseq file is a compiled show whose payload outputs follow the same order. Donder controllers in the setup receive the current sequence automatically when you press Play.</Dialog.Description>
          {stale && <p role="alert">The project changed. Close and reopen export to use the current sequence and outputs.</p>}
          {error !== null && <p role="alert">{error}</p>}
          {saved !== null && <p role="status">Saved {saved}</p>}
          <fieldset className="sequence-export-ports" disabled={pending || stale}>
            <legend>Output ports</legend>
            {ports.map((port) => {
              const first = firstChannels.get(port.index);
              return <label key={port.index}>
                <input type="checkbox" checked={first !== undefined} onChange={(event) => { setSaved(null); setSelected(event.target.checked ? [...selected, port.index] : selected.filter((index) => index !== port.index)); }} />
                {port.label} ({port.channels} channels){first !== undefined ? ` · output ${selected.indexOf(port.index) + 1}, channels ${first}–${first + port.channels - 1}` : ""}
              </label>;
            })}
            {!pending && options !== null && ports.length === 0 && <p>Add controller outputs in Display Setup before exporting.</p>}
          </fieldset>
          {options !== null && <fieldset className="sequence-export-step" disabled={pending || stale}>
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
          <div className="dialog-actions">
            <button type="button" disabled={pending} onClick={() => { setOrigin(null); }}>Close</button>
            <button type="button" disabled={pending || stale || selected.length === 0} onClick={() => { void save("donderseq"); }}>{pending ? "Preparing…" : "Save .donderseq file"}</button>
            <button type="button" disabled={pending || stale || selected.length === 0 || !stepValid} onClick={() => { void save("fseq"); }}>{pending ? "Preparing…" : "Save .fseq file"}</button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  </>;
}

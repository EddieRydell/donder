import * as Dialog from "@radix-ui/react-dialog";
import { Download } from "lucide-react";
import { useState } from "react";
import { commands } from "../../../api";
import { useAppStore } from "../../../store";
import { THEME_METRICS } from "../../../theme";
import type { GuiDocumentRequest, SequenceExportPort } from "../../../types";

export function SequenceExportDialog() {
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const editing = useAppStore((state) => state.guiEditPending);
  const [origin, setOrigin] = useState<GuiDocumentRequest | null>(null);
  const [ports, setPorts] = useState<SequenceExportPort[]>([]);
  const [selected, setSelected] = useState<number[]>([]);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const stale = origin !== null && origin !== request;
  const begin = async () => {
    if (request === null || editing || request.projectRevision !== revision) return;
    setOrigin(request); setPorts([]); setSelected([]); setError(null); setSaved(null); setPending(true);
    try {
      const result = await commands.sequenceExportPorts(request);
      if (result.status === "error") throw new Error(result.error);
      setPorts(result.data);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(false); }
  };
  const save = async () => {
    if (origin === null || stale) return;
    setPending(true); setError(null); setSaved(null);
    try {
      const result = await commands.exportSequenceFile(origin, selected);
      if (result.status === "error") throw new Error(result.error);
      setSaved(result.data);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(false); }
  };
  return <>
    <button type="button" title="Export compiled sequence" disabled={pending || editing || request === null || request.projectRevision !== revision} onClick={() => { void begin(); }}><Download size={THEME_METRICS.iconSizeCompact} /></button>
    <Dialog.Root open={origin !== null} onOpenChange={(open) => { if (!open && !pending) setOrigin(null); }}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content sequence-export-dialog">
          <Dialog.Title>Export compiled sequence</Dialog.Title>
          <Dialog.Description>Choose the outputs to include. Their selection order becomes the controller payload's output order. Donder controllers in the setup receive the current sequence automatically when you press Play.</Dialog.Description>
          {stale && <p role="alert">The project changed. Close and reopen export to use the current sequence and outputs.</p>}
          {error !== null && <p role="alert">{error}</p>}
          {saved !== null && <p role="status">Saved {saved}</p>}
          <fieldset className="sequence-export-ports" disabled={pending || stale}>
            <legend>Output ports</legend>
            {ports.map((port) => <label key={port.index}>
              <input type="checkbox" checked={selected.includes(port.index)} onChange={(event) => { setSaved(null); setSelected(event.target.checked ? [...selected, port.index] : selected.filter((index) => index !== port.index)); }} />
              {port.label} ({port.channels} channels){selected.includes(port.index) ? ` · payload output ${selected.indexOf(port.index) + 1}` : ""}
            </label>)}
            {!pending && ports.length === 0 && <p>Add controller outputs in Display Setup before exporting.</p>}
          </fieldset>
          <div className="dialog-actions">
            <button type="button" disabled={pending} onClick={() => { setOrigin(null); }}>Close</button>
            <button type="button" disabled={pending || stale || selected.length === 0} onClick={() => { void save(); }}>{pending ? "Preparing…" : "Save .donderseq file"}</button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  </>;
}

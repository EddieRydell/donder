import { useState } from "react";

import { commands } from "../../../api";
import { runSnapshotCommand, useAppStore } from "../../../store";
import type { SetupController } from "../../../types";
import { NumberField } from "../setup/PatchInputs";

export function OutputTestForm({ document }: { document: SetupController }) {
  const [portId, setPortId] = useState<number | null>(null);
  const [channel, setChannel] = useState(1);
  const [count, setCount] = useState(1);
  const [value, setValue] = useState(32);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const output = useAppStore((state) => state.snapshot?.liveOutput);
  const guiPending = useAppStore((state) => state.guiEditPending);
  const port = document.ports.find((candidate) => candidate.id === portId);
  const active = output !== undefined && output.state !== "disabled" && output.state !== "error";

  return <details className="setup-light-editor">
    <summary>Test outputs</summary>
    <form className="setup-authoring-form" onSubmit={(event) => {
      event.preventDefault();
      const request = useAppStore.getState().guiRequest;
      if (request === null || port === undefined) return;
      setPending(true);
      void runSnapshotCommand(() => commands.startOutputTest(request, { port: port.id, startSlot: channel - 1, slotCount: count, value }))
        .then(() => { setError(null); }).catch((testError: unknown) => { setError(String(testError)); })
        .finally(() => { setPending(false); });
    }}>
      <p>Sends directly to this controller without a sequence or patch. The selected channels receive the value below; other channels on that port receive zero. This replaces live sequence output and stops automatically after 10 seconds.</p>
      {(error ?? output?.lastError) !== null && (error ?? output?.lastError) !== undefined && <p role="alert">{error ?? output?.lastError}</p>}
      <fieldset disabled={pending || guiPending || active}>
        <label>Test output<select required value={port?.id ?? ""} onChange={(event) => { setPortId(Number(event.target.value)); }}>
          <option value="" disabled>Choose an output</option>
          {document.ports.map((candidate) => <option key={candidate.id} value={candidate.id}>Port {candidate.id} ({candidate.slotCount} channels)</option>)}
        </select></label>
        <NumberField label="Test first channel" value={channel} min={1} {...(port === undefined ? {} : { max: port.slotCount })} onChange={setChannel} />
        <NumberField label="Test channel count" value={count} min={1} {...(port === undefined ? {} : { max: Math.max(0, port.slotCount - channel + 1) })} onChange={setCount} />
        <NumberField label="Test channel value" value={value} min={0} max={255} onChange={setValue} />
        <button type="submit">Start 10-second output test</button>
      </fieldset>
      {active && <button type="button" disabled={output.state === "stopping"} onClick={() => { void runSnapshotCommand(() => commands.setLiveOutputActive(false)).catch((testError: unknown) => { setError(String(testError)); }); }}>{output.state === "stopping" ? "Stopping output..." : "Stop output"}</button>}
      {output?.state === "testing" && <p role="status">Channel test is sending to this controller. Stop output to end it now.</p>}
    </form>
  </details>;
}

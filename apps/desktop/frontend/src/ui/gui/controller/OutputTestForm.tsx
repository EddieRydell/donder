import { useState } from "react";

import { commands } from "../../../api";
import { runSnapshotCommand, useAppStore } from "../../../store";
import type { SetupController } from "../../../types";
import { Button, Field, NumberField, Section, Select } from "../../kit";

const TEST_SECONDS = 10;
const MAX_VALUE = 255;

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
  const disabled = pending || guiPending || active;
  const shownError = error ?? output?.lastError ?? null;
  const start = () => {
    const request = useAppStore.getState().guiRequest;
    if (request === null || port === undefined) return;
    setPending(true);
    void runSnapshotCommand(() => commands.startOutputTest(request, { port: port.id, startSlot: channel - 1, slotCount: count, value }))
      .then(() => { setError(null); }).catch((testError: unknown) => { setError(String(testError)); })
      .finally(() => { setPending(false); });
  };

  return <Section title="Test outputs" defaultOpen={false}>
    {shownError !== null && <p role="alert" className="kit-status-error">{shownError}</p>}
    <Field label="Output">
      <Select name="Test output" value={port === undefined ? "" : String(port.id)} disabled={disabled} onChange={(id) => { setPortId(Number(id)); }} options={[
        ...(port === undefined ? [{ value: "", label: "Choose", disabled: true }] : []),
        ...document.ports.map((candidate) => ({ value: String(candidate.id), label: `Port ${candidate.id} (${candidate.slotCount} ch)` }))
      ]} />
    </Field>
    <Field label="First channel"><NumberField name="Test first channel" value={channel} step={1} min={1} {...(port === undefined ? {} : { max: port.slotCount })} integer disabled={disabled} onCommit={setChannel} /></Field>
    <Field label="Count"><NumberField name="Test channel count" value={count} step={1} min={1} {...(port === undefined ? {} : { max: Math.max(0, port.slotCount - channel + 1) })} integer disabled={disabled} onCommit={setCount} /></Field>
    <Field label="Value"><NumberField name="Test channel value" value={value} step={1} min={0} max={MAX_VALUE} integer disabled={disabled} onCommit={setValue} /></Field>
    {active
      ? <Button disabled={output.state === "stopping"} onClick={() => { void runSnapshotCommand(() => commands.setLiveOutputActive(false)).catch((testError: unknown) => { setError(String(testError)); }); }}>{output.state === "stopping" ? "Stopping…" : "Stop output"}</Button>
      : <Button disabled={disabled || port === undefined} tip={`Sends the value to these channels and zero to the rest of the port, replacing live output for ${TEST_SECONDS} seconds`} onClick={start}>Test</Button>}
  </Section>;
}

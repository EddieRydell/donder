import { useState } from "react";

import { commands, errorMessage } from "../../../api";
import { runSnapshotCommand, useAppStore } from "../../../store";
import type { SetupController } from "../../../types";
import { Button, Field, NumberField, Section } from "../../kit";

const TEST_SECONDS = 10;
const MAX_VALUE = 255;

type Universes = Extract<SetupController["config"], { type: "e131" | "artNet" }>["universes"];

/** Send one value to a range of an E1.31 or Art-Net controller's channels, counted across its universes. */
export function OutputTestForm({ universes }: { universes: Universes }) {
  const [channel, setChannel] = useState(1);
  const [count, setCount] = useState(1);
  const [value, setValue] = useState(32);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const output = useAppStore((state) => state.snapshot?.liveOutput);
  const guiPending = useAppStore((state) => state.guiEditPending);
  const channels = universes.count * universes.channels;
  const active = output !== undefined && output.state !== "disabled" && output.state !== "error";
  const disabled = pending || guiPending || active;
  const shownError = error ?? output?.lastError ?? null;
  const start = () => {
    const request = useAppStore.getState().guiRequest;
    if (request === null) return;
    setPending(true);
    void runSnapshotCommand(() => commands.startOutputTest(request, { startChannel: channel - 1, channelCount: count, value }))
      .then(() => { setError(null); }).catch((testError: unknown) => { setError(errorMessage(testError)); })
      .finally(() => { setPending(false); });
  };

  return <Section title="Test outputs" defaultOpen={false}>
    <Field label="First channel" tip="Counted across all the controller's universes"><NumberField name="Test first channel" value={channel} step={1} min={1} max={channels} integer disabled={disabled} onCommit={setChannel} /></Field>
    <Field label="Count"><NumberField name="Test channel count" value={count} step={1} min={1} max={Math.max(1, channels - channel + 1)} integer disabled={disabled} onCommit={setCount} /></Field>
    <Field label="Value"><NumberField name="Test channel value" value={value} step={1} min={0} max={MAX_VALUE} integer disabled={disabled} onCommit={setValue} /></Field>
    {active
      ? <Button disabled={output.state === "stopping"} onClick={() => { void runSnapshotCommand(() => commands.setLiveOutputActive(false)).catch((testError: unknown) => { setError(errorMessage(testError)); }); }}>{output.state === "stopping" ? "Stopping…" : "Stop output"}</Button>
      : <Button disabled={disabled} failed={shownError !== null} tip={shownError ?? `Sends the value to these channels and zero to the rest of the controller, replacing live output for ${TEST_SECONDS} seconds`} onClick={start}>Test</Button>}
  </Section>;
}

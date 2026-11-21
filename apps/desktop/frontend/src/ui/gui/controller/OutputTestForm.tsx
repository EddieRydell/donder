import { useState } from "react";

import { useAppStore } from "../../../store";
import type { SetupController } from "../../../types";
import { Button, Field, MenuButton, NumberField, Section } from "../../kit";
import { patternEntries, stopOutputTest, useOutputTesting } from "../outputTest";

type Universes = Extract<SetupController["config"], { type: "e131" | "artNet" }>["universes"];

/** Stream a test pattern on a range of an E1.31 or Art-Net controller's channels, counted across its universes, as RGB pixels. */
export function OutputTestForm({ universes }: { universes: Universes }) {
  const [channel, setChannel] = useState(1);
  const [count, setCount] = useState(universes.count * universes.channels);
  const output = useAppStore((state) => state.snapshot?.liveOutput);
  const guiPending = useAppStore((state) => state.guiEditPending);
  const testing = useOutputTesting();
  const channels = universes.count * universes.channels;
  const stopping = output?.state === "stopping";
  const disabled = guiPending || stopping;

  return <Section title="Test outputs" defaultOpen={false}>
    <Field label="First channel" tip="Counted across all the controller's universes"><NumberField name="Test first channel" value={channel} step={1} min={1} max={channels} integer disabled={disabled} onCommit={setChannel} /></Field>
    <Field label="Count"><NumberField name="Test channel count" value={count} step={1} min={1} max={Math.max(1, channels - channel + 1)} integer disabled={disabled} onCommit={setCount} /></Field>
    <MenuButton entries={patternEntries({ type: "channels", startChannel: channel - 1, channelCount: Math.min(count, channels - channel + 1) }, disabled)}>
      <Button disabled={disabled} tip="Lights these channels as RGB pixels and zero the rest of the controller, in place of live output until stopped">Test</Button>
    </MenuButton>
    {testing && <Button disabled={stopping} onClick={stopOutputTest}>{stopping ? "Stopping…" : "Stop test"}</Button>}
  </Section>;
}

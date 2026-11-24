import { Play, Square, Video } from "lucide-react";
import { useState } from "react";
import { commands, errorMessage } from "../../../api";
import { sameGuiDocument } from "../../../snapshotState";
import { runGuiEditCommand, runSnapshotCommand, useAppStore } from "../../../store";
import type { FixtureGuiDocument, GuiFixtureCapture } from "../../../types";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { Badge, Button, Field, NumberField, Section, Select } from "../../kit";
import { stopOutputTest, useOutputTesting } from "../outputTest";

/** RGB channels per pixel the capture pattern lights. */
const CHANNELS_PER_PIXEL = 3;

/**
 * Measures the fixture's shapes from a phone video of its pixels playing the capture pattern on a
 * controller's channels, and replaces its shapes with them in one undoable edit.
 */
export function FixtureCapture({ document, disabled }: { document: FixtureGuiDocument; disabled: boolean }) {
  const controllers = document.captureControllers;
  const request = useAppStore((state) => state.guiRequest);
  const stopping = useAppStore((state) => state.snapshot?.liveOutput.state === "stopping");
  const testing = useOutputTesting();
  const [chosen, setChosen] = useState<string | null>(null);
  const [channel, setChannel] = useState(1);
  const [pixels, setPixels] = useState(Math.max(1, document.renderPlan.pixels.length));
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [capture, setCapture] = useState<GuiFixtureCapture | null>(null);
  const controller = controllers.find((item) => guiObjectKey(item.sourceRef) === chosen) ?? controllers[0];
  if (controller === undefined) return null;
  const busy = disabled || pending || stopping || request === null;
  const firstChannel = Math.min(channel, controller.channelCount);
  const maxPixels = Math.max(1, Math.floor((controller.channelCount - firstChannel + 1) / CHANNELS_PER_PIXEL));
  const count = Math.min(pixels, maxPixels);

  const play = () => {
    if (request === null) return;
    void runSnapshotCommand(() => commands.startFixtureCapture(request, controller.sourceRef, firstChannel - 1, count)).catch(() => undefined);
  };
  const measure = async () => {
    if (request === null) return;
    const path = await commands.chooseCaptureVideo();
    if (path === null) return;
    setPending(true);
    setError(null);
    try {
      const measured = await commands.captureFixture(request, path, count);
      if (!sameGuiDocument(useAppStore.getState().guiRequest, request)) {
        throw new Error("The open fixture changed while the video was measured.");
      }
      const applied = await runGuiEditCommand((current) =>
        commands.applyGuiEdit(current, { type: "fixture", edit: { type: "setElements", elements: measured.elements } }));
      if (applied !== null) setCapture(measured);
    } catch (measureError: unknown) {
      setError(errorMessage(measureError));
    } finally {
      setPending(false);
    }
  };

  return <Section title="Capture" defaultOpen={false}>
    <Field label="Controller">
      <Select name="Capture controller" value={guiObjectKey(controller.sourceRef)} disabled={busy} onChange={setChosen}
        options={controllers.map((item) => ({ value: guiObjectKey(item.sourceRef), label: item.label }))} />
    </Field>
    <Field label="First channel" tip="Counted across all the controller's universes">
      <NumberField name="Capture first channel" value={firstChannel} step={1} min={1} max={controller.channelCount} integer disabled={busy} onCommit={setChannel} />
    </Field>
    <Field label="Pixels" tip="RGB pixels from the first channel">
      <NumberField name="Capture pixels" value={count} step={1} min={1} max={maxPixels} integer disabled={busy} onCommit={setPixels} />
    </Field>
    {testing
      ? <Button icon={Square} disabled={stopping} onClick={stopOutputTest}>{stopping ? "Stopping…" : "Stop pattern"}</Button>
      : <Button icon={Play} disabled={busy} tip="Blink each pixel's number in place of live output until stopped. Film from a tripod with the exposure locked until the pattern repeats twice." onClick={play}>Play pattern</Button>}
    <Button icon={Video} disabled={busy} tip="Replace the shapes with ones measured from an MP4 or MOV video of the pattern" onClick={() => { void measure(); }}>
      {pending ? "Measuring…" : "From video"}
    </Button>
    {capture !== null && <Badge tip="Pixels seen in the video; the rest were placed between their neighbors">{capture.found}/{capture.pixels}</Badge>}
    {error !== null && <div className="kit-status-error" role="alert">{error}</div>}
  </Section>;
}

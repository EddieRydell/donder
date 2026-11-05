import { useEffect } from "react";
import { Monitor, RadioTower, Square } from "lucide-react";
import { commands } from "../api";
import { installPerformKeys, refreshPerform, stopPerforming, usePerformStore } from "../perform";
import { runSnapshotCommand, useAppStore } from "../store";
import { Button, IconButton, Tip } from "./kit";

/** The performance in progress: its console, clock and held buttons. */
export function PerformBar({ projectEpoch, projectRevision }: { projectEpoch: number; projectRevision: number }) {
  const session = usePerformStore((store) => store.perform?.session ?? null);
  const liveOutput = useAppStore((store) => store.snapshot?.liveOutput ?? null);
  const previewOpen = useAppStore((store) => store.snapshot?.previewOpen ?? false);

  useEffect(() => installPerformKeys(), []);
  useEffect(() => { refreshPerform(); }, [projectEpoch, projectRevision]);

  if (session === null || liveOutput === null) return null;
  const liveActive = liveOutput.state !== "disabled" && liveOutput.state !== "error";
  return (
    <section className="perform-bar" aria-label="Performing">
      <span className="perform-bar-console">Performing {session.console.name}</span>
      <span className="perform-bar-clock">{formatElapsed(session.elapsedSeconds)}</span>
      {session.midi !== null && <InputStatus
        summary={session.midi.ports.length === 0 ? "No MIDI device found" : `MIDI: ${session.midi.ports.join(", ")}`}
        details={session.midi.ports}
        error={session.midi.error} />}
      {session.osc !== null && <InputStatus
        summary={session.osc.ports.length === 0 ? "OSC not listening" : `OSC on port ${session.osc.ports.join(", ")}`}
        details={session.osc.ports.map((port) => `UDP port ${port}`)}
        error={session.osc.error} />}
      <Tip label="Held buttons"><span className="perform-bar-held">
        {session.held.length === 0 ? "—" : session.held.map((button) => `${button.device}.${button.control}`).join(" ")}
      </span></Tip>
      <IconButton
        icon={RadioTower}
        label={liveOutput.lastError ?? `Live output: ${liveOutput.state}`}
        pressed={liveActive}
        disabled={liveOutput.state === "stopping"}
        onClick={() => void runSnapshotCommand(() => commands.setLiveOutputActive(!liveActive))}
      />
      <IconButton
        icon={Monitor}
        label="Preview"
        pressed={previewOpen}
        onClick={() => void runSnapshotCommand(() => commands.setPreviewWindowOpen(!previewOpen))}
      />
      <Button variant="ghost" icon={Square} tip="Stop performing" command="perform.stop" onClick={stopPerforming}>Stop</Button>
    </section>
  );
}

/** How an input driver is connected, and why it failed when it did. */
function InputStatus({ summary, details, error }: { summary: string; details: string[]; error: string | null }) {
  const shown = error === null ? [summary] : [summary, error];
  const tip = error === null ? details : [...details, error];
  const status = <span className={`perform-bar-input ${error === null ? "" : "error"}`}>{shown.join(" · ")}</span>;
  return tip.length === 0 ? status : <Tip label={tip.join(" · ")}>{status}</Tip>;
}

function formatElapsed(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  const rest = (seconds - minutes * 60).toFixed(1).padStart(4, "0");
  return `${minutes}:${rest}`;
}

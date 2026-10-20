import { useEffect } from "react";
import { Monitor, RadioTower, Square } from "lucide-react";
import { commands } from "../api";
import { installPerformKeys, refreshPerform, stopPerforming, usePerformStore } from "../perform";
import { runSnapshotCommand, useAppStore } from "../store";
import { THEME_METRICS } from "../theme";

/** The performance in progress: its rig, clock, switches and held buttons. */
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
      <span className="perform-bar-rig">Performing {session.rig.name}</span>
      <span className="perform-bar-clock">{formatElapsed(session.elapsedSeconds)}</span>
      {session.switches.map((item) => (
        <span key={item.name} className={`perform-bar-switch ${item.on ? "on" : ""}`}>{item.name}</span>
      ))}
      {session.midi !== null && <InputStatus
        summary={session.midi.ports.length === 0 ? "No MIDI device found" : `MIDI: ${session.midi.ports.join(", ")}`}
        details={session.midi.ports}
        error={session.midi.error} />}
      {session.osc !== null && <InputStatus
        summary={session.osc.ports.length === 0 ? "OSC not listening" : `OSC on port ${session.osc.ports.join(", ")}`}
        details={session.osc.ports.map((port) => `UDP port ${port}`)}
        error={session.osc.error} />}
      <span className="perform-bar-held" title="Held buttons">
        {session.held.length === 0 ? "No buttons held" : session.held.map((button) => `${button.device}.${button.control}`).join(" ")}
      </span>
      <button
        type="button"
        className={liveActive ? "active" : ""}
        title={liveOutput.lastError ?? `Live output: ${liveOutput.state}`}
        disabled={liveOutput.state === "stopping"}
        onClick={() => void runSnapshotCommand(() => commands.setLiveOutputActive(!liveActive))}
      >
        <RadioTower size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button
        type="button"
        className={previewOpen ? "active" : ""}
        title={previewOpen ? "Close preview" : "Open preview"}
        onClick={() => void runSnapshotCommand(() => commands.setPreviewWindowOpen(!previewOpen))}
      >
        <Monitor size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button type="button" title="Stop performing" onClick={stopPerforming}>
        <Square size={THEME_METRICS.iconSizeCompact} /> Stop
      </button>
    </section>
  );
}

/** How an input driver is connected, and why it failed when it did. */
function InputStatus({ summary, details, error }: { summary: string; details: string[]; error: string | null }) {
  const shown = error === null ? [summary] : [summary, error];
  const title = error === null ? details : [...details, error];
  return <span className={`perform-bar-input ${error === null ? "" : "error"}`} title={title.join("\n")}>{shown.join(" · ")}</span>;
}

function formatElapsed(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  const rest = (seconds - minutes * 60).toFixed(1).padStart(4, "0");
  return `${minutes}:${rest}`;
}

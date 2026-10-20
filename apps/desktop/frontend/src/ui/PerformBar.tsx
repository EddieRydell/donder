import { useEffect } from "react";
import { Monitor, Music, RadioTower, Square } from "lucide-react";
import { commands } from "../api";
import { installPerformKeys, playSong, refreshPerform, stopPerforming, stopSong, usePerformStore } from "../perform";
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
      {session.midi !== null && <span className={`perform-bar-midi ${session.midi.error === null ? "" : "error"}`} title={[...session.midi.ports, session.midi.error].filter((text) => text !== null).join("\n")}>
        {[session.midi.ports.length === 0 ? "No MIDI device found" : `MIDI: ${session.midi.ports.join(", ")}`, session.midi.error]
          .filter((text) => text !== null).join(" · ")}
      </span>}
      {session.song === null ? (
        <button type="button" title="Play a MIDI file into the rig, as if a pianist played it" onClick={() => void playSong()}>
          <Music size={THEME_METRICS.iconSizeCompact} /> Play MIDI song…
        </button>
      ) : (
        <span className="perform-bar-song" title="MIDI song">
          <Music size={THEME_METRICS.iconSizeCompact} />
          <span className="perform-bar-song-name" title={session.song.name}>{session.song.name}</span>
          {formatSongTime(session.song.positionSeconds)} / {formatSongTime(session.song.lengthSeconds)}
          <button type="button" title="Stop the song" onClick={stopSong}>
            <Square size={THEME_METRICS.iconSizeCompact} />
          </button>
        </span>
      )}
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

function formatSongTime(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${Math.floor(seconds - minutes * 60).toString().padStart(2, "0")}`;
}

function formatElapsed(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  const rest = (seconds - minutes * 60).toFixed(1).padStart(4, "0");
  return `${minutes}:${rest}`;
}

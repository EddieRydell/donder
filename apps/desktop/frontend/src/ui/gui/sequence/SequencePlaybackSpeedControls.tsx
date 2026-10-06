import { useState } from "react";

import { useSequenceEditorHost } from "../../../editor/host";
import type { PlaybackFrameTiming, PlaybackSpeed } from "../../../editor/types";

const SHOW_MICROS_PER_PERCENT = 10_000;
const MAX_SHOW_MICROS_PER_SECOND = 0xffff_ffff;
const SPEED_STOP_PERCENTS = [10, 25, 50, 75, 100, 150, 200];

/** The slider snaps to common speeds; the field accepts any positive speed. */
export function SequencePlaybackSpeedControls({ speed }: { speed: PlaybackSpeed }) {
  const { commands, runSnapshotCommand } = useSequenceEditorHost();
  const percent = speed.showMicrosPerSecond / SHOW_MICROS_PER_PERCENT;
  // A draft exists only while the field is being edited.
  const [draft, setDraft] = useState<string | null>(null);
  const apply = (next: PlaybackSpeed) => {
    if (next.showMicrosPerSecond === speed.showMicrosPerSecond && next.frameTiming === speed.frameTiming) return;
    void runSnapshotCommand(() => commands.audioSetPlaybackSpeed(next));
  };
  const applyPercent = (value: number) => {
    apply({ ...speed, showMicrosPerSecond: Math.round(value * SHOW_MICROS_PER_PERCENT) });
  };
  const commitDraft = () => {
    if (draft === null) return;
    setDraft(null);
    const value = Number(draft);
    const micros = Math.round(value * SHOW_MICROS_PER_PERCENT);
    if (draft.trim() === "" || !Number.isFinite(value) || micros < 1 || micros > MAX_SHOW_MICROS_PER_SECOND) return;
    applyPercent(value);
  };
  const setFrameTiming = (frameTiming: PlaybackFrameTiming) => {
    apply({ ...speed, frameTiming });
  };
  return (
    <div className="sequence-speed-controls" aria-label="Playback speed">
      <input
        type="range"
        className="sequence-speed-slider"
        title={`Playback speed ${formatPercent(percent)}%`}
        min={0}
        max={SPEED_STOP_PERCENTS.length - 1}
        step={1}
        value={nearestStopIndex(percent)}
        onChange={(event) => {
          const stop = SPEED_STOP_PERCENTS[Number(event.target.value)];
          if (stop !== undefined) applyPercent(stop);
        }}
      />
      <input
        type="number"
        className="sequence-speed-input"
        title="Playback speed percent"
        min={0}
        step="any"
        value={draft ?? formatPercent(percent)}
        onChange={(event) => {
          setDraft(event.target.value);
        }}
        onBlur={commitDraft}
        onKeyDown={(event) => {
          if (event.key === "Enter") commitDraft();
          if (event.key === "Escape") setDraft(null);
        }}
      />
      <span className="sequence-speed-unit">%</span>
      <div className="segmented-control sequence-frame-timing" role="group" aria-label="Frame rate during speed changes">
        <button
          type="button"
          className={speed.frameTiming === "scaled" ? "active" : ""}
          title="Scale the frame rate with speed to show what the sequence really looks like"
          onClick={() => {
            setFrameTiming("scaled");
          }}
        >
          Scaled fps
        </button>
        <button
          type="button"
          className={speed.frameTiming === "constant" ? "active" : ""}
          title="Keep the frame rate, sampling between authored frames"
          onClick={() => {
            setFrameTiming("constant");
          }}
        >
          Fixed fps
        </button>
      </div>
    </div>
  );
}

export function playbackSpeedRatio(speed: PlaybackSpeed) {
  return speed.showMicrosPerSecond / (100 * SHOW_MICROS_PER_PERCENT);
}

function nearestStopIndex(percent: number) {
  return SPEED_STOP_PERCENTS.reduce(
    (best, stop, index) => (Math.abs(stop - percent) < Math.abs((SPEED_STOP_PERCENTS[best] ?? stop) - percent) ? index : best),
    0
  );
}

function formatPercent(percent: number) {
  return String(Number(percent.toPrecision(6)));
}

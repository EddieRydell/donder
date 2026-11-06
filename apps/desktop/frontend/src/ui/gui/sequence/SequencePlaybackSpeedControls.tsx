import { commands } from "../../../api";
import { runSnapshotCommand } from "../../../store";
import type { PlaybackFrameTiming, PlaybackSpeed } from "../../../types";
import { NumberField, Select, Tip, type SelectOption } from "../../kit";

const SHOW_MICROS_PER_PERCENT = 10_000;
const MAX_SHOW_MICROS_PER_SECOND = 0xffff_ffff;
const SPEED_STOP_PERCENTS = [10, 25, 50, 75, 100, 150, 200];
const FRAME_TIMINGS: SelectOption<PlaybackFrameTiming>[] = [
  { value: "scaled", label: "Scaled fps" },
  { value: "constant", label: "Fixed fps" }
];

/** The slider snaps to common speeds; the field accepts any positive speed. */
export function SequencePlaybackSpeedControls({ speed }: { speed: PlaybackSpeed }) {
  const percent = speed.showMicrosPerSecond / SHOW_MICROS_PER_PERCENT;
  const apply = (next: PlaybackSpeed) => {
    if (next.showMicrosPerSecond === speed.showMicrosPerSecond && next.frameTiming === speed.frameTiming) return;
    void runSnapshotCommand(() => commands.audioSetPlaybackSpeed(next));
  };
  const applyPercent = (value: number) => {
    apply({ ...speed, showMicrosPerSecond: Math.round(value * SHOW_MICROS_PER_PERCENT) });
  };
  return (
    <div className="sequence-speed-controls" aria-label="Playback speed">
      <Tip label={`Playback speed ${formatPercent(percent)}%`}>
        <input
          type="range"
          className="sequence-speed-slider"
          aria-label="Playback speed"
          min={0}
          max={SPEED_STOP_PERCENTS.length - 1}
          step={1}
          value={nearestStopIndex(percent)}
          onChange={(event) => {
            const stop = SPEED_STOP_PERCENTS[Number(event.target.value)];
            if (stop !== undefined) applyPercent(stop);
          }}
        />
      </Tip>
      <NumberField
        className="sequence-speed-input"
        name="Playback speed percent"
        value={percent}
        defaultValue={100}
        min={1 / SHOW_MICROS_PER_PERCENT}
        max={MAX_SHOW_MICROS_PER_SECOND / SHOW_MICROS_PER_PERCENT}
        step={1}
        unit="%"
        format={formatPercent}
        onCommit={applyPercent}
      />
      <Select
        name="Frame rate during speed changes"
        value={speed.frameTiming}
        options={FRAME_TIMINGS}
        onChange={(frameTiming) => { apply({ ...speed, frameTiming }); }}
      />
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

import { useSequenceEditorHost, type SequenceEditorHost } from "../../../editor/host";
import { isMac } from "../../../platform";
import { ChevronLeft, ChevronRight, Locate, LocateFixed, LocateOff, Monitor, Music, Pause, Play, RadioTower, Repeat, SkipBack, Square, Workflow } from "lucide-react";

import { useEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";


import type { AppSnapshot, AudioTransportState, SequenceEditorDocument, SequenceFollowMode } from "../../../editor/types";


import { clamp, formatSeconds, type AudioTransportViewSnapshot } from "../shared";
import { requestOpenLayerGraph, requestTapMark } from "../../uiEvents";
import { THEME_METRICS } from "../../../theme";
import { SequencePlaybackSpeedControls, playbackSpeedRatio } from "./SequencePlaybackSpeedControls";

export function SequenceTransportControls({
  document,
  previewOpen
}: {
  document: SequenceEditorDocument;
  previewOpen: boolean;
}) {
  const host = useSequenceEditorHost();
  const { commands, store: useAppStore, runSnapshotCommand } = host;

  const transport = useAppStore((store) => store.snapshot?.audioTransport ?? null);
  const liveOutput = useAppStore((store) => store.snapshot?.liveOutput ?? null);
  const followMode = useAppStore((store) => store.snapshot?.settings.sequenceFollowMode ?? null);
  if (transport === null || liveOutput === null || followMode === null) return null;
  const FollowIcon = FOLLOW_MODES[followMode].Icon;
  const unsupported = isSequenceTransportUnsupported(document, transport);
  const activePlayback = isActiveAudioPlayback(transport.state);
  const liveActive = liveOutput.state !== "disabled" && liveOutput.state !== "error";
  const stepFrame = (direction: -1 | 1) => {
    stepSequenceFrame(host, document, transport.positionSeconds, transport.durationSeconds, direction);
  };
  return (
    <div
      className="sequence-toolbar"
      aria-label="Sequence transport"
      onKeyDownCapture={(event) => {
        handleSequencePlaybackShortcut(host, event, document, transport, unsupported);
      }}
    >
      <button
        type="button"
        title="Play"
        disabled={unsupported || activePlayback}
        onClick={() => void runSnapshotCommand(commands.audioPlay)}
      >
        <Play size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button
        type="button"
        title="Pause"
        disabled={unsupported || !activePlayback}
        onClick={() => void runSnapshotCommand(commands.audioPause)}
      >
        <Pause size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button type="button" title="Stop" disabled={unsupported} onClick={() => void runSnapshotCommand(commands.audioStop)}>
        <Square size={THEME_METRICS.iconSizeSmall} />
      </button>
      <button
        type="button"
        className={transport.looping ? "active" : ""}
        title={transport.looping ? "Stop looping (L)" : "Loop (L)"}
        disabled={unsupported}
        onClick={() => void runSnapshotCommand(() => commands.audioSetLooping(!transport.looping))}
      >
        <Repeat size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button
        type="button"
        className={followMode === "off" ? "" : "active"}
        title={`${FOLLOW_MODES[followMode].title} (F)`}
        onClick={() => { cycleSequenceFollowMode(host); }}
      >
        <FollowIcon size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button type="button" title="Rewind to zero" disabled={unsupported} onClick={() => void runSnapshotCommand(commands.audioRewindToZero)}>
        <SkipBack size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button
        type="button"
        title="Step backward"
        disabled={unsupported}
        onClick={() => {
          stepFrame(-1);
        }}
      >
        <ChevronLeft size={THEME_METRICS.iconSizeMedium} />
      </button>
      <button
        type="button"
        title="Step forward"
        disabled={unsupported}
        onClick={() => {
          stepFrame(1);
        }}
      >
        <ChevronRight size={THEME_METRICS.iconSizeMedium} />
      </button>
      <button
        type="button"
        className={liveActive ? "active" : ""}
        title={liveOutput.lastError ?? `Live output: ${liveOutput.state}`}
        hidden={!host.capabilities.liveOutput}
        disabled={liveOutput.state === "stopping"}
        onClick={() => void runSnapshotCommand(() => commands.setLiveOutputActive(!liveActive))}
      >
        <RadioTower size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button
        type="button"
        className={previewOpen ? "active" : ""}
        title={previewOpen ? "Close preview" : "Open preview"}
        hidden={!host.capabilities.previewWindow}
        onClick={() => void runSnapshotCommand(() => commands.setPreviewWindowOpen(!previewOpen))}
      >
        <Monitor size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button type="button" title="Open layer graph" onClick={requestOpenLayerGraph}>
        <Workflow size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button
        type="button"
        title="Choose audio"
        hidden={!host.capabilities.audioFile}
        onClick={() => void chooseAudioWithResizePrompt(host, document)}
      >
        <Music size={THEME_METRICS.iconSizeCompact} />
      </button>
      {host.capabilities.playbackSpeed ? <SequencePlaybackSpeedControls speed={transport.playbackSpeed} /> : null}
      <span className="sequence-time-readout" title="Stop returns the playhead to Home. Click the ruler to move Home; drag in it to set a playback range.">
        <SequenceTimeReadout transport={transport} durationSeconds={document.durationSeconds} />
        {liveOutput.state !== "disabled" ? ` | Live ${liveOutput.state} (${liveOutput.activeUniverseCount})` : ""}
      </span>
      {host.exportControls}
    </div>
  );
}

const FOLLOW_MODES = {
  off: { next: "page", title: "Follow playhead: off", Icon: LocateOff },
  page: { next: "continuous", title: "Follow playhead: page", Icon: Locate },
  continuous: { next: "off", title: "Follow playhead: continuous", Icon: LocateFixed }
} as const satisfies Record<SequenceFollowMode, { next: SequenceFollowMode; title: string; Icon: typeof Locate }>;

function cycleSequenceFollowMode(host: SequenceEditorHost) {
  const settings = host.store.getState().snapshot?.settings;
  if (settings === undefined) return;
  void host.runSnapshotCommand(() => host.commands.updateAppSettings({ ...settings, sequenceFollowMode: FOLLOW_MODES[settings.sequenceFollowMode].next }));
}

async function chooseAudioWithResizePrompt(host: SequenceEditorHost, document: SequenceEditorDocument) {
  const { commands, runGuiEditCommand } = host;

  const result = await runGuiEditCommand(commands.chooseSequenceAudio);
  if (result.document.type !== "sequence" || result.document.document.audio === null) return;
  const durationSeconds = await loadAudioDurationSeconds(host, result.document.document.audio.resolvedPath);
  if (durationSeconds === null || Math.abs(durationSeconds - document.durationSeconds) < 0.01) return;
  const resize = window.confirm(
    `Resize sequence to ${formatSeconds(durationSeconds)} to match ${result.document.document.audio.fileName}?`
  );
  if (!resize) return;
  await runGuiEditCommand((request) =>
    commands.applySequenceGuiEdit(request, {
      type: "setDuration",
      durationSeconds
    })
  );
}

function loadAudioDurationSeconds(host: SequenceEditorHost, path: string): Promise<number | null> {
  const { resolveAssetUrl: convertFileSrc } = host;

  return new Promise((resolve) => {
    const audio = new Audio();
    audio.preload = "metadata";
    audio.onloadedmetadata = () => {
      resolve(Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration : null);
    };
    audio.onerror = () => {
      resolve(null);
    };
    audio.src = convertFileSrc(path);
  });
}

/** The transport snapshot is an anchor; the readout extrapolates it like the playhead. */
function SequenceTimeReadout({ transport, durationSeconds }: { transport: AppSnapshot["audioTransport"]; durationSeconds: number }) {
  const live = useSequenceTransport(transport);
  return <>{formatSeconds(live.positionSeconds)} / {formatSeconds(transport.durationSeconds || durationSeconds)} | Home {formatSeconds(transport.homeSeconds)}</>;
}

type TransportAnchor = { transport: AppSnapshot["audioTransport"]; positionSeconds: number; anchoredAt: number };

/** The playhead now: the latest position, extrapolated from the snapshot's arrival while playing. */
function playheadSeconds(latest: AppSnapshot["audioTransport"], anchor: TransportAnchor) {
  if (!shouldAnimateTransportPosition(latest) || !shouldAnimateTransportPosition(anchor.transport)) return latest.positionSeconds;
  const elapsedSeconds = Math.max(0, transportExtrapolationSeconds(anchor.anchoredAt) - anchor.transport.startDelaySeconds) * playbackSpeedRatio(anchor.transport.playbackSpeed);
  return clamp(anchor.positionSeconds + elapsedSeconds, 0, anchor.transport.durationSeconds);
}

/** `playheadClockRef` receives a reader of the drawn playhead, for actions timed to it such as tapping marks. */
export function useSequenceTransport(transport: AppSnapshot["audioTransport"], playheadClockRef?: RefObject<(() => number) | null>): AudioTransportViewSnapshot {
  const [animatedPositionSeconds, setAnimatedPositionSeconds] = useState(transport.positionSeconds);
  const transportRef = useRef(transport);
  const anchor = useRef<TransportAnchor>({
    transport,
    positionSeconds: transport.positionSeconds,
    anchoredAt: 0
  });

  useEffect(() => {
    transportRef.current = transport;
    anchor.current = {
      transport,
      positionSeconds: transport.positionSeconds,
      anchoredAt: performance.now()
    };
  }, [transport]);

  useEffect(() => {
    let frame = 0;
    const tick = () => {
      const latest = transportRef.current;
      setAnimatedPositionSeconds(playheadSeconds(latest, anchor.current));
      if (!shouldAnimateTransportPosition(latest) || !shouldAnimateTransportPosition(anchor.current.transport)) return;
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => {
      window.cancelAnimationFrame(frame);
    };
  }, [transport.state, transport.positionSeconds]);

  useEffect(() => {
    if (playheadClockRef === undefined) return;
    playheadClockRef.current = () => playheadSeconds(transportRef.current, anchor.current);
    return () => {
      playheadClockRef.current = null;
    };
  }, [playheadClockRef]);

  return shouldAnimateTransportPosition(transport)
    ? {
        ...transport,
        positionSeconds: animatedPositionSeconds
      }
    : transport;
}

function isEditableShortcutTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  return target.closest("input, textarea, select") !== null;
}

export function handleSequencePlaybackShortcut(host: SequenceEditorHost,
  event: KeyboardEvent<HTMLElement>,
  document: SequenceEditorDocument,
  transport: AppSnapshot["audioTransport"],
  unsupported: boolean
) {
  const { commands, runSnapshotCommand } = host;

  if (unsupported || isEditableShortcutTarget(event.target)) return;
  // Mac keyboards have no Home key; Command-Left Arrow is the platform's line-start gesture.
  const rewind = event.key === "Home" || (isMac && event.metaKey && event.key === "ArrowLeft");
  // Modified keys belong to app commands such as Save, not to transport keys.
  if (!rewind && (event.metaKey || event.ctrlKey || event.altKey)) return;
  if (event.key === " ") {
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    void runSnapshotCommand(isActiveAudioPlayback(transport.state) ? commands.audioStop : commands.audioPlay);
  } else if (event.key.toLowerCase() === "s") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(commands.audioStop);
  } else if (event.key.toLowerCase() === "l") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(() => commands.audioSetLooping(!transport.looping));
  } else if (event.key.toLowerCase() === "f") {
    event.preventDefault();
    event.stopPropagation();
    cycleSequenceFollowMode(host);
  } else if (event.key.toLowerCase() === "m") {
    // M drops a mark at the playhead, so marks can be tapped along with the music.
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    requestTapMark();
  } else if (rewind) {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(commands.audioRewindToZero);
  } else if (event.key === "ArrowLeft") {
    event.preventDefault();
    event.stopPropagation();
    stepSequenceFrame(host, document, transport.positionSeconds, transport.durationSeconds, -1);
  } else if (event.key === "ArrowRight") {
    event.preventDefault();
    event.stopPropagation();
    stepSequenceFrame(host, document, transport.positionSeconds, transport.durationSeconds, 1);
  }
}

export function isSequenceTransportUnsupported(
  document: SequenceEditorDocument,
  transport: AppSnapshot["audioTransport"]
) {
  return document.durationSeconds <= 0 || transport.state === "unloaded" || transport.state === "error";
}

function isActiveAudioPlayback(state: AudioTransportState) {
  return state === "playing";
}

function shouldAnimateTransportPosition(transport: AudioTransportViewSnapshot) {
  return transport.state === "playing";
}

function transportExtrapolationSeconds(anchoredAt: number) {
  return anchoredAt > 0 ? (performance.now() - anchoredAt) / 1000 : 0;
}

function stepSequenceFrame(host: SequenceEditorHost, document: SequenceEditorDocument, positionSeconds: number, transportDurationSeconds: number, direction: -1 | 1) {
  const { commands, runSnapshotCommand } = host;

  const frameSeconds = 1 / Math.max(1, document.frameRate);
  const nextPositionSeconds = clamp(positionSeconds + direction * frameSeconds, 0, transportDurationSeconds || document.durationSeconds);
  void runSnapshotCommand(() => commands.audioSeek(nextPositionSeconds));
}

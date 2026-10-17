import { useSequenceEditorHost, type SequenceEditorHost } from "../../../editor/host";
import type { EditorCommands } from "../../../shortcuts";
import { ChevronLeft, ChevronRight, Locate, LocateFixed, LocateOff, Magnet, Monitor, Music, Pause, Play, RadioTower, Repeat, Scissors, SkipBack, Square, Workflow } from "lucide-react";
import { toggleSliceTool, useSequenceTool } from "./sequenceTool";

import { useEffect, useRef, useState, type RefObject } from "react";


import type { AppSnapshot, AudioTransportState, SequenceEditorDocument, SequenceFollowMode } from "../../../editor/types";


import { clamp, formatSeconds, type AudioTransportViewSnapshot } from "../shared";
import { requestOpenLayerGraph, requestTapMark } from "../../uiEvents";
import { THEME_METRICS } from "../../../theme";
import { useConfirmDialog, type Confirm } from "../../ConfirmDialog";
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
  const { confirm, dialog: confirmDialog } = useConfirmDialog();

  const transport = useAppStore((store) => store.snapshot?.audioTransport ?? null);
  const liveOutput = useAppStore((store) => store.snapshot?.liveOutput ?? null);
  const followMode = useAppStore((store) => store.snapshot?.settings.sequenceFollowMode ?? null);
  const snapping = useAppStore((store) => store.snapshot?.settings.sequenceSnap ?? null);
  const sliceTool = useSequenceTool((state) => state.tool === "slice");
  if (transport === null || liveOutput === null || followMode === null || snapping === null) return null;
  const FollowIcon = FOLLOW_MODES[followMode].Icon;
  const unsupported = isSequenceTransportUnsupported(document, transport);
  const activePlayback = isActiveAudioPlayback(transport.state);
  const liveActive = liveOutput.state !== "disabled" && liveOutput.state !== "error";
  const stepFrame = (direction: -1 | 1) => {
    stepSequenceFrame(host, document, transport.positionSeconds, transport.durationSeconds, direction);
  };
  return (
    <div className="sequence-toolbar" aria-label="Sequence transport">
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
        className={sliceTool ? "active" : ""}
        title={sliceTool ? "Stop slicing (C)" : "Slice clips (C)"}
        onClick={toggleSliceTool}
      >
        <Scissors size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button
        type="button"
        className={snapping ? "active" : ""}
        title={snapping ? "Snap: on (hold Alt to bypass)" : "Snap: off (hold Alt to snap)"}
        onClick={() => { toggleSequenceSnap(host); }}
      >
        <Magnet size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button
        type="button"
        title="Choose audio"
        hidden={!host.capabilities.audioFile}
        onClick={() => void chooseAudioWithResizePrompt(host, document, confirm)}
      >
        <Music size={THEME_METRICS.iconSizeCompact} />
      </button>
      {host.capabilities.playbackSpeed ? <SequencePlaybackSpeedControls speed={transport.playbackSpeed} /> : null}
      <span className="sequence-time-readout" title="Stop returns the playhead to Home. Click the ruler to move Home; drag in it to set a playback range.">
        <SequenceTimeReadout transport={transport} durationSeconds={document.durationSeconds} />
        {liveOutput.state !== "disabled" ? ` | Live ${liveOutput.state} (${liveOutput.activeUniverseCount})` : ""}
      </span>
      {host.exportControls}
      {confirmDialog}
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

function toggleSequenceSnap(host: SequenceEditorHost) {
  const settings = host.store.getState().snapshot?.settings;
  if (settings === undefined) return;
  void host.runSnapshotCommand(() => host.commands.updateAppSettings({ ...settings, sequenceSnap: !settings.sequenceSnap }));
}

async function chooseAudioWithResizePrompt(host: SequenceEditorHost, document: SequenceEditorDocument, confirm: Confirm) {
  const { commands, runGuiEditCommand } = host;

  const result = await runGuiEditCommand(commands.chooseSequenceAudio);
  const edited = result.change.type === "sequenceClips"
    ? result.change.document
    : result.change.document.type === "sequence" ? result.change.document.document : null;
  const audio = edited?.audio ?? null;
  if (audio === null) return;
  const durationSeconds = await loadAudioDurationSeconds(host, audio.resolvedPath);
  if (durationSeconds === null || Math.abs(durationSeconds - document.durationSeconds) < 0.01) return;
  const resize = await confirm({
    title: "Resize sequence?",
    description: `Resize the sequence to ${formatSeconds(durationSeconds)} to match ${audio.fileName}.`,
    confirmLabel: "Resize",
    danger: false
  });
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

/** The transport's keyboard commands, which act while the sequence's audio is loaded. */
export function sequenceTransportCommands(host: SequenceEditorHost, document: SequenceEditorDocument) {
  const { commands, runSnapshotCommand } = host;
  const transport = () => host.store.getState().snapshot?.audioTransport;
  const enabled = () => {
    const current = transport();
    return current !== undefined && !isSequenceTransportUnsupported(document, current);
  };
  const step = (direction: -1 | 1) => () => {
    const current = transport();
    if (current !== undefined) stepSequenceFrame(host, document, current.positionSeconds, current.durationSeconds, direction);
  };
  return {
    "transport.togglePlayback": { enabled, run: ({ repeat }) => {
      if (repeat) return;
      void runSnapshotCommand(isActiveAudioPlayback(transport()?.state ?? "unloaded") ? commands.audioStop : commands.audioPlay);
    } },
    "transport.stop": { enabled, run: () => { void runSnapshotCommand(commands.audioStop); } },
    "transport.toggleLooping": { enabled, run: () => { void runSnapshotCommand(() => commands.audioSetLooping(!(transport()?.looping ?? false))); } },
    "transport.cycleFollowMode": { enabled, run: () => { cycleSequenceFollowMode(host); } },
    // M drops a mark at the playhead, so marks can be tapped along with the music.
    "transport.tapMark": { enabled, run: ({ repeat }) => { if (!repeat) requestTapMark(); } },
    "transport.rewind": { enabled, run: () => { void runSnapshotCommand(commands.audioRewindToZero); } },
    "edit.moveLeft": { enabled, run: step(-1) },
    "edit.moveRight": { enabled, run: step(1) }
  } satisfies EditorCommands;
}

function isSequenceTransportUnsupported(
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

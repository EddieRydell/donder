import { useSequenceEditorHost, type SequenceEditorHost } from "../../../editor/host";
import type { EditorCommands } from "../../../shortcuts";
import { ChevronLeft, ChevronRight, Locate, LocateFixed, LocateOff, Magnet, Monitor, Music, Pause, Play, RadioTower, Repeat, Scissors, SkipBack, Square, Workflow } from "lucide-react";
import { toggleSliceTool, useSequenceTool } from "./sequenceTool";

import { useEffect, useRef, useState, type RefObject } from "react";


import type { AppSnapshot, AudioTransportState, SequenceGuiDocument, SequenceFollowMode } from "../../../editor/types";


import { clamp, formatSeconds, type AudioTransportViewSnapshot } from "../shared";
import { requestOpenLayerGraph, requestTapMark } from "../../uiEvents";
import { IconButton, Tip } from "../../kit";
import { SequencePlaybackSpeedControls, playbackSpeedRatio } from "./SequencePlaybackSpeedControls";

export function SequenceTransportControls({
  document,
  previewOpen
}: {
  document: SequenceGuiDocument;
  previewOpen: boolean;
}) {
  const host = useSequenceEditorHost();
  const { commands, store: useAppStore, runSnapshotCommand } = host;

  const transport = useAppStore((store) => store.snapshot?.audioTransport ?? null);
  const liveOutput = useAppStore((store) => store.snapshot?.liveOutput ?? null);
  const followMode = useAppStore((store) => store.snapshot?.settings.sequenceFollowMode ?? null);
  const snapping = useAppStore((store) => store.snapshot?.settings.sequenceSnap ?? null);
  const sliceTool = useSequenceTool((state) => state.tool === "slice");
  if (transport === null || liveOutput === null || followMode === null || snapping === null) return null;
  const unsupported = isSequenceTransportUnsupported(document, transport);
  const activePlayback = isActiveAudioPlayback(transport.state);
  const liveActive = liveOutput.state !== "disabled" && liveOutput.state !== "error";
  // Each failing controller, Donder or streamed, once.
  const liveFailures = [...new Set(liveOutput.controllers.flatMap((controller) => controller.lastError === null ? [] : [controller.lastError]))];
  const stepFrame = (direction: -1 | 1) => {
    stepSequenceFrame(host, document, transport.positionSeconds, transport.durationSeconds, direction);
  };
  return (
    <div className="sequence-toolbar" aria-label="Sequence transport">
      <IconButton icon={Play} label="Play" command="transport.togglePlayback" disabled={unsupported || activePlayback} onClick={() => void runSnapshotCommand(commands.audioPlay)} />
      <IconButton icon={Pause} label="Pause" command="transport.togglePlayback" disabled={unsupported || !activePlayback} onClick={() => void runSnapshotCommand(commands.audioPause)} />
      <IconButton icon={Square} label="Stop" command="transport.stop" disabled={unsupported} onClick={() => void runSnapshotCommand(commands.audioStop)} />
      <IconButton icon={Repeat} label="Loop" command="transport.toggleLooping" pressed={transport.looping} disabled={unsupported}
        onClick={() => void runSnapshotCommand(() => commands.audioSetLooping(!transport.looping))} />
      <IconButton icon={FOLLOW_MODES[followMode].Icon} label={FOLLOW_MODES[followMode].title} command="transport.cycleFollowMode" pressed={followMode !== "off"}
        onClick={() => { cycleSequenceFollowMode(host); }} />
      <IconButton icon={SkipBack} label="Rewind to start" command="transport.rewind" disabled={unsupported} onClick={() => void runSnapshotCommand(commands.audioRewindToZero)} />
      <IconButton icon={ChevronLeft} label="Step backward" command="edit.moveLeft" disabled={unsupported} onClick={() => { stepFrame(-1); }} />
      <IconButton icon={ChevronRight} label="Step forward" command="edit.moveRight" disabled={unsupported} onClick={() => { stepFrame(1); }} />
      {host.capabilities.liveOutput && (
        <IconButton
          icon={RadioTower}
          label={liveFailures.length > 0 ? liveFailures.join("; ") : liveOutput.lastError ?? `Live output: ${liveOutput.state}`}
          pressed={liveActive}
          disabled={liveOutput.state === "stopping"}
          onClick={() => void runSnapshotCommand(() => commands.setLiveOutputActive(!liveActive))}
        />
      )}
      {host.capabilities.previewWindow && (
        <IconButton icon={Monitor} label="Preview" pressed={previewOpen} onClick={() => void runSnapshotCommand(() => commands.setPreviewWindowOpen(!previewOpen))} />
      )}
      <IconButton icon={Workflow} label="Layer graph" command="view.openLayerGraph" onClick={requestOpenLayerGraph} />
      <IconButton icon={Scissors} label="Slice clips" command="sequence.toggleSliceTool" pressed={sliceTool} onClick={toggleSliceTool} />
      <IconButton icon={Magnet} label={snapping ? "Snap: on, Alt bypasses" : "Snap: off, Alt snaps"} pressed={snapping} onClick={() => { toggleSequenceSnap(host); }} />
      {host.capabilities.audioFile && <IconButton icon={Music} label="Choose audio" onClick={() => void chooseAudio(host, document)} />}
      {host.capabilities.playbackSpeed ? <SequencePlaybackSpeedControls speed={transport.playbackSpeed} /> : null}
      <Tip label="Stop returns to Home. Click the ruler to move Home; drag in it to set a playback range.">
        <span className="sequence-time-readout">
          <SequenceTimeReadout transport={transport} durationSeconds={document.durationSeconds} />
          {liveOutput.state !== "disabled" ? ` | Live ${liveOutput.state} (${liveOutput.activeControllerCount} controllers)` : ""}
        </span>
      </Tip>
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

function toggleSequenceSnap(host: SequenceEditorHost) {
  const settings = host.store.getState().snapshot?.settings;
  if (settings === undefined) return;
  void host.runSnapshotCommand(() => host.commands.updateAppSettings({ ...settings, sequenceSnap: !settings.sequenceSnap }));
}

/** Chooses the sequence's audio, then fits the sequence to its length; undo reverts the resize. */
async function chooseAudio(host: SequenceEditorHost, document: SequenceGuiDocument) {
  const { commands, runGuiEditCommand } = host;

  const result = await runGuiEditCommand(commands.chooseSequenceAudio);
  const edited = result.change.type === "sequenceClips"
    ? result.change.document
    : result.change.document.type === "sequence" ? result.change.document.document : null;
  const audio = edited?.audio ?? null;
  if (audio === null) return;
  const durationSeconds = await loadAudioDurationSeconds(host, audio.resolvedPath);
  if (durationSeconds === null || Math.abs(durationSeconds - document.durationSeconds) < 0.01) return;
  await runGuiEditCommand((request) =>
    commands.applyGuiEdit(request, { type: "sequence", edit: {
      type: "setDuration",
      durationSeconds
    } })
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
export function sequenceTransportCommands(host: SequenceEditorHost, document: SequenceGuiDocument) {
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
  // Alt+arrows seek to the nearest mark of any collection before or after the playhead.
  const jumpToMark = (direction: -1 | 1) => () => {
    const current = transport();
    if (current === undefined) return;
    const position = current.positionSeconds;
    const marks = document.markCollections.flatMap((collection) => collection.marksSeconds);
    // Within half a frame of the playhead counts as the current mark, so repeated presses move on.
    const halfFrame = 0.5 / Math.max(1, document.frameRate);
    const target = direction < 0
      ? Math.max(...marks.filter((mark) => mark < position - halfFrame))
      : Math.min(...marks.filter((mark) => mark > position + halfFrame));
    if (Number.isFinite(target)) void runSnapshotCommand(() => commands.audioSeek(target));
  };
  // I and O set the playback range's start or end at the playhead, keeping the other end.
  const setRangeEdge = (edge: "start" | "end") => () => {
    const current = transport();
    if (current === undefined) return;
    const position = current.positionSeconds;
    const startSeconds = edge === "start" ? position : current.range?.startSeconds ?? 0;
    const endSeconds = edge === "end" ? position : current.range?.endSeconds ?? current.durationSeconds;
    if (endSeconds > startSeconds) void runSnapshotCommand(() => commands.audioSetRange({ startSeconds, endSeconds }));
  };
  return {
    "transport.previousMark": { enabled, run: jumpToMark(-1) },
    "transport.nextMark": { enabled, run: jumpToMark(1) },
    "transport.setRangeIn": { enabled, run: setRangeEdge("start") },
    "transport.setRangeOut": { enabled, run: setRangeEdge("end") },
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
  document: SequenceGuiDocument,
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

function stepSequenceFrame(host: SequenceEditorHost, document: SequenceGuiDocument, positionSeconds: number, transportDurationSeconds: number, direction: -1 | 1) {
  const { commands, runSnapshotCommand } = host;

  const frameSeconds = 1 / Math.max(1, document.frameRate);
  const nextPositionSeconds = clamp(positionSeconds + direction * frameSeconds, 0, transportDurationSeconds || document.durationSeconds);
  void runSnapshotCommand(() => commands.audioSeek(nextPositionSeconds));
}

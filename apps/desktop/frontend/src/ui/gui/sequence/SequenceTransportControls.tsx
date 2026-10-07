import { useSequenceEditorHost, type SequenceEditorHost } from "../../../editor/host";
import { isMac } from "../../../platform";
import { ChevronLeft, ChevronRight, GitBranch, Monitor, Music, Pause, Play, RadioTower, SkipBack, Square } from "lucide-react";

import { useEffect, useRef, useState, type KeyboardEvent } from "react";


import type { AppSnapshot, AudioTransportState, SequenceEditorDocument } from "../../../editor/types";


import { clamp, formatSeconds, type AudioTransportViewSnapshot } from "../shared";
import { requestOpenLayerGraph, requestShowMarkCollection } from "../../uiEvents";
import { THEME_METRICS } from "../../../theme";
import { SequencePlaybackSpeedControls, playbackSpeedRatio } from "./SequencePlaybackSpeedControls";
import { defaultMarkColor } from "./marks";

type TransportAnchor = { transport: AppSnapshot["audioTransport"]; positionSeconds: number; anchoredAt: number };

/** The latest transport snapshot and when it arrived; the playhead and taps extrapolate from it. */
let latestTransportAnchor: TransportAnchor | null = null;
const TAP_COLLECTION_KEY = "taps";
let tapQueue: Promise<void> = Promise.resolve();

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
  if (transport === null || liveOutput === null) return null;
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
        <GitBranch size={THEME_METRICS.iconSizeCompact} />
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
      <span className="sequence-time-readout">
        <SequenceTimeReadout transport={transport} durationSeconds={document.durationSeconds} />
        {liveOutput.state !== "disabled" ? ` | Live ${liveOutput.state} (${liveOutput.activeUniverseCount})` : ""}
      </span>
      {host.exportControls}
    </div>
  );
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

export function useSequenceTransport(transport: AppSnapshot["audioTransport"]): AudioTransportViewSnapshot {
  const [animatedPositionSeconds, setAnimatedPositionSeconds] = useState(transport.positionSeconds);
  const transportRef = useRef(transport);
  const anchor = useRef({
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
    latestTransportAnchor = anchor.current;
  }, [transport]);

  useEffect(() => {
    let frame = 0;
    const tick = () => {
      const latest = transportRef.current;
      const current = anchor.current;
      if (!shouldAnimateTransportPosition(latest) || !shouldAnimateTransportPosition(current.transport)) {
        setAnimatedPositionSeconds(latest.positionSeconds);
        return;
      }
      setAnimatedPositionSeconds(extrapolatedTransportSeconds(current));
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => {
      window.cancelAnimationFrame(frame);
    };
  }, [transport.state, transport.positionSeconds]);

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
  } else if (event.key.toLowerCase() === "m") {
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    tapMark(host, transport);
  } else if (event.key.toLowerCase() === "s") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(commands.audioStop);
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

function extrapolatedTransportSeconds(anchor: TransportAnchor) {
  const elapsedSeconds = Math.max(0, transportExtrapolationSeconds(anchor.anchoredAt) - anchor.transport.startDelaySeconds) * playbackSpeedRatio(anchor.transport.playbackSpeed);
  return clamp(anchor.positionSeconds + elapsedSeconds, 0, anchor.transport.durationSeconds);
}

/** Records a mark at the playhead in the Taps collection, creating it on the first tap. Taps run in order. */
function tapMark(host: SequenceEditorHost, transport: AppSnapshot["audioTransport"]) {
  const { commands, store, runGuiEditCommand } = host;
  const anchor = latestTransportAnchor;
  const timeSeconds = shouldAnimateTransportPosition(transport) && anchor !== null && shouldAnimateTransportPosition(anchor.transport)
    ? extrapolatedTransportSeconds(anchor)
    : transport.positionSeconds;
  tapQueue = tapQueue.then(async () => {
    const guiDocument = store.getState().guiDocument;
    const collections = guiDocument?.type === "sequence" ? guiDocument.document.markCollections : [];
    if (!collections.some((collection) => collection.key === TAP_COLLECTION_KEY)) {
      await runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, {
        type: "createMarkCollection",
        name: TAP_COLLECTION_KEY,
        color: defaultMarkColor(collections.length)
      }));
    }
    await runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, {
      type: "addMark",
      collectionKey: TAP_COLLECTION_KEY,
      timeSeconds
    }));
    requestShowMarkCollection(TAP_COLLECTION_KEY);
  }).catch((error: unknown) => {
    store.getState().setError(error instanceof Error ? error.message : String(error));
  });
}

function stepSequenceFrame(host: SequenceEditorHost, document: SequenceEditorDocument, positionSeconds: number, transportDurationSeconds: number, direction: -1 | 1) {
  const { commands, runSnapshotCommand } = host;

  const frameSeconds = 1 / Math.max(1, document.frameRate);
  const nextPositionSeconds = clamp(positionSeconds + direction * frameSeconds, 0, transportDurationSeconds || document.durationSeconds);
  void runSnapshotCommand(() => commands.audioSeek(nextPositionSeconds));
}

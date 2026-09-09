import { convertFileSrc } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, GitBranch, Monitor, Music, Pause, Play, RadioTower, SkipBack, Square } from "lucide-react";

import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { commands } from "../../../api";

import type { AppSnapshot, AudioTransportState, SequenceEditorDocument } from "../../../types";

import { runGuiEditCommand, runSnapshotCommand, useAppStore } from "../../../store";

import { clamp, formatSeconds, type AudioTransportViewSnapshot } from "../shared";
import { requestOpenLayerGraph } from "../../uiEvents";
import { THEME_METRICS } from "../../../theme";
import { SequenceExportDialog } from "./SequenceExportDialog";

export function SequenceTransportControls({
  document,
  previewOpen
}: {
  document: SequenceEditorDocument;
  previewOpen: boolean;
}) {
  const transport = useAppStore((store) => store.snapshot?.audioTransport ?? null);
  const liveOutput = useAppStore((store) => store.snapshot?.liveOutput ?? null);
  if (transport === null || liveOutput === null) return null;
  const unsupported = isSequenceTransportUnsupported(document, transport);
  const activePlayback = isActiveAudioPlayback(transport.state);
  const liveActive = liveOutput.state !== "disabled" && liveOutput.state !== "error";
  const stepFrame = (direction: -1 | 1) => {
    stepSequenceFrame(document, transport.positionSeconds, transport.durationSeconds, direction);
  };
  return (
    <div
      className="sequence-toolbar"
      aria-label="Sequence transport"
      onKeyDownCapture={(event) => {
        handleSequencePlaybackShortcut(event, document, transport, unsupported);
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
      <button type="button" title="Open layer graph" onClick={requestOpenLayerGraph}>
        <GitBranch size={THEME_METRICS.iconSizeCompact} />
      </button>
      <button
        type="button"
        title="Choose audio"
        onClick={() => void chooseAudioWithResizePrompt(document)}
      >
        <Music size={THEME_METRICS.iconSizeCompact} />
      </button>
      <span className="sequence-time-readout">
        {formatSeconds(transport.positionSeconds)} / {formatSeconds(transport.durationSeconds || document.durationSeconds)} | Home {formatSeconds(transport.homeSeconds)}
        {liveOutput.state !== "disabled" ? ` | Live ${liveOutput.state} (${liveOutput.activeUniverseCount})` : ""}
      </span>
      <SequenceExportDialog />
    </div>
  );
}

async function chooseAudioWithResizePrompt(document: SequenceEditorDocument) {
  const result = await runGuiEditCommand(commands.chooseSequenceAudio);
  if (result.document.type !== "sequence" || result.document.document.audio === null) return;
  const durationSeconds = await loadAudioDurationSeconds(result.document.document.audio.resolvedPath);
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

function loadAudioDurationSeconds(path: string): Promise<number | null> {
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
      const elapsedSeconds = transportExtrapolationSeconds(current.anchoredAt);
      setAnimatedPositionSeconds(clamp(current.positionSeconds + elapsedSeconds, 0, current.transport.durationSeconds));
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
  if (target.closest(".cm-editor")) return true;
  return target.closest("input, textarea, select") !== null;
}

export function handleSequencePlaybackShortcut(
  event: KeyboardEvent<HTMLElement>,
  document: SequenceEditorDocument,
  transport: AppSnapshot["audioTransport"],
  unsupported: boolean
) {
  if (unsupported || isEditableShortcutTarget(event.target)) return;
  if (event.key === " ") {
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    void runSnapshotCommand(isActiveAudioPlayback(transport.state) ? commands.audioStop : commands.audioPlay);
  } else if (event.key.toLowerCase() === "s") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(commands.audioStop);
  } else if (event.key === "Home") {
    event.preventDefault();
    event.stopPropagation();
    void runSnapshotCommand(commands.audioRewindToZero);
  } else if (event.key === "ArrowLeft") {
    event.preventDefault();
    event.stopPropagation();
    stepSequenceFrame(document, transport.positionSeconds, transport.durationSeconds, -1);
  } else if (event.key === "ArrowRight") {
    event.preventDefault();
    event.stopPropagation();
    stepSequenceFrame(document, transport.positionSeconds, transport.durationSeconds, 1);
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

function stepSequenceFrame(document: SequenceEditorDocument, positionSeconds: number, transportDurationSeconds: number, direction: -1 | 1) {
  const frameSeconds = 1 / Math.max(1, document.frameRate);
  const nextPositionSeconds = clamp(positionSeconds + direction * frameSeconds, 0, transportDurationSeconds || document.durationSeconds);
  void runSnapshotCommand(() => commands.audioSeek(nextPositionSeconds));
}

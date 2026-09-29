import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import WaveSurfer from "wavesurfer.js";
import SpectrogramPlugin from "wavesurfer.js/plugins/spectrogram";

import type { AppSettings, SequenceAudio } from "../../../types";
import { opaqueRgbBytes } from "../../../color";
import { clamp } from "../shared";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";

type SequenceWaveformProps = {
  audio: SequenceAudio | null;
  settings: AppSettings | null;
  left: number;
  top: number;
  width: number;
  height: number;
  pxPerSecond: number;
  scrollXSeconds: number;
};

type WaveSurferInstance = { path: string; wavesurfer: WaveSurfer };

const DEFAULT_SPECTROGRAM_SETTINGS = {
  timeResolutionMs: 10,
  fftSize: 2048
} as const;

const SPECTROGRAM_GAIN_DECIBELS = -6;
const SPECTROGRAM_RANGE_DECIBELS = 60;
const SPECTROGRAM_COLOR_MAP = buildSpectrogramColorMap();

export function SequenceWaveform({
  audio,
  settings,
  left,
  top,
  width,
  height,
  pxPerSecond,
  scrollXSeconds
}: SequenceWaveformProps) {
  const container = useRef<HTMLDivElement | null>(null);
  const activeInstance = useRef<WaveSurferInstance | null>(null);
  const previousView = useRef<{ wavesurfer: WaveSurfer; height: number; pxPerSecond: number; scrollXSeconds: number } | null>(null);
  const [instance, setInstance] = useState<WaveSurferInstance | null>(null);
  const spectrogramEnabled = settings?.sequenceSpectrogramEnabled ?? false;
  const spectrogramTimeResolutionMs = settings?.sequenceSpectrogramTimeResolutionMs ?? DEFAULT_SPECTROGRAM_SETTINGS.timeResolutionMs;
  const spectrogramFftSize = settings?.sequenceSpectrogramFftSize ?? DEFAULT_SPECTROGRAM_SETTINGS.fftSize;

  useEffect(() => {
    const host = container.current;
    const path = audio?.exists === true ? audio.resolvedPath : null;
    if (host === null || path === null) return;

    const controller = new AbortController();
    const wavesurfer = WaveSurfer.create({
      container: host,
      height: THEME_METRICS.visualMinSize,
      waveColor: THEME_COLORS.accent,
      progressColor: THEME_COLORS.accent,
      fillParent: false,
      hideScrollbar: true,
      interact: false,
      autoScroll: false,
      autoCenter: false
    });
    const current = { path, wavesurfer };
    activeInstance.current = current;
    const unsubscribe = wavesurfer.on("error", (error) => {
      console.error("Sequence audio visualization failed", error);
    });

    void (async () => {
      const response = await fetch(convertFileSrc(path), { signal: controller.signal });
      if (!response.ok) throw new Error(`Audio request failed with status ${response.status}`);
      await wavesurfer.loadBlob(await response.blob());
      if (activeInstance.current === current) setInstance(current);
    })().catch((error: unknown) => {
      if (activeInstance.current === current) console.error("Sequence audio visualization failed", error);
    });

    return () => {
      controller.abort();
      unsubscribe();
      if (activeInstance.current === current) activeInstance.current = null;
      if (previousView.current?.wavesurfer === wavesurfer) previousView.current = null;
      wavesurfer.destroy();
    };
  }, [audio?.exists, audio?.resolvedPath]);

  useEffect(() => {
    if (instance === null || instance !== activeInstance.current || instance.path !== audio?.resolvedPath || !spectrogramEnabled) return;
    const buffer = instance.wavesurfer.getDecodedData();
    if (buffer === null) return;

    const fftSize = clamp(spectrogramFftSize, 512, 16384);
    const hopSize = Math.max(1, Math.min(fftSize - 1, Math.round(buffer.sampleRate * spectrogramTimeResolutionMs / 1000)));
    const channelCount = Math.max(1, buffer.numberOfChannels);
    const plugin = SpectrogramPlugin.create({
      rendering: "windowed",
      height: Math.max(THEME_METRICS.visualMinSize, height / channelCount),
      fftSamples: fftSize,
      fftSize,
      noverlap: fftSize - hopSize,
      scale: "linear",
      gainDB: SPECTROGRAM_GAIN_DECIBELS,
      rangeDB: SPECTROGRAM_RANGE_DECIBELS,
      colorMap: SPECTROGRAM_COLOR_MAP,
      splitChannels: true,
      useWebWorker: true,
      fallbackToMainThread: false
    });
    const unsubscribe = plugin.on("error", (error) => {
      console.error("Sequence spectrogram rendering failed", error);
    });
    instance.wavesurfer.registerPlugin(plugin);

    return () => {
      unsubscribe();
      instance.wavesurfer.unregisterPlugin(plugin);
    };
  }, [audio?.resolvedPath, height, instance, spectrogramEnabled, spectrogramFftSize, spectrogramTimeResolutionMs]);

  useEffect(() => {
    if (instance === null || instance !== activeInstance.current || instance.path !== audio?.resolvedPath) return;
    const wavesurfer = instance.wavesurfer;
    const previous = previousView.current;
    if (previous?.wavesurfer !== wavesurfer || previous.height !== height) {
      wavesurfer.setOptions({ height: Math.max(THEME_METRICS.visualMinSize, height) });
    }
    if (previous?.wavesurfer !== wavesurfer || previous.pxPerSecond !== pxPerSecond) {
      wavesurfer.zoom(pxPerSecond);
    }
    if (previous?.wavesurfer !== wavesurfer || previous.scrollXSeconds !== scrollXSeconds) {
      wavesurfer.setScrollTime(scrollXSeconds);
    }
    previousView.current = { wavesurfer, height, pxPerSecond, scrollXSeconds };
  }, [audio?.resolvedPath, height, instance, pxPerSecond, scrollXSeconds]);

  if (audio?.exists !== true) return null;

  return (
    <div
      ref={container}
      className={`sequence-audio-visualization${spectrogramEnabled ? " is-spectrogram" : ""}`}
      style={{ left, top, width, height }}
      aria-hidden="true"
    />
  );
}

function buildSpectrogramColorMap(): number[][] {
  const baseColor = opaqueRgbBytes(THEME_COLORS.spectrogram);
  const highlightColor = opaqueRgbBytes(THEME_COLORS.white);
  return Array.from({ length: 256 }, (_, value) => {
    const intensity = value / 255;
    if (intensity === 0) return [0, 0, 0, 0];
    const highlightProgress = intensity < THEME_METRICS.spectrogramHighlightThreshold
      ? 0
      : (intensity - THEME_METRICS.spectrogramHighlightThreshold) / (1 - THEME_METRICS.spectrogramHighlightThreshold);
    const highlightAlpha = Math.pow(highlightProgress, THEME_METRICS.spectrogramHighlightPower);
    const baseWeight = intensity * (1 - highlightAlpha);
    const weight = baseWeight + highlightAlpha;
    return [
      (baseColor[0] * baseWeight + highlightColor[0] * highlightAlpha) / (weight * 255),
      (baseColor[1] * baseWeight + highlightColor[1] * highlightAlpha) / (weight * 255),
      (baseColor[2] * baseWeight + highlightColor[2] * highlightAlpha) / (weight * 255),
      weight
    ];
  });
}

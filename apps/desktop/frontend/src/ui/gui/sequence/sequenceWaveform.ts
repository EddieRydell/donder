import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import WaveSurfer from "wavesurfer.js";
import SpectrogramPlugin from "wavesurfer.js/plugins/spectrogram";

import type { AppSettings, SequenceAudio } from "../../../types";
import { clamp } from "../shared";
import { opaqueRgbBytes } from "../../../color";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";

type WaveformLevel = { samplesPerPeak: number; mins: Float32Array; maxes: Float32Array };
type PreparedSpectrogram = { plugin: SpectrogramPlugin; hopSize: number };
type Spectrogram = {
  timeStepSeconds: number;
  frequencyBinCount: number;
  columnCount: number;
  raster: HTMLCanvasElement | null;
};
type WaveformAudio = {
  durationSeconds: number;
  sampleRate: number;
  samples: Float32Array;
  levels: WaveformLevel[];
  spectrogram: Spectrogram | null;
};
export type WaveformState = { key: string | null; audio: WaveformAudio | null };

type AudioVisualizationSettings = {
  sequenceWaveformResolutionMs: number;
  sequenceSpectrogramEnabled: boolean;
  sequenceSpectrogramTimeResolutionMs: number;
  sequenceSpectrogramFftSize: number;
};

const WAVEFORM_CACHE_LIMIT = 4;
const MAX_SPECTROGRAM_COLUMNS = 16384;
const SPECTROGRAM_GAIN_DECIBELS = -6;
const SPECTROGRAM_RANGE_DECIBELS = 60;
const waveformCache = new Map<string, { request: Promise<WaveformAudio | null>; lastUsed: number }>();
let waveformCacheAccess = 1;

const DEFAULT_AUDIO_SETTINGS: AudioVisualizationSettings = {
  sequenceWaveformResolutionMs: 5,
  sequenceSpectrogramEnabled: false,
  sequenceSpectrogramTimeResolutionMs: 10,
  sequenceSpectrogramFftSize: 2048
};

export function useSequenceWaveform(audio: SequenceAudio | null, settings: AppSettings | null): WaveformState {
  const waveformResolutionMs = settings?.sequenceWaveformResolutionMs ?? DEFAULT_AUDIO_SETTINGS.sequenceWaveformResolutionMs;
  const spectrogramEnabled = settings?.sequenceSpectrogramEnabled ?? DEFAULT_AUDIO_SETTINGS.sequenceSpectrogramEnabled;
  const spectrogramTimeResolutionMs = settings?.sequenceSpectrogramTimeResolutionMs ?? DEFAULT_AUDIO_SETTINGS.sequenceSpectrogramTimeResolutionMs;
  const spectrogramFftSize = settings?.sequenceSpectrogramFftSize ?? DEFAULT_AUDIO_SETTINGS.sequenceSpectrogramFftSize;
  const visualizationSettings = useMemo(
    () => ({
      sequenceWaveformResolutionMs: waveformResolutionMs,
      sequenceSpectrogramEnabled: spectrogramEnabled,
      sequenceSpectrogramTimeResolutionMs: spectrogramTimeResolutionMs,
      sequenceSpectrogramFftSize: spectrogramFftSize
    }),
    [spectrogramEnabled, spectrogramFftSize, spectrogramTimeResolutionMs, waveformResolutionMs]
  );
  const key = audio?.exists === true
    ? JSON.stringify({ path: audio.resolvedPath, ...visualizationSettings })
    : null;
  const [state, setState] = useState<WaveformState>({ key, audio: null });

  useEffect(() => {
    if (key === null || audio?.exists !== true) return;
    let cancelled = false;
    let cached = waveformCache.get(key);
    if (cached === undefined) {
      cached = {
        request: decodeWaveformPeaks(audio.resolvedPath, visualizationSettings),
        lastUsed: waveformCacheAccess++
      };
      waveformCache.set(key, cached);
      evictWaveformCache();
    } else {
      cached.lastUsed = waveformCacheAccess++;
    }
    void cached.request.then((waveform) => {
      if (!cancelled) setState({ key, audio: waveform });
    });
    return () => {
      cancelled = true;
    };
  }, [audio, key, visualizationSettings]);

  return state.key === key ? state : { key, audio: null };
}

function evictWaveformCache() {
  while (waveformCache.size > WAVEFORM_CACHE_LIMIT) {
    let oldest: [string, number] | null = null;
    for (const [key, entry] of waveformCache) {
      if (oldest === null || entry.lastUsed < oldest[1]) oldest = [key, entry.lastUsed];
    }
    if (oldest === null) return;
    waveformCache.delete(oldest[0]);
  }
}

async function decodeWaveformPeaks(path: string, settings: AudioVisualizationSettings): Promise<WaveformAudio | null> {
  let container: HTMLDivElement | null = null;
  let waveSurfer: WaveSurfer | null = null;
  try {
    const response = await fetch(convertFileSrc(path));
    if (!response.ok) return null;
    container = document.createElement("div");
    container.className = "sequence-waveform-decoder";
    document.body.appendChild(container);
    waveSurfer = WaveSurfer.create({
      container,
      fillParent: false,
      hideScrollbar: true,
      interact: false
    });
    await waveSurfer.loadBlob(await response.blob());
    const buffer = waveSurfer.getDecodedData();
    if (buffer === null) return null;
    const spectrogram = settings.sequenceSpectrogramEnabled ? createSpectrogramPlugin(buffer.sampleRate, settings) : null;
    if (spectrogram !== null) waveSurfer.registerPlugin(spectrogram.plugin);
    return await buildWaveformAudio(buffer, spectrogram, settings);
  } catch {
    return null;
  } finally {
    waveSurfer?.destroy();
    container?.remove();
  }
}

async function buildWaveformAudio(
  buffer: AudioBuffer,
  spectrogram: PreparedSpectrogram | null,
  settings: AudioVisualizationSettings
): Promise<WaveformAudio> {
  const samples = downmixAudio(buffer);
  const samplesPerPeak = Math.max(1, Math.round(buffer.sampleRate * settings.sequenceWaveformResolutionMs / 1000));
  const levels: WaveformLevel[] = [buildWaveformLevel(samples, samplesPerPeak)];
  while ((levels[levels.length - 1]?.mins.length ?? 0) > 1) {
    const previous = levels[levels.length - 1];
    if (previous === undefined) break;
    levels.push(coarsenWaveformLevel(previous));
  }
  return {
    durationSeconds: buffer.duration,
    sampleRate: buffer.sampleRate,
    samples,
    levels,
    spectrogram: spectrogram === null ? null : await buildSpectrogram(spectrogram.plugin, spectrogram.hopSize, buffer.sampleRate)
  };
}

function downmixAudio(buffer: AudioBuffer): Float32Array {
  const samples = new Float32Array(buffer.length);
  const channelCount = buffer.numberOfChannels;
  if (channelCount === 0) return samples;
  for (let channel = 0; channel < channelCount; channel += 1) {
    const data = buffer.getChannelData(channel);
    for (let index = 0; index < samples.length; index += 1) {
      samples[index] = (samples[index] ?? 0) + (data[index] ?? 0) / channelCount;
    }
  }
  return samples;
}

function buildWaveformLevel(samples: Float32Array, samplesPerPeak: number): WaveformLevel {
  const bucketCount = Math.max(1, Math.ceil(samples.length / samplesPerPeak));
  const mins = new Float32Array(bucketCount);
  const maxes = new Float32Array(bucketCount);
  for (let bucket = 0; bucket < bucketCount; bucket += 1) {
    const start = bucket * samplesPerPeak;
    const end = Math.min(samples.length, start + samplesPerPeak);
    let min = 0;
    let max = 0;
    for (let index = start; index < end; index += 1) {
      const sample = samples[index] ?? 0;
      min = Math.min(min, sample);
      max = Math.max(max, sample);
    }
    mins[bucket] = min;
    maxes[bucket] = max;
  }
  return { samplesPerPeak, mins, maxes };
}

function createSpectrogramPlugin(sampleRate: number, settings: AudioVisualizationSettings): PreparedSpectrogram {
  const fftSize = clamp(settings.sequenceSpectrogramFftSize, 512, 16384);
  const hopSize = Math.max(1, Math.min(fftSize - 1, Math.round(sampleRate * settings.sequenceSpectrogramTimeResolutionMs / 1000)));
  return {
    plugin: SpectrogramPlugin.create({
      rendering: "full",
      fftSamples: fftSize,
      fftSize,
      noverlap: fftSize - hopSize,
      scale: "linear",
      gainDB: SPECTROGRAM_GAIN_DECIBELS,
      rangeDB: SPECTROGRAM_RANGE_DECIBELS,
      splitChannels: true,
      useWebWorker: true,
      fallbackToMainThread: false
    }),
    hopSize
  };
}

function coarsenWaveformLevel(level: WaveformLevel): WaveformLevel {
  const bucketCount = Math.ceil(level.mins.length / 2);
  const mins = new Float32Array(bucketCount);
  const maxes = new Float32Array(bucketCount);
  for (let bucket = 0; bucket < bucketCount; bucket += 1) {
    const left = bucket * 2;
    const right = left + 1;
    mins[bucket] = Math.min(level.mins[left] ?? 0, level.mins[right] ?? level.mins[left] ?? 0);
    maxes[bucket] = Math.max(level.maxes[left] ?? 0, level.maxes[right] ?? level.maxes[left] ?? 0);
  }
  return { samplesPerPeak: level.samplesPerPeak * 2, mins, maxes };
}

async function buildSpectrogram(
  spectrogramPlugin: SpectrogramPlugin,
  hopSize: number,
  sampleRate: number
): Promise<Spectrogram> {
  const frequencies = await spectrogramPlugin.getFrequenciesData();
  const channels = frequencies ?? [];
  const frameCount = Math.max(0, ...channels.map((channel) => channel.length));
  const frequencyBinCount = channels.length === 0 ? 0 : Math.min(...channels.map((channel) => channel[0]?.length ?? 0));
  if (frameCount === 0 || frequencyBinCount === 0) {
    return { timeStepSeconds: hopSize / sampleRate, frequencyBinCount: 0, columnCount: 0, raster: null };
  }
  const sourceColumnCount = Math.min(frameCount, MAX_SPECTROGRAM_COLUMNS);
  const frameStride = Math.max(1, Math.ceil(frameCount / sourceColumnCount));
  const columnCount = Math.ceil(frameCount / frameStride);
  const values = new Uint8Array(columnCount * Math.max(0, frequencyBinCount));
  for (let column = 0; column < columnCount; column += 1) {
    const frame = Math.min(frameCount - 1, column * frameStride);
    for (let bin = 0; bin < frequencyBinCount; bin += 1) {
      let value = 0;
      let count = 0;
      for (const channel of channels) {
        const frameValues = channel[frame];
        if (frameValues === undefined) continue;
        value += frameValues[bin] ?? 0;
        count += 1;
      }
      values[column * frequencyBinCount + bin] = count === 0 ? 0 : Math.round(value / count);
    }
  }

  return {
    timeStepSeconds: (hopSize * frameStride) / sampleRate,
    frequencyBinCount,
    columnCount,
    raster: buildSpectrogramRaster(values, columnCount, frequencyBinCount)
  };
}

function buildSpectrogramRaster(values: Uint8Array, columnCount: number, frequencyBinCount: number): HTMLCanvasElement {
  const raster = document.createElement("canvas");
  raster.width = columnCount;
  raster.height = Math.max(1, Math.ceil(THEME_METRICS.sequenceMaxAudioStripHeight));
  const context = raster.getContext("2d");
  if (context === null) throw new Error("Unable to create the spectrogram raster context");
  const image = context.createImageData(raster.width, raster.height);
  const baseColor = opaqueRgbBytes(THEME_COLORS.spectrogram);
  const highlightColor = opaqueRgbBytes(THEME_COLORS.white);
  const rasterRows = raster.height;
  for (let row = 0; row < rasterRows; row += 1) {
    const highBin = Math.min(frequencyBinCount, Math.max(1, Math.ceil(((rasterRows - row) * frequencyBinCount) / rasterRows)));
    const lowBin = Math.max(0, Math.min(highBin - 1, Math.floor(((rasterRows - row - 1) * frequencyBinCount) / rasterRows)));
    for (let column = 0; column < columnCount; column += 1) {
      let value = 0;
      for (let bin = lowBin; bin < highBin; bin += 1) {
        value = Math.max(value, values[column * frequencyBinCount + bin] ?? 0);
      }
      const intensity = value / 255;
      if (intensity === 0) continue;
      const highlightProgress = intensity < THEME_METRICS.spectrogramHighlightThreshold
        ? 0
        : (intensity - THEME_METRICS.spectrogramHighlightThreshold) / (1 - THEME_METRICS.spectrogramHighlightThreshold);
      const highlightAlpha = Math.pow(highlightProgress, THEME_METRICS.spectrogramHighlightPower);
      const baseWeight = intensity * (1 - highlightAlpha);
      const weight = baseWeight + highlightAlpha;
      const offset = (row * columnCount + column) * 4;
      image.data[offset] = Math.round((baseColor[0] * baseWeight + highlightColor[0] * highlightAlpha) / weight);
      image.data[offset + 1] = Math.round((baseColor[1] * baseWeight + highlightColor[1] * highlightAlpha) / weight);
      image.data[offset + 2] = Math.round((baseColor[2] * baseWeight + highlightColor[2] * highlightAlpha) / weight);
      image.data[offset + 3] = Math.round(weight * 255);
    }
  }
  context.putImageData(image, 0, 0);
  return raster;
}

export function drawWaveformStrip(
  ctx: CanvasRenderingContext2D,
  audio: WaveformAudio | null,
  left: number,
  top: number,
  width: number,
  height: number,
  durationSeconds: number,
  pxPerSecond: number,
  scrollXSeconds: number,
  colors: { grid: string; accent: string }
) {
  drawAudioStripBackground(ctx, left, top, width, height, colors.grid);
  if (audio !== null && audio.durationSeconds > 0 && audio.levels.length > 0) {
    const samplesPerPixel = audio.sampleRate / pxPerSecond;
    if (samplesPerPixel <= THEME_METRICS.waveformRawSampleThreshold && audio.samples.length > 0) {
      drawWaveformSamples(ctx, audio, left, width, height, top, durationSeconds, pxPerSecond, scrollXSeconds, colors.accent);
      return;
    }
    const level = audio.levels.find((item) => item.samplesPerPeak >= samplesPerPixel) ?? audio.levels[audio.levels.length - 1];
    if (level !== undefined) drawWaveformLevel(ctx, level, audio, left, width, height, top, durationSeconds, pxPerSecond, scrollXSeconds, colors.accent);
  }
}

export function drawSpectrogramStrip(
  ctx: CanvasRenderingContext2D,
  audio: WaveformAudio | null,
  left: number,
  top: number,
  width: number,
  height: number,
  durationSeconds: number,
  pxPerSecond: number,
  scrollXSeconds: number,
  colors: { grid: string; spectrogram: string; spectrogramHighlight: string }
) {
  drawAudioStripBackground(ctx, left, top, width, height, colors.grid);
  const spectrogram = audio?.spectrogram;
  if (spectrogram === null || spectrogram === undefined || spectrogram.columnCount === 0 || spectrogram.raster === null || audio === null) return;
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, top, width, height);
  ctx.clip();
  const visibleEndSeconds = Math.min(durationSeconds, audio.durationSeconds, scrollXSeconds + width / pxPerSecond);
  const sourceStart = clamp(Math.floor(scrollXSeconds / spectrogram.timeStepSeconds) - 1, 0, spectrogram.columnCount);
  const sourceEnd = clamp(Math.ceil(visibleEndSeconds / spectrogram.timeStepSeconds) + 1, sourceStart, spectrogram.columnCount);
  if (sourceEnd > sourceStart) {
    const destinationX = left + (sourceStart * spectrogram.timeStepSeconds - scrollXSeconds) * pxPerSecond;
    const destinationEndX = left + (sourceEnd * spectrogram.timeStepSeconds - scrollXSeconds) * pxPerSecond;
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(
      spectrogram.raster,
      sourceStart,
      0,
      sourceEnd - sourceStart,
      spectrogram.raster.height,
      destinationX,
      top,
      destinationEndX - destinationX,
      height
    );
  }
  ctx.restore();
}

function drawAudioStripBackground(ctx: CanvasRenderingContext2D, left: number, top: number, width: number, height: number, grid: string) {
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, top, width, height);
  ctx.clip();
  ctx.strokeStyle = grid;
  ctx.beginPath();
  ctx.moveTo(left, top + height / 2 + THEME_METRICS.visualHairlineOffset);
  ctx.lineTo(left + width, top + height / 2 + THEME_METRICS.visualHairlineOffset);
  ctx.stroke();
  ctx.restore();
}

function drawWaveformLevel(ctx: CanvasRenderingContext2D, level: WaveformLevel, audio: WaveformAudio, left: number, width: number, height: number, top: number, durationSeconds: number, pxPerSecond: number, scrollXSeconds: number, color: string) {
  const clipEnd = Math.min(durationSeconds, audio.durationSeconds);
  const first = Math.max(0, Math.floor((Math.max(0, scrollXSeconds) * audio.sampleRate) / level.samplesPerPeak));
  const last = Math.min(level.mins.length - 1, Math.ceil((Math.min(clipEnd, scrollXSeconds + width / pxPerSecond) * audio.sampleRate) / level.samplesPerPeak));
  const centerY = top + height / 2;
  const amplitude = Math.max(THEME_METRICS.visualMinSize, height / 2 - THEME_METRICS.waveformAmplitudeInset);
  if (last < first) return;
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, top, width, height);
  ctx.clip();
  ctx.fillStyle = color;
  const firstSeconds = (first * level.samplesPerPeak) / audio.sampleRate;
  ctx.moveTo(left + (firstSeconds - scrollXSeconds) * pxPerSecond, centerY);
  for (let index = first; index <= last; index += 1) {
    const seconds = (index * level.samplesPerPeak) / audio.sampleRate;
    const x = left + (seconds - scrollXSeconds) * pxPerSecond;
    if (seconds > clipEnd) break;
    ctx.lineTo(x, centerY - (level.maxes[index] ?? 0) * amplitude);
  }
  for (let index = last; index >= first; index -= 1) {
    const seconds = (index * level.samplesPerPeak) / audio.sampleRate;
    if (seconds > clipEnd) continue;
    const x = left + (seconds - scrollXSeconds) * pxPerSecond;
    ctx.lineTo(x, centerY - (level.mins[index] ?? 0) * amplitude);
  }
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

function drawWaveformSamples(ctx: CanvasRenderingContext2D, audio: WaveformAudio, left: number, width: number, height: number, top: number, durationSeconds: number, pxPerSecond: number, scrollXSeconds: number, color: string) {
  const clipEnd = Math.min(durationSeconds, audio.durationSeconds);
  const first = Math.max(0, Math.floor(Math.max(0, scrollXSeconds) * audio.sampleRate));
  const last = Math.min(audio.samples.length - 1, Math.ceil(Math.min(clipEnd, scrollXSeconds + width / pxPerSecond) * audio.sampleRate));
  if (last < first) return;
  const centerY = top + height / 2;
  const amplitude = Math.max(THEME_METRICS.visualMinSize, height / 2 - THEME_METRICS.waveformAmplitudeInset);
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, top, width, height);
  ctx.clip();
  ctx.fillStyle = color;
  ctx.moveTo(left + (first / audio.sampleRate - scrollXSeconds) * pxPerSecond, centerY);
  for (let index = first; index <= last; index += 1) {
    const x = left + (index / audio.sampleRate - scrollXSeconds) * pxPerSecond;
    ctx.lineTo(x, centerY - Math.max(0, audio.samples[index] ?? 0) * amplitude);
  }
  for (let index = last; index >= first; index -= 1) {
    const x = left + (index / audio.sampleRate - scrollXSeconds) * pxPerSecond;
    ctx.lineTo(x, centerY - Math.min(0, audio.samples[index] ?? 0) * amplitude);
  }
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

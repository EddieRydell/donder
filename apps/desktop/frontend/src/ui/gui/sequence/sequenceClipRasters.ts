import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";

// Runtime evaluates effects, the desktop worker schedules/caches and exposes
// RGBA payloads, and this hook decodes and draws them for the sequence UI.

import { commands } from "../../../api";
import type { AppSettings, SequenceClipRaster, SequenceEditorDocument } from "../../../types";
import { useAppStore } from "../../../store";
import type { SequenceClipLayout } from "./sequenceSelection";

type ClipRasterState = {
  requestKey: string;
  projectRevision: number | null;
  rasters: Map<string, DecodedClipRaster>;
  expectedRasterKeys: Map<number, string>;
  errors: Set<number>;
};

type DecodedClipRaster = {
  signature: string;
  image: CanvasImageSource;
  columns: number;
  rows: number;
  requestRows: number;
  byteLength: number;
  lastUsed: number;
};

type QueuedClipRasterDecode = {
  payload: SequenceClipRaster;
  keyContext: ClipRasterKeyContext;
};

const CLIP_RASTER_REQUEST_THROTTLE_MS = 50;
const CLIP_RASTER_DECODE_CHUNK_SIZE = 2;
const CLIP_RASTER_DECODED_BYTE_BUDGET = 64 * 1024 * 1024;
type ClipRasterRequestItem = { effectId: number; displayColumnCount: number; requestedColumns: number; requestedRows: number };
type ClipRasterKeyContext = { rasterSettingsKey: string; requestedColumns: number; requestedRows: number };

export function useSequenceClipRasters(
  document: SequenceEditorDocument,
  visibleClips: SequenceClipLayout[],
  laneHeight: number,
  settings: AppSettings | null
): ClipRasterState {
  const projectRevision = useAppStore((store) => store.snapshot?.projectRevision ?? null);
  const requestKey = `${document.path}:${document.objectKey}`;
  const rasterSettings = settings?.effectRaster ?? null;
  const rasterSettingsKey = rasterSettings === null
    ? "unavailable"
    : `${rasterSettings.renderScale}:${rasterSettings.maxColumns}:${rasterSettings.maxRows}:${rasterSettings.minFrameStride}`;
  const rasterRequestKey = `${requestKey}:${rasterSettingsKey}`;
  const effectIds = useMemo(() => document.effects.map((effect) => effect.id), [document.effects]);
  const effectIdsKey = effectIds.join(",");
  const visibleRequestItems = useMemo(() => {
    if (rasterSettings === null) return [];
    const dpr = (window.devicePixelRatio || 1) * rasterSettings.renderScale;
    const displayRowCount = Math.max(1, Math.ceil(laneHeight * dpr));
    const items: ClipRasterRequestItem[] = [];
    const requested = new Set<number>();
    for (const clip of visibleClips) {
      if (requested.has(clip.effect.id)) continue;
      const displayColumnCount = Math.max(1, Math.ceil(clip.rect.width * dpr));
      const durationFrames = Math.max(1, clip.effect.durationSeconds * document.frameRate);
      items.push({
        effectId: clip.effect.id,
        displayColumnCount,
        requestedColumns: Math.min(displayColumnCount, Math.ceil(durationFrames / rasterSettings.minFrameStride), rasterSettings.maxColumns),
        requestedRows: displayRowCount
      });
      requested.add(clip.effect.id);
    }
    return items;
  }, [document.frameRate, laneHeight, rasterSettings, visibleClips]);
  const visibleRequestItemsKey = visibleRequestItems.map((item) => `${item.effectId}:${item.displayColumnCount}:${item.requestedColumns}:${item.requestedRows}`).join(",");
  const visibleRequestItemsRef = useRef<ClipRasterRequestItem[]>(visibleRequestItems);
  const rasters = useRef<Map<string, DecodedClipRaster>>(new Map());
  const expectedRasterKeys = useRef<Map<number, string>>(new Map());
  const rasterCacheAccess = useRef(1);
  const errors = useRef<Set<number>>(new Set());
  const cachedRequestKey = useRef(rasterRequestKey);
  const projectRevisionRef = useRef(projectRevision);
  const [state, setState] = useState<ClipRasterState>({
    requestKey: rasterRequestKey,
    projectRevision,
    rasters: new Map(),
    expectedRasterKeys: new Map(),
    errors: new Set()
  });

  useEffect(() => {
    visibleRequestItemsRef.current = visibleRequestItems;
  }, [visibleRequestItems]);

  useEffect(() => {
    projectRevisionRef.current = projectRevision;
  }, [projectRevision]);

  useEffect(() => {
    if (projectRevision === null || rasterSettings === null) return;
    const abortController = new AbortController();
    let pollTimeout: number | null = null;
    let requestTimeout: number | null = null;
    let decodeFrame: number | null = null;
    const decodeQueue: QueuedClipRasterDecode[] = [];
    let decoding = false;
    const displayRowCount = Math.max(1, Math.ceil(laneHeight * (window.devicePixelRatio || 1) * rasterSettings.renderScale));
    if (cachedRequestKey.current !== rasterRequestKey) {
      cachedRequestKey.current = rasterRequestKey;
      rasters.current.clear();
      expectedRasterKeys.current.clear();
      errors.current.clear();
    }
    const effectIdSet = new Set(effectIds);
    for (const [effectId, rasterKey] of [...expectedRasterKeys.current]) {
      if (!effectIdSet.has(effectId)) {
        expectedRasterKeys.current.delete(effectId);
        rasters.current.delete(rasterKey);
      }
    }
    for (const effectId of [...errors.current]) {
      if (!effectIdSet.has(effectId)) errors.current.delete(effectId);
    }

    const publishState = (nextProjectRevision: number) => {
      setState({
        requestKey: rasterRequestKey,
        projectRevision: nextProjectRevision,
        rasters: new Map(rasters.current),
        expectedRasterKeys: new Map(expectedRasterKeys.current),
        errors: new Set(errors.current)
      });
    };
    if (cachedRequestKey.current === rasterRequestKey && rasters.current.size === 0 && expectedRasterKeys.current.size === 0 && errors.current.size === 0) {
      publishState(projectRevision);
    }

    const scheduleDecode = (nextProjectRevision: number) => {
      if (decoding) return;
      decoding = true;
      const decodeNextChunk = async () => {
        if (clipRasterRequestCancelled(abortController.signal)) return;
        for (let index = 0; index < CLIP_RASTER_DECODE_CHUNK_SIZE; index += 1) {
          const queued = decodeQueue.shift();
          if (queued === undefined) break;
          const raster = queued.payload;
          try {
            const image = await decodeClipRaster(raster);
            if (clipRasterRequestCancelled(abortController.signal)) return;
            if (!Object.is(nextProjectRevision, projectRevisionRef.current)) return;
            const rasterKey = clipRasterKey(document.path, document.objectKey, raster.effectId, raster.signature, queued.keyContext);
            rasters.current.set(rasterKey, {
              signature: raster.signature,
              image,
              columns: raster.columns,
              rows: raster.rows,
              requestRows: queued.keyContext.requestedRows,
              byteLength: raster.columns * raster.rows * 4,
              lastUsed: rasterCacheAccess.current++
            });
            expectedRasterKeys.current.set(raster.effectId, rasterKey);
            evictDecodedClipRasters(rasters.current, new Set(expectedRasterKeys.current.values()));
            errors.current.delete(raster.effectId);
          } catch {
            const rasterKey = expectedRasterKeys.current.get(raster.effectId);
            if (rasterKey !== undefined) rasters.current.delete(rasterKey);
            expectedRasterKeys.current.delete(raster.effectId);
            errors.current.add(raster.effectId);
          }
        }
        publishState(nextProjectRevision);
        if (decodeQueue.length === 0) {
          decoding = false;
          decodeFrame = null;
          return;
        }
        decodeFrame = window.requestAnimationFrame(() => void decodeNextChunk());
      };
      decodeFrame = window.requestAnimationFrame(() => void decodeNextChunk());
    };

    const visibleRequestRasterItems = () => {
      const existingEffectIds = new Set(effectIds);
      const requested = new Set<number>();
      const items: ClipRasterRequestItem[] = [];
      for (const item of visibleRequestItemsRef.current) {
        if (!existingEffectIds.has(item.effectId) || requested.has(item.effectId)) continue;
        const rasterKey = expectedRasterKeys.current.get(item.effectId);
        const raster = rasterKey === undefined ? undefined : rasters.current.get(rasterKey);
        if (raster !== undefined) raster.lastUsed = rasterCacheAccess.current++;
        items.push(item);
        requested.add(item.effectId);
      }
      return items;
    };

    if (effectIds.length === 0) {
      publishState(projectRevision);
      return;
    }

    const pollResults = async (requestId: number, requestComplete: boolean, requestContexts: Map<number, ClipRasterKeyContext>): Promise<boolean> => {
      const batch = await commands.takeSequenceClipRasterResults({
        projectRevision,
        path: document.path,
        view: "sequence",
        objectKey: document.objectKey
      }, requestId);
      if (clipRasterRequestCancelled(abortController.signal)) return false;
      for (const raster of batch.ready) {
        const keyContext = requestContexts.get(raster.effectId);
        if (keyContext !== undefined) decodeQueue.push({ payload: raster, keyContext });
      }
      for (const error of batch.errors) {
        const rasterKey = expectedRasterKeys.current.get(error.effectId);
        if (rasterKey !== undefined) rasters.current.delete(rasterKey);
        expectedRasterKeys.current.delete(error.effectId);
        errors.current.add(error.effectId);
      }
      for (const unavailable of batch.unavailable) {
        const rasterKey = expectedRasterKeys.current.get(unavailable.effectId);
        if (rasterKey !== undefined) rasters.current.delete(rasterKey);
        expectedRasterKeys.current.delete(unavailable.effectId);
        errors.current.delete(unavailable.effectId);
      }
      if (decodeQueue.length > 0) scheduleDecode(batch.projectRevision);
      else publishState(batch.projectRevision);
      const complete = requestComplete || batch.complete || batch.projectRevision !== projectRevision;
      if (!complete) {
        return await new Promise((resolve) => {
          pollTimeout = window.setTimeout(() => {
            void pollResults(requestId, false, requestContexts).then(resolve);
          }, 100);
        });
      }
      return batch.projectRevision === projectRevision;
    };

    const requestRasters = async (requestItems: ClipRasterRequestItem[]): Promise<boolean> => {
      if (requestItems.length === 0) return true;
      const requestContexts = new Map<number, ClipRasterKeyContext>();
      const response = await commands.requestSequenceClipRasters({
        projectRevision,
        path: document.path,
        view: "sequence",
        objectKey: document.objectKey,
        items: requestItems.map((item) => {
          const rasterKeyContext = {
            rasterSettingsKey,
            requestedColumns: item.requestedColumns,
            requestedRows: item.requestedRows
          };
          requestContexts.set(item.effectId, rasterKeyContext);
          const expectedRasterKey = expectedRasterKeys.current.get(item.effectId) ?? null;
          const cached = expectedRasterKey === null ? null : rasters.current.get(expectedRasterKey) ?? null;
          const signature = cached !== null && decodedClipRasterSatisfies(cached, item) ? cached.signature : null;
          return { effectId: item.effectId, signature, displayColumnCount: item.displayColumnCount };
        }),
        displayRowCount
      });
      if (clipRasterRequestCancelled(abortController.signal)) return false;
      return await pollResults(response.requestId, response.complete, requestContexts);
    };

    requestTimeout = window.setTimeout(() => void requestRasters(visibleRequestRasterItems()), CLIP_RASTER_REQUEST_THROTTLE_MS);
    return () => {
      abortController.abort();
      if (pollTimeout !== null) window.clearTimeout(pollTimeout);
      window.clearTimeout(requestTimeout);
      if (decodeFrame !== null) window.cancelAnimationFrame(decodeFrame);
    };
  }, [document.objectKey, document.path, effectIds, effectIdsKey, laneHeight, projectRevision, rasterRequestKey, rasterSettings, rasterSettingsKey, visibleRequestItemsKey]);

  return state.requestKey === rasterRequestKey ? state : {
    requestKey: rasterRequestKey,
    projectRevision,
    rasters: new Map(),
    expectedRasterKeys: new Map(),
    errors: new Set()
  };
}

function clipRasterKey(path: string, objectKey: string | null, effectId: number, signature: string, context: ClipRasterKeyContext): string {
  return JSON.stringify([path, objectKey, context.rasterSettingsKey, effectId, context.requestedColumns, context.requestedRows, signature]);
}

function clipRasterRequestCancelled(signal: AbortSignal): boolean {
  return signal.aborted;
}

function decodedClipRasterSatisfies(raster: DecodedClipRaster, item: ClipRasterRequestItem): boolean {
  return raster.columns >= item.requestedColumns && raster.requestRows >= item.requestedRows;
}

function evictDecodedClipRasters(rasters: Map<string, DecodedClipRaster>, protectedRasterKeys: Set<string>) {
  let byteLength = 0;
  for (const raster of rasters.values()) byteLength += raster.byteLength;
  while (byteLength > CLIP_RASTER_DECODED_BYTE_BUDGET) {
    let evictRasterKey: string | null = null;
    let oldest = Number.POSITIVE_INFINITY;
    for (const [rasterKey, raster] of rasters) {
      if (protectedRasterKeys.has(rasterKey)) continue;
      if (raster.lastUsed < oldest) {
        oldest = raster.lastUsed;
        evictRasterKey = rasterKey;
      }
    }
    if (evictRasterKey === null) return;
    const raster = rasters.get(evictRasterKey);
    if (raster === undefined) return;
    byteLength -= raster.byteLength;
    rasters.delete(evictRasterKey);
  }
}

export function drawClipRaster(ctx: CanvasRenderingContext2D, raster: DecodedClipRaster, rect: { x: number; y: number; width: number; height: number }) {
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(raster.image, rect.x, rect.y, rect.width, rect.height);
  ctx.imageSmoothingEnabled = true;
}

async function decodeClipRaster(payload: SequenceClipRaster): Promise<CanvasImageSource> {
  const raster = window.document.createElement("canvas");
  raster.width = payload.columns;
  raster.height = payload.rows;
  const rasterContext = raster.getContext("2d");
  if (rasterContext === null) throw new Error("Raster canvas context is unavailable.");
  const response = await fetch(convertFileSrc(payload.pixelsRgbaToken, "donder-raster"));
  if (!response.ok) throw new Error(`Raster pixel fetch failed with ${response.status}.`);
  const pixels = new Uint8ClampedArray(await response.arrayBuffer());
  const image = new ImageData(pixels, payload.columns, payload.rows);
  rasterContext.putImageData(image, 0, 0);
  return raster;
}

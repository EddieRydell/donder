import { useSequenceEditorHost, type SequenceEditorHost } from "../../../editor/host";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { useEffect, useMemo, useState } from "react";

// Runtime evaluates effects, the desktop worker renders every clip's raster
// once at its natural resolution, and this hook fetches each raster when its
// revision changes and scales it into the clip's rectangle.

import type { SequenceClipRaster, SequenceEditorDocument } from "../../../editor/types";
import type { SequenceClipLayout } from "./sequenceSelection";

type DecodedClipRaster = {
  revision: number;
  image: CanvasImageSource;
  byteLength: number;
  lastUsed: number;
};

export type ClipRasterState = {
  // Changes whenever a raster is decoded, failed or dropped.
  version: number;
  raster: (effectId: number) => DecodedClipRaster | undefined;
  failed: (effectId: number) => boolean;
};

const CLIP_RASTER_REQUEST_THROTTLE_MS = 50;
const CLIP_RASTER_POLL_MS = 100;
// Decoding stops for the frame once it has taken this long.
const CLIP_RASTER_DECODE_FRAME_BUDGET_MS = 8;
const CLIP_RASTER_DECODED_BYTE_BUDGET = 256 * 1024 * 1024;

export function useSequenceClipRasters(document: SequenceEditorDocument, visibleClips: SequenceClipLayout[]): ClipRasterState {
  const host = useSequenceEditorHost();
  const { commands, store: useAppStore } = host;
  const projectRevision = useAppStore((store) => store.snapshot?.projectRevision ?? null);
  const documentKey = guiObjectKey(document.sourceRef);
  const visibleKey = visibleClips.map((clip) => clip.effect.id).join(",");
  const [version, setVersion] = useState(0);
  // Rasters live outside React: worker callbacks fill the cache and `version`
  // tells the canvas to redraw.
  const { ownedPath } = document.sourceRef;
  const { path, objectKey, effects } = document;

  // Rasters of clips the sequence no longer has are dropped; the canvas
  // never asks for them, so nothing needs to redraw.
  useEffect(() => {
    const cache = rasterCache(documentKey);
    const ids = new Set(effects.map((effect) => effect.id));
    for (const id of [...cache.known.keys()]) {
      if (ids.has(id)) continue;
      cache.known.delete(id);
      cache.failed.delete(id);
      forgetDecoded(cache, id);
    }
  }, [documentKey, effects]);

  // Tell the worker which clips are on screen, and fetch what it renders.
  useEffect(() => {
    if (projectRevision === null) return;
    const cache = rasterCache(documentKey);
    cache.visible = visibleKey === "" ? [] : visibleKey.split(",").map(Number);
    const request = { ownedPath, projectRevision, path, view: "sequence" as const, objectKey };
    let cancelled = false;
    let pollTimeout: number | null = null;
    const publish = () => {
      if (!cancelled) setVersion((value) => value + 1);
    };
    const poll = async () => {
      pollTimeout = null;
      const batch = await commands.takeSequenceClipRasterResults(request, cache.since);
      if (cancelled) return;
      cache.since = batch.revision;
      let changed = false;
      for (const raster of batch.rasters) {
        cache.known.set(raster.effectId, raster);
        changed = cache.failed.delete(raster.effectId) || changed;
      }
      for (const error of batch.errors) {
        cache.known.delete(error.effectId);
        cache.failed.add(error.effectId);
        changed = forgetDecoded(cache, error.effectId) || changed;
      }
      if (changed) publish();
      scheduleDecode(host, cache, publish);
      if (batch.pending > 0) {
        pollTimeout = window.setTimeout(() => {
          void poll();
        }, CLIP_RASTER_POLL_MS);
      }
    };
    const requestTimeout = window.setTimeout(() => {
      void commands.requestSequenceClipRasters({ ...request, visibleEffectIds: cache.visible }).then(() => {
        if (!cancelled) void poll();
      });
    }, CLIP_RASTER_REQUEST_THROTTLE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(requestTimeout);
      if (pollTimeout !== null) window.clearTimeout(pollTimeout);
    };
  }, [commands, documentKey, host, objectKey, ownedPath, path, projectRevision, visibleKey]);

  return useMemo(() => ({
    version,
    raster: (effectId: number) => usedRaster(documentKey, effectId),
    failed: (effectId: number) => rasterCache(documentKey).failed.has(effectId)
  }), [documentKey, version]);
}

// The open sequence's rasters; opening another sequence starts empty.
let openRasters: RasterCache | null = null;

function rasterCache(documentKey: string): RasterCache {
  if (openRasters?.documentKey !== documentKey) openRasters = newRasterCache(documentKey);
  return openRasters;
}

function usedRaster(documentKey: string, effectId: number): DecodedClipRaster | undefined {
  const cache = rasterCache(documentKey);
  const decoded = cache.decoded.get(effectId);
  if (decoded !== undefined) decoded.lastUsed = cache.access++;
  return decoded;
}

type RasterCache = {
  documentKey: string;
  // The newest raster revision fetched.
  since: number;
  // The latest raster of each clip, decoded or not.
  known: Map<number, SequenceClipRaster>;
  decoded: Map<number, DecodedClipRaster>;
  failed: Set<number>;
  decodedBytes: number;
  access: number;
  decoding: boolean;
  /** The clips on screen, which decode first. */
  visible: number[];
};

function newRasterCache(documentKey: string): RasterCache {
  return { documentKey, since: 0, known: new Map(), decoded: new Map(), failed: new Set(), decodedBytes: 0, access: 0, decoding: false, visible: [] };
}

function forgetDecoded(cache: RasterCache, effectId: number): boolean {
  const decoded = cache.decoded.get(effectId);
  if (decoded === undefined) return false;
  cache.decoded.delete(effectId);
  cache.decodedBytes -= decoded.byteLength;
  return true;
}

// The known rasters not yet decoded at their latest revision, visible clips first.
function decodeQueue(cache: RasterCache, visible: number[]): SequenceClipRaster[] {
  const stale = (raster: SequenceClipRaster | undefined) => raster !== undefined && cache.decoded.get(raster.effectId)?.revision !== raster.revision;
  const queue = visible.map((id) => cache.known.get(id)).filter(stale) as SequenceClipRaster[];
  const queued = new Set(queue.map((raster) => raster.effectId));
  // Off-screen clips decode while the budget has room, so scrolling shows them at once.
  if (cache.decodedBytes < CLIP_RASTER_DECODED_BYTE_BUDGET) {
    for (const raster of cache.known.values()) {
      if (!queued.has(raster.effectId) && stale(raster)) queue.push(raster);
    }
  }
  return queue;
}

function scheduleDecode(host: SequenceEditorHost, cache: RasterCache, published: () => void) {
  if (cache.decoding) return;
  cache.decoding = true;
  const decodeFrame = async () => {
    const started = performance.now();
    const queue = decodeQueue(cache, cache.visible);
    let decodedAny = false;
    for (const raster of queue) {
      if (performance.now() - started > CLIP_RASTER_DECODE_FRAME_BUDGET_MS) break;
      try {
        const image = await decodeClipRaster(host, raster);
        if (cache.known.get(raster.effectId)?.revision !== raster.revision) continue;
        forgetDecoded(cache, raster.effectId);
        const byteLength = raster.columns * raster.rows * 4;
        cache.decoded.set(raster.effectId, { revision: raster.revision, image, byteLength, lastUsed: cache.access++ });
        cache.decodedBytes += byteLength;
        decodedAny = true;
      } catch {
        cache.known.delete(raster.effectId);
        cache.failed.add(raster.effectId);
        decodedAny = true;
      }
    }
    evictDecoded(cache, new Set(cache.visible));
    if (decodedAny) published();
    if (decodeQueue(cache, cache.visible).length === 0) {
      cache.decoding = false;
      return;
    }
    window.requestAnimationFrame(() => void decodeFrame());
  };
  window.requestAnimationFrame(() => void decodeFrame());
}

// Drop the decoded rasters used longest ago, keeping visible clips; they
// decode again from the backend when they come back into view.
function evictDecoded(cache: RasterCache, visible: Set<number>) {
  if (cache.decodedBytes <= CLIP_RASTER_DECODED_BYTE_BUDGET) return;
  const candidates = [...cache.decoded].filter(([id]) => !visible.has(id)).sort((a, b) => a[1].lastUsed - b[1].lastUsed);
  for (const [id] of candidates) {
    if (cache.decodedBytes <= CLIP_RASTER_DECODED_BYTE_BUDGET) return;
    forgetDecoded(cache, id);
  }
}

export function drawClipRaster(ctx: CanvasRenderingContext2D, raster: DecodedClipRaster, rect: { x: number; y: number; width: number; height: number }) {
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(raster.image, rect.x, rect.y, rect.width, rect.height);
  ctx.imageSmoothingEnabled = true;
}

async function decodeClipRaster(host: SequenceEditorHost, payload: SequenceClipRaster): Promise<CanvasImageSource> {
  const response = await fetch(host.resolveAssetUrl(payload.pixelsRgbaToken, "donder-raster"));
  if (!response.ok) throw new Error(`Raster pixel fetch failed with ${response.status}.`);
  const pixels = new Uint8ClampedArray(await response.arrayBuffer());
  return await createImageBitmap(new ImageData(pixels, payload.columns, payload.rows));
}

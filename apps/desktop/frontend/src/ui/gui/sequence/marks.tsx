import { useEffect, useState } from "react";

import type { SequenceMarkCollection } from "../../../types";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";

import type { GuiFocus } from "../shared";

import { getMarkDraft, markDraftEntries, setMarkDraft, type MarkDraftLookup, type MarkRefLookup } from "./sequenceSelection";

export type MarkDisplayMode = "overlay" | "strip" | "hidden";

const DEFAULT_MARK_COLORS = [THEME_COLORS.markBlue, THEME_COLORS.markOrange, THEME_COLORS.markGreen, THEME_COLORS.markPink, THEME_COLORS.markYellow, THEME_COLORS.markRed];

const MARK_DRAWING = {
  cullPaddingPx: THEME_METRICS.markCullPadding,
  overlayAlpha: THEME_METRICS.markOverlayOpacity,
  stripAlpha: THEME_METRICS.markStripOpacity,
  selectedCapHalfWidthPx: THEME_METRICS.markSelectedHalfWidth,
  selectedStroke: THEME_COLORS.textStrong
} as const;

let markDisplayMode: MarkDisplayMode = "overlay";

export function setGlobalMarkDisplayMode(nextMode: MarkDisplayMode) {
  markDisplayMode = nextMode;
  window.dispatchEvent(new CustomEvent<MarkDisplayMode>("donder-mark-display-mode", { detail: nextMode }));
}

export function useMarkDisplayMode() {
  const [mode, setMode] = useState<MarkDisplayMode>(markDisplayMode);

  useEffect(() => {
    const listener = (event: Event) => {
      setMode((event as CustomEvent<MarkDisplayMode>).detail);
    };
    window.addEventListener("donder-mark-display-mode", listener);
    return () => {
      window.removeEventListener("donder-mark-display-mode", listener);
    };
  }, []);

  return [mode, setMode] as const;
}

export function drawSequenceMarks(
  ctx: CanvasRenderingContext2D,
  collections: SequenceMarkCollection[],
  selected: GuiFocus,
  selectedMarks: MarkRefLookup,
  mode: MarkDisplayMode,
  left: number,
  audioStripTop: number,
  audioStripHeight: number,
  width: number,
  height: number,
  pxPerSecond: number,
  scrollXSeconds: number,
  drafts: MarkDraftLookup
) {
  if (mode === "hidden") return;
  const y1 = audioStripTop;
  const y2 = mode === "strip" ? audioStripTop + audioStripHeight : height;
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, y1, width, y2 - y1);
  ctx.clip();
  for (const collection of collections) {
    for (const [index, timeSeconds] of collection.marksSeconds.entries()) {
      const mark = { collectionKey: collection.key, index };
      const draft = getMarkDraft(drafts, mark);
      const drawnTimeSeconds = draft?.timeSeconds ?? timeSeconds;
      const x = left + (drawnTimeSeconds - scrollXSeconds) * pxPerSecond;
      if (x < left - MARK_DRAWING.cullPaddingPx || x > left + width + MARK_DRAWING.cullPaddingPx) continue;
      const isSelected =
        (selected?.type === "mark" && selected.collectionKey === collection.key && selected.index === index) ||
        (selectedMarks.get(collection.key)?.has(index) ?? false);
      ctx.strokeStyle = collection.color;
      ctx.lineWidth = isSelected ? THEME_METRICS.visualLineWidthStrong : THEME_METRICS.visualLineWidth;
      ctx.globalAlpha = mode === "strip" ? MARK_DRAWING.stripAlpha : MARK_DRAWING.overlayAlpha;
      ctx.beginPath();
      ctx.moveTo(x + THEME_METRICS.visualHairlineOffset, y1);
      ctx.lineTo(x + THEME_METRICS.visualHairlineOffset, y2);
      ctx.stroke();
      if (isSelected) {
        ctx.globalAlpha = THEME_METRICS.opacityFull;
        ctx.strokeStyle = MARK_DRAWING.selectedStroke;
        ctx.lineWidth = THEME_METRICS.visualLineWidth;
        ctx.beginPath();
        ctx.moveTo(x - MARK_DRAWING.selectedCapHalfWidthPx, y1 + THEME_METRICS.visualHairlineOffset);
        ctx.lineTo(x + MARK_DRAWING.selectedCapHalfWidthPx, y1 + THEME_METRICS.visualHairlineOffset);
        ctx.stroke();
      }
    }
  }
  ctx.restore();
}

export function committedMarkDrafts(collections: SequenceMarkCollection[], drafts: MarkDraftLookup) {
  const next: MarkDraftLookup = new Map();
  for (const draft of markDraftEntries(drafts)) {
    if (draft.committedIndex === undefined) {
      setMarkDraft(next, draft, draft);
      continue;
    }
    const collection = collections.find((candidate) => candidate.key === draft.collectionKey);
    if (collection?.marksSeconds[draft.committedIndex] !== draft.timeSeconds) {
      setMarkDraft(next, draft, draft);
    }
  }
  return next;
}

export function markIndexAfterMove(collection: SequenceMarkCollection, index: number, timeSeconds: number) {
  const sorted = collection.marksSeconds
    .map((markTimeSeconds, markIndex) => ({
      markIndex,
      timeSeconds: markIndex === index ? timeSeconds : markTimeSeconds
    }))
    .sort((left, right) => left.timeSeconds - right.timeSeconds || left.markIndex - right.markIndex);
  return Math.max(0, sorted.findIndex((mark) => mark.markIndex === index));
}

export function nextCollectionKey(name: string, collections: SequenceMarkCollection[]) {
  const used = new Set(collections.map((collection) => collection.key));
  const base = snakeCaseKey(name);
  if (!used.has(base)) return base;
  for (let suffix = 2; ; suffix += 1) {
    const key = `${base}_${suffix}`;
    if (!used.has(key)) return key;
  }
}

export function defaultMarkColor(index: number) {
  return DEFAULT_MARK_COLORS[index % DEFAULT_MARK_COLORS.length] ?? THEME_COLORS.markBlue;
}

function snakeCaseKey(value: string) {
  const key = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_]+/g, "_")
    .replace(/_+/g, "_")
    .replace(/^_+|_+$/g, "");
  return /^[a-z]/.test(key) ? key : key.length > 0 ? `marks_${key}` : "marks";
}

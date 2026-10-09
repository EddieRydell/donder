import type { SequenceMarkCollection } from "../../../editor/types";
import { THEME_COLORS, THEME_METRICS, THEME_TYPOGRAPHY } from "../../../theme";

import type { GuiFocus } from "../shared";

import { fitCanvasLabel } from "./sequenceAutomationLayout";
import { getMarkDraft, markDraftEntries, setMarkDraft, type MarkDraftLookup, type MarkRefLookup } from "./sequenceSelection";

const DEFAULT_MARK_COLORS = [THEME_COLORS.markBlue, THEME_COLORS.markOrange, THEME_COLORS.markGreen, THEME_COLORS.markPink, THEME_COLORS.markYellow, THEME_COLORS.markRed];

const MARK_DRAWING = {
  cullPaddingPx: THEME_METRICS.markCullPadding,
  lineAlpha: THEME_METRICS.markOverlayOpacity,
  selectedCapHalfWidthPx: THEME_METRICS.markSelectedHalfWidth,
  selectedStroke: THEME_COLORS.textStrong
} as const;

/** The collection that new marks go into: the active one, else the first. */
export function activeMarkCollection(collections: SequenceMarkCollection[], activeKey: string | null) {
  return collections.find((collection) => collection.key === activeKey) ?? collections[0] ?? null;
}

/**
 * The Marks lane holds each mark's handle, the only place a mark can be grabbed; the active
 * collection draws on top. Unless `laneOnly`, marks also draw as guide lines from the audio strip
 * down through the lanes.
 */
export function drawSequenceMarks(
  ctx: CanvasRenderingContext2D,
  collections: SequenceMarkCollection[],
  activeCollectionKey: string | null,
  selected: GuiFocus,
  selectedMarks: MarkRefLookup,
  laneOnly: boolean,
  left: number,
  linesTop: number,
  rulerTop: number,
  rulerHeight: number,
  width: number,
  height: number,
  pxPerSecond: number,
  scrollXSeconds: number,
  drafts: MarkDraftLookup
) {
  const ordered = [
    ...collections.filter((collection) => collection.key !== activeCollectionKey),
    ...collections.filter((collection) => collection.key === activeCollectionKey)
  ];
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, linesTop, width, height - linesTop);
  ctx.clip();
  for (const collection of ordered) {
    const active = collection.key === activeCollectionKey;
    for (const [index, timeSeconds] of collection.marksSeconds.entries()) {
      const mark = { collectionKey: collection.key, index };
      const draft = getMarkDraft(drafts, mark);
      const drawnTimeSeconds = draft?.timeSeconds ?? timeSeconds;
      const x = left + (drawnTimeSeconds - scrollXSeconds) * pxPerSecond;
      if (x < left - MARK_DRAWING.cullPaddingPx || x > left + width + MARK_DRAWING.cullPaddingPx) continue;
      const isSelected =
        (selected?.type === "mark" && selected.collectionKey === collection.key && selected.index === index) ||
        (selectedMarks.get(collection.key)?.has(index) ?? false);
      const lineX = x + THEME_METRICS.visualHairlineOffset;
      ctx.strokeStyle = collection.color;
      if (!laneOnly) {
        ctx.lineWidth = isSelected ? THEME_METRICS.visualLineWidthStrong : THEME_METRICS.visualLineWidth;
        ctx.globalAlpha = MARK_DRAWING.lineAlpha;
        ctx.beginPath();
        ctx.moveTo(lineX, linesTop);
        ctx.lineTo(lineX, height);
        ctx.stroke();
      }
      ctx.globalAlpha = active ? THEME_METRICS.opacityFull : MARK_DRAWING.lineAlpha;
      ctx.lineWidth = THEME_METRICS.visualLineWidthStrong;
      ctx.beginPath();
      ctx.moveTo(lineX, rulerTop);
      ctx.lineTo(lineX, rulerTop + rulerHeight);
      ctx.stroke();
      if (isSelected) {
        ctx.globalAlpha = THEME_METRICS.opacityFull;
        ctx.strokeStyle = MARK_DRAWING.selectedStroke;
        ctx.lineWidth = THEME_METRICS.visualLineWidth;
        ctx.strokeRect(
          x - MARK_DRAWING.selectedCapHalfWidthPx + THEME_METRICS.visualHairlineOffset,
          rulerTop + THEME_METRICS.visualHairlineOffset,
          MARK_DRAWING.selectedCapHalfWidthPx * 2,
          rulerHeight - THEME_METRICS.visualLineWidth
        );
      }
    }
  }
  ctx.restore();
}

export function drawMarkRulerLabel(ctx: CanvasRenderingContext2D, rulerTop: number, rulerHeight: number, left: number) {
  const x = THEME_METRICS.sequenceLabelX;
  ctx.save();
  ctx.font = THEME_TYPOGRAPHY.sequenceHeading;
  ctx.fillStyle = THEME_COLORS.textSoft;
  ctx.fillText(fitCanvasLabel(ctx, "Marks", left - x * 2), x, rulerTop + rulerHeight / 2 + THEME_METRICS.sequenceLabelYOffset);
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

export function nextCollectionKey(name: string, collections: Pick<SequenceMarkCollection, "key">[]) {
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

/**
 * Times that divide each gap between consecutive selected times into `divisions` equal parts.
 * The selected times themselves are not included.
 */
export function subdivisionTimes(selectedSeconds: number[], divisions: number) {
  const times = [...new Set(selectedSeconds)].sort((left, right) => left - right);
  const subdivided: number[] = [];
  for (let index = 1; index < times.length; index += 1) {
    const start = times[index - 1];
    const end = times[index];
    if (start === undefined || end === undefined) continue;
    for (let step = 1; step < divisions; step += 1) {
      subdivided.push(start + ((end - start) * step) / divisions);
    }
  }
  return subdivided;
}

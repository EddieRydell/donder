import type { FixtureTarget, SequenceEditorDocument, SequenceEffect, SequenceMarkCollection, SequenceMarkRef, SequenceSelection } from "../../../types";

import { clamp, type GuiFocus } from "../shared";

import { markIndexAfterMove, type MarkDisplayMode } from "./marks";

import { targetsEqual } from "./sequenceTargets";
import { THEME_METRICS } from "../../../theme";
import type { SequenceRowLayout } from "./sequenceAutomationLayout";

export type SequenceDraft = { id: number; startSeconds: number; durationSeconds: number; laneIndex: number };

export type MarkDraft = { collectionKey: string; index: number; timeSeconds: number; committedIndex?: number };

export type MarkDraftLookup = Map<string, Map<number, MarkDraft>>;

export type MarkRefLookup = Map<string, Set<number>>;

export type SequenceContextMenu =
  | { kind: "blank"; laneIndex: number; startSeconds: number }
  | { kind: "effect"; laneIndex: number; startSeconds: number; effectId: number }
  | { kind: "automation"; laneIndex: number; startSeconds: number; clipId: number }
  | { kind: "mark"; laneIndex: number; startSeconds: number; collectionKey: string; index: number };

export type SequenceHover =
  | null
  | { kind: "effect"; effectId: number; resize: "left" | "right" | "none" }
  | { kind: "mark"; collectionKey: string; index: number };

export type SequenceMarquee = { mode: "effects" | "marks"; startX: number; startY: number; x: number; y: number; active: boolean; shift: boolean; ctrl: boolean };

export const MIN_EFFECT_DURATION_SECONDS = 0.000000001;

const SEQUENCE_HIT_RADII = {
  effectResizeHandlePx: THEME_METRICS.sequenceEffectResizeHitWidth,
  markPx: THEME_METRICS.sequenceMarkHitRadius
} as const;

export type SequenceViewport = {
  pxPerSecond: number;
  rowHeights: number[][];
  scrollXSeconds: number;
  scrollY: number;
};

export type SequenceClipLayout = {
  effect: SequenceEffect;
  laneIndex: number;
  rect: { x: number; y: number; width: number; height: number };
};

export type SequenceClipLayoutBounds = {
  width: number;
  height: number;
};

type SequenceClip = {
  effect: SequenceEffect;
  laneIndex: number;
};

type SequenceClipWithSlot = SequenceClip & { slot: number };

export type SequenceHit = {
  effect: SequenceEffect;
  laneIndex: number;
  resize: "left" | "right" | "none";
};

export type SequenceMarkHit = {
  collectionKey: string;
  index: number;
  timeSeconds: number;
};

export function buildSequenceClipLayout(
  document: SequenceEditorDocument,
  drafts: SequenceDraft[],
  viewport: SequenceViewport,
  left: number,
  top: number,
  bounds: SequenceClipLayoutBounds,
  rows: SequenceRowLayout[]
): SequenceClipLayout[] {
  const laneIndexByTarget = new Map(document.lanes.map((lane, index) => [lane.target.fixture, index]));
  const draftById = new Map(drafts.map((draft) => [draft.id, draft]));
  const visibleStartSeconds = viewport.scrollXSeconds;
  const visibleEndSeconds = viewport.scrollXSeconds + Math.max(1, bounds.width - left) / viewport.pxPerSecond;
  const clips: SequenceClip[] = [];
  for (const effect of document.effects) {
    const activeDraft = draftById.get(effect.id) ?? null;
    const clip: SequenceClip = activeDraft === null
      ? {
          effect,
          laneIndex: laneIndexForTarget(laneIndexByTarget, effect.target)
        }
      : clipFromDraft(document, effect, activeDraft);
    if (!effectIntersectsTimeRange(clip.effect, visibleStartSeconds, visibleEndSeconds)) continue;
    clips.push(clip);
  }

  const byLane = new Map<number, SequenceClip[]>();
  for (const clip of clips) {
    if (clip.laneIndex < 0) continue;
    const laneClips = byLane.get(clip.laneIndex) ?? [];
    laneClips.push(clip);
    byLane.set(clip.laneIndex, laneClips);
  }

  const layouts: SequenceClipLayout[] = [];
  for (const [laneIndex, laneClips] of byLane) {
    const row = rows.find((row) => row.laneIndex === laneIndex && row.rowIndex === 0);
    if (row === undefined) throw new Error("Effect clip has no timeline row.");
    const groups = groupOverlappingClips(laneClips);
    for (const group of groups) {
      const assigned = assignOverlapSlots(group);
      const slotCount = Math.max(1, Math.max(...assigned.map((clip) => clip.slot)) + 1);
      const laneHeight = row.height;
      const slotHeight = laneHeight / slotCount;
      for (const clip of assigned) {
        const startSeconds = clip.effect.startSeconds;
        const endSeconds = startSeconds + clip.effect.durationSeconds;
        const x = left + (startSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
        const width = Math.max(THEME_METRICS.sequenceClipMinWidth, (endSeconds - startSeconds) * viewport.pxPerSecond);
        layouts.push({
          effect: clip.effect,
          laneIndex,
          rect: {
            x,
            y: top + row.top - viewport.scrollY + clip.slot * slotHeight + THEME_METRICS.sequenceClipSlotOffset,
            width,
            height: Math.max(THEME_METRICS.sequenceClipMinHeight, slotHeight - THEME_METRICS.sequenceClipHandleInset)
          }
        });
      }
    }
  }
  return layouts;
}

function clipFromDraft(
  document: SequenceEditorDocument,
  effect: SequenceEffect,
  draft: SequenceDraft
): SequenceClip {
  const draftLane = document.lanes[draft.laneIndex];
  return {
    effect: {
      ...effect,
      startSeconds: draft.startSeconds,
      durationSeconds: draft.durationSeconds,
      target: draftLane?.target ?? effect.target,
      targetLabel: draftLane?.label ?? effect.targetLabel
    },
    laneIndex: draft.laneIndex
  };
}

function laneIndexForTarget(lanes: Map<number, number>, target: FixtureTarget): number {
  return lanes.get(target.fixture) ?? 0;
}

function effectIntersectsTimeRange(effect: SequenceEffect, startSeconds: number, endSeconds: number): boolean {
  const effectStart = effect.startSeconds;
  const effectEnd = effect.startSeconds + effect.durationSeconds;
  return effectEnd >= startSeconds && effectStart <= endSeconds;
}

function groupOverlappingClips(clips: SequenceClip[]) {
  const sorted = [...clips].sort(compareClipsByTime);
  const groups: SequenceClip[][] = [];
  let current: SequenceClip[] = [];
  let currentEnd = -Infinity;
  for (const clip of sorted) {
    const start = clip.effect.startSeconds;
    const end = clip.effect.startSeconds + clip.effect.durationSeconds;
    if (current.length === 0 || start < currentEnd) {
      current.push(clip);
      currentEnd = Math.max(currentEnd, end);
      continue;
    }
    groups.push(current);
    current = [clip];
    currentEnd = end;
  }
  if (current.length > 0) groups.push(current);
  return groups;
}

function assignOverlapSlots(group: SequenceClip[]): SequenceClipWithSlot[] {
  const sorted = [...group].sort(compareClipsByTime);
  const slotEnds: number[] = [];
  return sorted.map((clip) => {
    const start = clip.effect.startSeconds;
    const end = clip.effect.startSeconds + clip.effect.durationSeconds;
    let slot = slotEnds.findIndex((slotEnd) => slotEnd <= start);
    if (slot === -1) slot = slotEnds.length;
    slotEnds[slot] = end;
    return { ...clip, slot };
  });
}

function compareClipsByTime(left: { effect: SequenceEffect }, right: { effect: SequenceEffect }) {
  return (
    left.effect.startSeconds - right.effect.startSeconds ||
    left.effect.startSeconds + left.effect.durationSeconds - (right.effect.startSeconds + right.effect.durationSeconds) ||
    left.effect.id - right.effect.id
  );
}

export function hitSequence(clips: SequenceClipLayout[], x: number, y: number): SequenceHit | null {
  for (const clip of [...clips].reverse()) {
    const { rect } = clip;
    if (x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height) {
      const resize: "left" | "right" | "none" =
        x - rect.x < SEQUENCE_HIT_RADII.effectResizeHandlePx ? "left" : rect.x + rect.width - x < SEQUENCE_HIT_RADII.effectResizeHandlePx ? "right" : "none";
      return {
        effect: clip.effect,
        laneIndex: clip.laneIndex,
        resize
      };
    }
  }
  return null;
}

export function hitSequenceMark(
  collections: SequenceMarkCollection[],
  mode: MarkDisplayMode,
  x: number,
  y: number,
  left: number,
  audioStripTop: number,
  audioStripHeight: number,
  canvasHeight: number,
  viewport: SequenceViewport
): SequenceMarkHit | null {
  if (mode === "hidden" || x < left) return null;
  if (mode === "strip" && (y < audioStripTop || y > audioStripTop + audioStripHeight)) return null;
  if (mode === "overlay" && (y < audioStripTop || y > canvasHeight)) return null;
  for (const collection of [...collections].reverse()) {
    for (let index = collection.marksSeconds.length - 1; index >= 0; index -= 1) {
      const timeSeconds = collection.marksSeconds[index] ?? 0;
      const markX = left + (timeSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
      if (Math.abs(x - markX) <= SEQUENCE_HIT_RADII.markPx) {
        return { collectionKey: collection.key, index, timeSeconds };
      }
    }
  }
  return null;
}

export function sequenceHoverEqual(left: SequenceHover, right: SequenceHover) {
  if (left === right) return true;
  if (left === null || right === null || left.kind !== right.kind) return false;
  if (left.kind === "effect" && right.kind === "effect") {
    return left.effectId === right.effectId && left.resize === right.resize;
  }
  if (left.kind !== "mark" || right.kind !== "mark") return false;
  return left.collectionKey === right.collectionKey && left.index === right.index;
}

export function selectedEffectId(selected: GuiFocus): number | null {
  return selected?.type === "effect" ? selected.id : null;
}

export function selectionFromSingle(selected: GuiFocus): SequenceSelection | null {
  const effectId = selectedEffectId(selected);
  if (effectId !== null) return { type: "effects", ids: [effectId] };
  if (selected?.type === "mark") return { type: "marks", marks: [{ collectionKey: selected.collectionKey, index: selected.index }] };
  return null;
}

export function singleSelectionFocus(selection: SequenceSelection | null): GuiFocus {
  if (selection?.type === "effects") return singleEffectSelectionFocus(selection.ids);
  if (selection?.type === "marks" && selection.marks.length === 1) {
    const mark = selection.marks[0];
    return mark === undefined ? null : { type: "mark", collectionKey: mark.collectionKey, index: mark.index };
  }
  return null;
}

export function singleEffectSelectionFocus(ids: number[]): GuiFocus {
  if (ids.length !== 1) return null;
  const id = ids[0];
  return id === undefined ? null : { type: "effect", id };
}

export function selectionCount(selection: SequenceSelection) {
  return selection.type === "effects" ? selection.ids.length : selection.marks.length;
}

export function selectionCompatibleWithFocusedItem(selection: SequenceSelection, selected: GuiFocus) {
  const effectId = selectedEffectId(selected);
  if (effectId !== null) return selection.type === "effects" && selection.ids.includes(effectId);
  if (selected?.type === "mark") {
    const mark = { collectionKey: selected.collectionKey, index: selected.index };
    return selection.type === "marks" && markLookupHas(markRefLookup(selection.marks), mark);
  }
  return true;
}

export function nextEffectSelection(current: SequenceSelection | null, id: number, shift: boolean, ctrl: boolean): SequenceSelection {
  if (current?.type !== "effects" || (!shift && !ctrl)) return { type: "effects", ids: [id] };
  const ids = new Set(current.ids);
  if (ctrl && ids.has(id)) ids.delete(id);
  else ids.add(id);
  return { type: "effects", ids: [...ids] };
}

export function nextMarkSelection(current: SequenceSelection | null, mark: SequenceMarkRef, shift: boolean, ctrl: boolean): SequenceSelection {
  if (current?.type !== "marks" || (!shift && !ctrl)) return { type: "marks", marks: [mark] };
  const byCollection = markRefLookup(current.marks);
  if (ctrl && markLookupHas(byCollection, mark)) removeMarkRef(byCollection, mark);
  else addMarkRef(byCollection, mark);
  return { type: "marks", marks: markRefsFromLookup(byCollection) };
}

export function mergeSequenceSelection(current: SequenceSelection | null, next: SequenceSelection, shift: boolean, ctrl: boolean): SequenceSelection {
  if ((!shift && !ctrl) || current?.type !== next.type) return next;
  if (next.type === "effects") {
    const ids = new Set(current.type === "effects" ? current.ids : []);
    for (const id of next.ids) {
      if (ctrl && ids.has(id)) ids.delete(id);
      else ids.add(id);
    }
    return { type: "effects", ids: [...ids] };
  }
  const marks = markRefLookup(current.type === "marks" ? current.marks : []);
  for (const mark of next.marks) {
    if (ctrl && markLookupHas(marks, mark)) removeMarkRef(marks, mark);
    else addMarkRef(marks, mark);
  }
  return { type: "marks", marks: markRefsFromLookup(marks) };
}

export function normalizedRect(startX: number, startY: number, x: number, y: number) {
  const left = Math.min(startX, x);
  const top = Math.min(startY, y);
  return { x: left, y: top, width: Math.abs(x - startX), height: Math.abs(y - startY) };
}

function rectsIntersect(left: { x: number; y: number; width: number; height: number }, right: { x: number; y: number; width: number; height: number }) {
  return left.x <= right.x + right.width && left.x + left.width >= right.x && left.y <= right.y + right.height && left.y + left.height >= right.y;
}

export function selectionFromMarqueeEffects(clips: SequenceClipLayout[], marquee: SequenceMarquee): SequenceSelection {
  const box = normalizedRect(marquee.startX, marquee.startY, marquee.x, marquee.y);
  return { type: "effects", ids: clips.filter((clip) => rectsIntersect(box, clip.rect)).map((clip) => clip.effect.id) };
}

export function selectionFromMarqueeMarks(
  collections: SequenceMarkCollection[],
  mode: MarkDisplayMode,
  marquee: SequenceMarquee,
  left: number,
  audioStripTop: number,
  audioStripHeight: number,
  canvasHeight: number,
  viewport: SequenceViewport
): SequenceSelection {
  const box = normalizedRect(marquee.startX, marquee.startY, marquee.x, marquee.y);
  const y1 = mode === "strip" ? audioStripTop : audioStripTop;
  const y2 = mode === "strip" ? audioStripTop + audioStripHeight : canvasHeight;
  const marks: SequenceMarkRef[] = [];
  if (mode === "hidden") return { type: "marks", marks };
  for (const collection of collections) {
    collection.marksSeconds.forEach((timeSeconds, index) => {
      const x = left + (timeSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
      if (rectsIntersect(box, { x: x - SEQUENCE_HIT_RADII.markPx, y: y1, width: SEQUENCE_HIT_RADII.markPx * 2, height: y2 - y1 })) {
        marks.push({ collectionKey: collection.key, index });
      }
    });
  }
  return { type: "marks", marks };
}

export function constrainEffectMoveDelta(document: SequenceEditorDocument, ids: number[], deltaSeconds: number) {
  let minDelta = -Infinity;
  let maxDelta = Infinity;
  for (const effect of document.effects.filter((candidate) => ids.includes(candidate.id))) {
    minDelta = Math.max(minDelta, -effect.startSeconds);
    maxDelta = Math.min(maxDelta, document.durationSeconds - effect.durationSeconds - effect.startSeconds);
  }
  return clamp(deltaSeconds, minDelta, maxDelta);
}

export function constrainEffectLaneDelta(document: SequenceEditorDocument, ids: number[], laneDelta: number) {
  let minDelta = -Infinity;
  let maxDelta = Infinity;
  for (const effect of document.effects.filter((candidate) => ids.includes(candidate.id))) {
    const laneIndex = document.lanes.findIndex((lane) => targetsEqual(lane.target, effect.target));
    if (laneIndex < 0) continue;
    minDelta = Math.max(minDelta, -laneIndex);
    maxDelta = Math.min(maxDelta, document.lanes.length - 1 - laneIndex);
  }
  return Math.trunc(clamp(laneDelta, minDelta, maxDelta));
}

export function effectMoveDrafts(document: SequenceEditorDocument, ids: number[], deltaSeconds: number, laneDelta: number): SequenceDraft[] {
  return document.effects
    .filter((effect) => ids.includes(effect.id))
    .map((effect) => {
      const laneIndex = document.lanes.findIndex((lane) => targetsEqual(lane.target, effect.target));
      return {
        id: effect.id,
        startSeconds: clamp(effect.startSeconds + deltaSeconds, 0, Math.max(0, document.durationSeconds - effect.durationSeconds)),
        durationSeconds: effect.durationSeconds,
        laneIndex: clamp(laneIndex + laneDelta, 0, Math.max(0, document.lanes.length - 1))
      };
    });
}

export function effectResizeDrafts(document: SequenceEditorDocument, ids: number[], edge: "left" | "right", deltaSeconds: number): SequenceDraft[] {
  return document.effects
    .filter((effect) => ids.includes(effect.id))
    .map((effect) => {
      const laneIndex = Math.max(0, document.lanes.findIndex((lane) => targetsEqual(lane.target, effect.target)));
      if (edge === "left") {
        const endSeconds = effect.startSeconds + effect.durationSeconds;
        const startSeconds = clamp(effect.startSeconds + deltaSeconds, 0, endSeconds - MIN_EFFECT_DURATION_SECONDS);
        return { id: effect.id, startSeconds, durationSeconds: endSeconds - startSeconds, laneIndex };
      }
      return {
        id: effect.id,
        startSeconds: effect.startSeconds,
        durationSeconds: clamp(effect.durationSeconds + deltaSeconds, MIN_EFFECT_DURATION_SECONDS, document.durationSeconds - effect.startSeconds),
        laneIndex
      };
    });
}

export function constrainEffectResizeDelta(document: SequenceEditorDocument, ids: number[], edge: "left" | "right", deltaSeconds: number) {
  let minDelta = -Infinity;
  let maxDelta = Infinity;
  for (const effect of document.effects.filter((candidate) => ids.includes(candidate.id))) {
    if (edge === "left") {
      minDelta = Math.max(minDelta, -effect.startSeconds);
      maxDelta = Math.min(maxDelta, effect.durationSeconds - MIN_EFFECT_DURATION_SECONDS);
    } else {
      minDelta = Math.max(minDelta, MIN_EFFECT_DURATION_SECONDS - effect.durationSeconds);
      maxDelta = Math.min(maxDelta, document.durationSeconds - effect.startSeconds - effect.durationSeconds);
    }
  }
  return clamp(deltaSeconds, minDelta, maxDelta);
}

export function constrainMarkDelta(document: SequenceEditorDocument, marks: SequenceMarkRef[], deltaSeconds: number) {
  let minDelta = -Infinity;
  let maxDelta = Infinity;
  for (const mark of marks) {
    const collection = document.markCollections.find((candidate) => candidate.key === mark.collectionKey);
    const timeSeconds = collection?.marksSeconds[mark.index];
    if (timeSeconds === undefined) continue;
    minDelta = Math.max(minDelta, -timeSeconds);
    maxDelta = Math.min(maxDelta, document.durationSeconds - timeSeconds);
  }
  return clamp(deltaSeconds, minDelta, maxDelta);
}

export function markMoveDrafts(document: SequenceEditorDocument, marks: SequenceMarkRef[], deltaSeconds: number): MarkDraftLookup {
  const drafts: MarkDraftLookup = new Map();
  for (const mark of marks) {
    const collection = document.markCollections.find((candidate) => candidate.key === mark.collectionKey);
    const timeSeconds = collection?.marksSeconds[mark.index];
    if (collection === undefined || timeSeconds === undefined) continue;
    const nextTimeSeconds = clamp(timeSeconds + deltaSeconds, 0, document.durationSeconds);
    setMarkDraft(drafts, mark, {
      collectionKey: mark.collectionKey,
      index: mark.index,
      timeSeconds: nextTimeSeconds,
      committedIndex: markIndexAfterMove(collection, mark.index, nextTimeSeconds)
    });
  }
  return drafts;
}

export function markSelectionConsumesKey(selected: GuiFocus, key: string) {
  return selected?.type === "mark" && (key === "ArrowLeft" || key === "ArrowRight");
}

export function markRefLookup(marks: SequenceMarkRef[]): MarkRefLookup {
  const lookup: MarkRefLookup = new Map();
  for (const mark of marks) {
    addMarkRef(lookup, mark);
  }
  return lookup;
}

function markLookupHas(lookup: MarkRefLookup, mark: SequenceMarkRef) {
  return lookup.get(mark.collectionKey)?.has(mark.index) ?? false;
}

function addMarkRef(lookup: MarkRefLookup, mark: SequenceMarkRef) {
  const collection = lookup.get(mark.collectionKey) ?? new Set<number>();
  collection.add(mark.index);
  lookup.set(mark.collectionKey, collection);
}

function removeMarkRef(lookup: MarkRefLookup, mark: SequenceMarkRef) {
  const collection = lookup.get(mark.collectionKey);
  if (collection === undefined) return;
  collection.delete(mark.index);
  if (collection.size === 0) lookup.delete(mark.collectionKey);
}

function markRefsFromLookup(lookup: MarkRefLookup): SequenceMarkRef[] {
  const marks: SequenceMarkRef[] = [];
  for (const [collectionKey, indexes] of lookup) {
    for (const index of indexes) {
      marks.push({ collectionKey, index });
    }
  }
  return marks;
}

export function getMarkDraft(lookup: MarkDraftLookup, mark: SequenceMarkRef): MarkDraft | undefined {
  return lookup.get(mark.collectionKey)?.get(mark.index);
}

export function setMarkDraft(lookup: MarkDraftLookup, mark: SequenceMarkRef, draft: MarkDraft) {
  const collection = lookup.get(mark.collectionKey) ?? new Map<number, MarkDraft>();
  collection.set(mark.index, draft);
  lookup.set(mark.collectionKey, collection);
}

export function markDraftEntries(lookup: MarkDraftLookup): MarkDraft[] {
  return [...lookup.values()].flatMap((collection) => [...collection.values()]);
}

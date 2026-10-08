import type { SequenceEditorDocument, SequenceEffect, SequenceAutomationResize, SequenceMarkCollection, SequenceMarkRef, SequenceSelection } from "../../../editor/types";

import { clamp, type GuiFocus } from "../shared";

import { nearestLane, targetAtLane } from "./sequenceTargets";
import { THEME_METRICS } from "../../../theme";
import type { SequenceRowLayout, SequenceRowHeightMap, AutomationClipLayout, AutomationDraft } from "./sequenceAutomationLayout";

export type SequenceDraft = { id: number; startSeconds: number; durationSeconds: number; laneIndex: number };

export type MarkDraft = { collectionKey: string; index: number; timeSeconds: number; committedIndex?: number };

export type MarkDraftLookup = Map<string, Map<number, MarkDraft>>;

export type MarkRefLookup = Map<string, Set<number>>;

export type SequenceContextMenu =
  | { kind: "blank"; laneIndex: number; startSeconds: number }
  | { kind: "effect"; laneIndex: number; startSeconds: number; effectId: number }
  | { kind: "automation"; laneIndex: number; startSeconds: number; clipId: number }
  // Mark menus open from the mark ruler, which belongs to no lane.
  | { kind: "markRuler"; startSeconds: number }
  | { kind: "mark"; startSeconds: number; collectionKey: string; index: number };

export type SequenceHover =
  | null
  | { kind: "effect"; effectId: number; resize: "left" | "right" | "none" }
  | { kind: "mark"; collectionKey: string; index: number };

export type SequenceMarquee = { mode: "clips" | "marks"; startX: number; startY: number; x: number; y: number; active: boolean; shift: boolean; ctrl: boolean };

export const MIN_EFFECT_DURATION_SECONDS = 0.000000001;

const SEQUENCE_HIT_RADII = {
  effectResizeHandlePx: THEME_METRICS.sequenceEffectResizeHitWidth,
  markPx: THEME_METRICS.sequenceMarkHitRadius
} as const;

export type SequenceViewport = {
  pxPerSecond: number;
  audioStripHeight: number;
  markRulerHeight: number;
  rowHeights: SequenceRowHeightMap;
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
  const lanesByTarget = new Map<number, number[]>();
  document.lanes.forEach((lane, index) => { lanesByTarget.set(lane.target.fixture, [...lanesByTarget.get(lane.target.fixture) ?? [], index]); });
  const draftById = new Map(drafts.map((draft) => [draft.id, draft]));
  const visibleStartSeconds = viewport.scrollXSeconds;
  const visibleEndSeconds = viewport.scrollXSeconds + Math.max(1, bounds.width - left) / viewport.pxPerSecond;
  // A clip shows on every lane of its target, including a draft's destination target.
  const byLane = new Map<number, SequenceClip[]>();
  for (const original of document.effects) {
    const activeDraft = draftById.get(original.id);
    const effect = activeDraft === undefined ? original : effectFromDraft(document, original, activeDraft);
    if (!effectIntersectsTimeRange(effect, visibleStartSeconds, visibleEndSeconds)) continue;
    for (const laneIndex of lanesByTarget.get(effect.target.fixture) ?? []) {
      byLane.set(laneIndex, [...byLane.get(laneIndex) ?? [], { effect, laneIndex }]);
    }
  }

  const layouts: SequenceClipLayout[] = [];
  for (const [laneIndex, laneClips] of byLane) {
    const row = rows.find((row) => row.laneIndex === laneIndex && row.kind === "effects");
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

function effectFromDraft(
  document: SequenceEditorDocument,
  effect: SequenceEffect,
  draft: SequenceDraft
): SequenceEffect {
  const draftLane = document.lanes[draft.laneIndex];
  return {
    ...effect,
    startSeconds: draft.startSeconds,
    durationSeconds: draft.durationSeconds,
    target: draftLane?.target ?? effect.target,
    targetLabel: draftLane?.label ?? effect.targetLabel
  };
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

/** The mark under x in the mark ruler; the active collection, drawn on top, wins overlaps. */
export function hitSequenceMark(
  collections: SequenceMarkCollection[],
  activeCollectionKey: string | null,
  x: number,
  left: number,
  viewport: SequenceViewport
): SequenceMarkHit | null {
  if (x < left) return null;
  const ordered = [
    ...collections.filter((collection) => collection.key === activeCollectionKey),
    ...[...collections].reverse().filter((collection) => collection.key !== activeCollectionKey)
  ];
  for (const collection of ordered) {
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

export function reconcileSequenceSelection(document: SequenceEditorDocument | null, selection: SequenceSelection | null): SequenceSelection | null {
  if (document === null || selection === null) return null;
  if (selection.type === "clips") {
    const effectIds = selection.effectIds.filter((id) => document.effects.some((clip) => clip.id === id));
    const automationIds = selection.automationIds.filter((id) => document.automationClips.some((clip) => clip.id === id));
    if (effectIds.length + automationIds.length === 0) return null;
    if (effectIds.length === selection.effectIds.length && automationIds.length === selection.automationIds.length) return selection;
    return { type: "clips", effectIds, automationIds };
  }
  const marks = selection.marks.filter((mark) => document.markCollections.some((collection) => collection.key === mark.collectionKey && collection.marksSeconds[mark.index] !== undefined));
  return marks.length === 0 ? null : marks.length === selection.marks.length ? selection : { type: "marks", marks };
}

export function selectionFromSingle(selected: GuiFocus): SequenceSelection | null {
  const effectId = selectedEffectId(selected);
  if (effectId !== null) return { type: "clips", automationIds: [], effectIds: [effectId] };
  if (selected?.type === "automationClip") return { type: "clips", effectIds: [], automationIds: [selected.id] };
  if (selected?.type === "mark") return { type: "marks", marks: [{ collectionKey: selected.collectionKey, index: selected.index }] };
  return null;
}

export function singleSelectionFocus(selection: SequenceSelection | null): GuiFocus {
  if (selection?.type === "clips") {
    const automationId = selection.automationIds[0];
    if (selection.effectIds.length === 0 && selection.automationIds.length === 1 && automationId !== undefined) return { type: "automationClip", id: automationId };
    return selection.automationIds.length === 0 ? singleEffectSelectionFocus(selection.effectIds) : null;
  }
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
  return selection.type === "clips" ? selection.effectIds.length + selection.automationIds.length : selection.marks.length;
}

export function selectionCompatibleWithFocusedItem(selection: SequenceSelection, selected: GuiFocus) {
  const effectId = selectedEffectId(selected);
  if (effectId !== null) return selection.type === "clips" && selection.effectIds.includes(effectId);
  if (selected?.type === "automationClip") return selection.type === "clips" && selection.automationIds.includes(selected.id);
  if (selected?.type === "mark") {
    const mark = { collectionKey: selected.collectionKey, index: selected.index };
    return selection.type === "marks" && markLookupHas(markRefLookup(selection.marks), mark);
  }
  return true;
}

export function nextEffectSelection(current: SequenceSelection | null, id: number, shift: boolean, ctrl: boolean): SequenceSelection {
  if (current?.type !== "clips" || (!shift && !ctrl)) return { type: "clips", automationIds: [], effectIds: [id] };
  const ids = new Set(current.effectIds);
  if (ctrl && ids.has(id)) ids.delete(id);
  else ids.add(id);
  return { type: "clips", automationIds: current.automationIds, effectIds: [...ids] };
}

export function nextAutomationSelection(current: SequenceSelection | null, id: number, shift: boolean, ctrl: boolean): SequenceSelection {
  return mergeSequenceSelection(current, { type: "clips", effectIds: [], automationIds: [id] }, shift, ctrl);
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
  if (next.type === "clips") {
    const ids = new Set(current.type === "clips" ? current.effectIds : []);
    for (const id of next.effectIds) {
      if (ctrl && ids.has(id)) ids.delete(id);
      else ids.add(id);
    }
    const automationIds = new Set(current.type === "clips" ? current.automationIds : []);
    for (const id of next.automationIds) {
      if (ctrl && automationIds.has(id)) automationIds.delete(id);
      else automationIds.add(id);
    }
    return { type: "clips", automationIds: [...automationIds], effectIds: [...ids] };
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

export function selectionFromMarqueeEffects(clips: SequenceClipLayout[], automation: AutomationClipLayout[], marquee: SequenceMarquee): SequenceSelection {
  const box = normalizedRect(marquee.startX, marquee.startY, marquee.x, marquee.y);
  // A clip drawn on several lanes is selected once.
  return { type: "clips", automationIds: [...new Set(automation.filter((clip) => rectsIntersect(box, clip.rect)).map((clip) => clip.clip.id))], effectIds: [...new Set(clips.filter((clip) => rectsIntersect(box, clip.rect)).map((clip) => clip.effect.id))] };
}

/** A mark-ruler marquee selects the marks its time span covers. */
export function selectionFromMarqueeMarks(
  collections: SequenceMarkCollection[],
  marquee: SequenceMarquee,
  left: number,
  viewport: SequenceViewport
): SequenceSelection {
  const startX = Math.min(marquee.startX, marquee.x);
  const endX = Math.max(marquee.startX, marquee.x);
  const marks: SequenceMarkRef[] = [];
  for (const collection of collections) {
    collection.marksSeconds.forEach((timeSeconds, index) => {
      const x = left + (timeSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
      if (x + SEQUENCE_HIT_RADII.markPx >= startX && x - SEQUENCE_HIT_RADII.markPx <= endX) {
        marks.push({ collectionKey: collection.key, index });
      }
    });
  }
  return { type: "marks", marks };
}

/** `anchorLane` is the lane the gesture started on; each clip moves from its target's lane nearest it. */
export function clipSelectionGesture(document: SequenceEditorDocument, selection: Extract<SequenceSelection, { type: "clips" }>, edge: "none" | "left" | "right", requestedTimeDelta: number, anchorLane: number, requestedLaneDelta: number, automationResize: SequenceAutomationResize) {
  const clips = [
    ...document.effects.filter((clip) => selection.effectIds.includes(clip.id)).map((clip) => ({ ...clip, rowTarget: clip.target, kind: "effects" as const })),
    ...document.automationClips.filter((clip) => selection.automationIds.includes(clip.id)).map((clip) => ({ ...clip, kind: "automation" as const }))
  ];
  let minTime = -Infinity;
  let maxTime = Infinity;
  let minLane = -Infinity;
  let maxLane = Infinity;
  for (const clip of clips) {
    const lane = nearestLane(document, clip.rowTarget, anchorLane);
    if (lane < 0) throw new Error("Clip row target is missing.");
    minLane = Math.max(minLane, -lane);
    maxLane = Math.min(maxLane, document.lanes.length - 1 - lane);
    minTime = Math.max(minTime, edge === "right" ? MIN_EFFECT_DURATION_SECONDS - clip.durationSeconds : -clip.startSeconds);
    maxTime = Math.min(maxTime, edge === "left" ? clip.durationSeconds - MIN_EFFECT_DURATION_SECONDS : document.durationSeconds - clip.startSeconds - clip.durationSeconds);
  }
  const timeDeltaSeconds = clips.length === 0 ? 0 : clamp(requestedTimeDelta, minTime, maxTime);
  const laneDelta = edge === "none" && clips.length > 0 ? Math.trunc(clamp(requestedLaneDelta, minLane, maxLane)) : 0;
  const effects: SequenceDraft[] = [];
  const automation: AutomationDraft[] = [];
  for (const clip of clips) {
    const laneIndex = nearestLane(document, clip.rowTarget, anchorLane) + laneDelta;
    const timing = { id: clip.id, startSeconds: clip.startSeconds + (edge === "right" ? 0 : timeDeltaSeconds), durationSeconds: clip.durationSeconds + (edge === "none" ? 0 : edge === "left" ? -timeDeltaSeconds : timeDeltaSeconds) };
    if (clip.kind === "effects") effects.push({ ...timing, laneIndex });
    else {
      const content = edge !== "none" && automationResize === "crop" ? clip : timing;
      automation.push({ ...timing, rowTarget: targetAtLane(document, laneIndex), contentStartSeconds: content.startSeconds, contentDurationSeconds: content.durationSeconds });
    }
  }
  const edit = edge === "none"
    ? { type: "moveClips" as const, effectIds: selection.effectIds, automationIds: selection.automationIds, timeDeltaSeconds, anchorLane, laneDelta }
    : { type: "resizeClips" as const, effectIds: selection.effectIds, automationIds: selection.automationIds, edge, automation: automationResize, timeDeltaSeconds };
  return { effects, automation, edit, changed: timeDeltaSeconds !== 0 || laneDelta !== 0 };
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

export function markIndexAfterMove(collection: SequenceMarkCollection, index: number, timeSeconds: number) {
  const sorted = collection.marksSeconds
    .map((markTimeSeconds, markIndex) => ({
      markIndex,
      timeSeconds: markIndex === index ? timeSeconds : markTimeSeconds
    }))
    .sort((left, right) => left.timeSeconds - right.timeSeconds || left.markIndex - right.markIndex);
  return Math.max(0, sorted.findIndex((mark) => mark.markIndex === index));
}

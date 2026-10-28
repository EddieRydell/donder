import type { SequenceGuiDocument, SequenceClip, SequenceAutomationResize, SequenceClipRef, SequenceMarkCollection, SequenceMarkRef, SequenceSelection } from "../../../editor/types";

import { clamp, type GuiFocus } from "../shared";

import { nearestLane, targetAtLane } from "./sequenceTargets";
import { THEME_METRICS } from "../../../theme";
import { assignOverlapSlots, groupOverlappingClips, type SequenceRowLayout, type SequenceRowHeightMap, type AutomationClipLayout, type AutomationDraft } from "./sequenceAutomationLayout";

export type SequenceDraft = { id: number; startSeconds: number; durationSeconds: number; laneIndex: number };

export type SequenceClipKind = SequenceClipRef["type"];

export type MarkDraft = { collectionKey: string; index: number; timeSeconds: number; committedIndex?: number };

export type MarkDraftLookup = Map<string, Map<number, MarkDraft>>;

export type MarkRefLookup = Map<string, Set<number>>;

export type SequenceContextMenu =
  | { kind: "blank"; laneIndex: number; startSeconds: number }
  | { kind: "clip"; laneIndex: number; startSeconds: number; clipId: number }
  | { kind: "automation"; laneIndex: number; startSeconds: number; automationClipId: number }
  // Mark menus open from the mark ruler, which belongs to no lane.
  | { kind: "markRuler"; startSeconds: number }
  | { kind: "mark"; startSeconds: number; collectionKey: string; index: number };

export type SequenceHover =
  | null
  | { kind: "clip"; clipId: number; resize: "left" | "right" | "none" }
  | { kind: "mark"; collectionKey: string; index: number };

/**
 * A box selection whose start stays on the timeline while the view scrolls: `anchorSeconds`, and
 * `anchorY`, a content offset below the ruler for clips and spans or a canvas offset in the fixed mark
 * ruler. `x` and `y` are the pointer on the canvas. A span selects by time alone, across every lane.
 */
export type SequenceMarquee = { mode: "clips" | "marks" | "span";anchorSeconds: number; anchorY: number; x: number; y: number; active: boolean; shift: boolean; ctrl: boolean };
/** A box between two corners in one coordinate space. */
export type MarqueeBox = { startX: number; startY: number; x: number; y: number };

export const MIN_CLIP_DURATION_SECONDS = 0.000000001;

const SEQUENCE_HIT_RADII = {
  clipResizeHandlePx: THEME_METRICS.sequenceClipResizeHitWidth,
  markPx: THEME_METRICS.sequenceMarkHitRadius
} as const;

export type SequenceViewport = {
  pxPerSecond: number;
  /** Width of the row-name column left of the timeline. */
  leftGutterWidth: number;
  audioStripHeight: number;
  markRulerHeight: number;
  rowHeights: SequenceRowHeightMap;
  /** Group lanes whose members are hidden, by group fixture. */
  collapsedGroups: ReadonlySet<number>;
  scrollXSeconds: number;
  scrollY: number;
};

export type SequenceClipLayout = {
  clip: SequenceClip;
  laneIndex: number;
  rect: { x: number; y: number; width: number; height: number };
};

export type SequenceClipLayoutBounds = {
  width: number;
  height: number;
};

type LaneClip = {
  clip: SequenceClip;
  laneIndex: number;
};


export type SequenceHit = {
  clip: SequenceClip;
  laneIndex: number;
  resize: "left" | "right" | "none";
};

export type SequenceMarkHit = {
  collectionKey: string;
  index: number;
  timeSeconds: number;
};

export function buildSequenceClipLayout(
  document: SequenceGuiDocument,
  drafts: SequenceDraft[],
  viewport: SequenceViewport,
  left: number,
  top: number,
  bounds: SequenceClipLayoutBounds,
  rows: SequenceRowLayout[]
): SequenceClipLayout[] {
  // Lanes scrolled out of view vertically lay out no clips.
  const visibleRows = new Map<number, SequenceRowLayout>();
  for (const row of rows) {
    const rowTop = top + row.top - viewport.scrollY;
    if (row.kind === "clips" && row.height > 0 && rowTop + row.height >= top && rowTop <= bounds.height) visibleRows.set(row.laneIndex, row);
  }
  const lanesByTarget = new Map<number, number[]>();
  document.lanes.forEach((lane, index) => {
    if (!visibleRows.has(index)) return;
    const lanes = lanesByTarget.get(lane.target.fixture);
    if (lanes === undefined) lanesByTarget.set(lane.target.fixture, [index]);
    else lanes.push(index);
  });
  const draftById = new Map(drafts.map((draft) => [draft.id, draft]));
  const visibleStartSeconds = viewport.scrollXSeconds;
  const visibleEndSeconds = viewport.scrollXSeconds + Math.max(1, bounds.width - left) / viewport.pxPerSecond;
  // A clip shows on every lane of its target, including a draft's destination target.
  const byLane = new Map<number, LaneClip[]>();
  for (const original of document.clips) {
    const activeDraft = draftById.get(original.id);
    const clip = activeDraft === undefined ? original : clipFromDraft(document, original, activeDraft);
    if (!clipIntersectsTimeRange(clip, visibleStartSeconds, visibleEndSeconds)) continue;
    for (const laneIndex of lanesByTarget.get(clip.target.fixture) ?? []) {
      const laneClips = byLane.get(laneIndex);
      if (laneClips === undefined) byLane.set(laneIndex, [{ clip, laneIndex }]);
      else laneClips.push({ clip, laneIndex });
    }
  }

  const layouts: SequenceClipLayout[] = [];
  for (const [laneIndex, laneClips] of byLane) {
    const row = visibleRows.get(laneIndex);
    if (row === undefined) throw new Error("Clip has no timeline row.");
    const groups = groupOverlappingClips(laneClips, (clip) => clip.clip);
    for (const group of groups) {
      const assigned = assignOverlapSlots(group, (clip) => clip.clip);
      const slotCount = Math.max(1, Math.max(...assigned.map((clip) => clip.slot)) + 1);
      const laneHeight = row.height;
      const slotHeight = laneHeight / slotCount;
      for (const clip of assigned) {
        const startSeconds = clip.clip.startSeconds;
        const endSeconds = startSeconds + clip.clip.durationSeconds;
        const x = left + (startSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
        const width = Math.max(THEME_METRICS.sequenceClipMinWidth, (endSeconds - startSeconds) * viewport.pxPerSecond);
        layouts.push({
          clip: clip.clip,
          laneIndex,
          rect: {
            x,
            y: top + row.top - viewport.scrollY + clip.slot * slotHeight + THEME_METRICS.sequenceClipSlotOffset,
            width,
            height: Math.max(0, slotHeight - THEME_METRICS.sequenceClipHandleInset)
          }
        });
      }
    }
  }
  return layouts;
}

function clipFromDraft(
  document: SequenceGuiDocument,
  clip: SequenceClip,
  draft: SequenceDraft
): SequenceClip {
  const draftLane = document.lanes[draft.laneIndex];
  return {
    ...clip,
    startSeconds: draft.startSeconds,
    durationSeconds: draft.durationSeconds,
    target: draftLane?.target ?? clip.target,
    targetLabel: draftLane?.label ?? clip.targetLabel
  };
}

function clipIntersectsTimeRange(clip: SequenceClip, startSeconds: number, endSeconds: number): boolean {
  const clipStart = clip.startSeconds;
  const clipEnd = clip.startSeconds + clip.durationSeconds;
  return clipEnd >= startSeconds && clipStart <= endSeconds;
}

export function hitSequence(clips: SequenceClipLayout[], x: number, y: number): SequenceHit | null {
  for (let index = clips.length - 1; index >= 0; index -= 1) {
    const clip = clips[index];
    if (clip === undefined) continue;
    const { rect } = clip;
    if (x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height) {
      const resize: "left" | "right" | "none" =
        x - rect.x < SEQUENCE_HIT_RADII.clipResizeHandlePx ? "left" : rect.x + rect.width - x < SEQUENCE_HIT_RADII.clipResizeHandlePx ? "right" : "none";
      return {
        clip: clip.clip,
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
  if (left.kind === "clip" && right.kind === "clip") {
    return left.clipId === right.clipId && left.resize === right.resize;
  }
  if (left.kind !== "mark" || right.kind !== "mark") return false;
  return left.collectionKey === right.collectionKey && left.index === right.index;
}

export function selectedClipId(selected: GuiFocus): number | null {
  return selected?.type === "clip" ? selected.id : null;
}

/** The ids of the clips of each kind. */
export function clipIdSets(clips: SequenceClipRef[]): Record<SequenceClipKind, Set<number>> {
  const sets = { clip: new Set<number>(), automation: new Set<number>() };
  for (const clip of clips) sets[clip.type].add(clip.id);
  return sets;
}

/** The ids of the clips of one kind. */
export function clipIdsOfKind(clips: SequenceClipRef[], type: SequenceClipKind): number[] {
  return clips.filter((clip) => clip.type === type).map((clip) => clip.id);
}

export function hasClip(clips: SequenceClipRef[], type: SequenceClipKind, id: number) {
  return clips.some((clip) => clip.type === type && clip.id === id);
}

export function clipRefs(type: SequenceClipKind, ids: Iterable<number>): SequenceClipRef[] {
  return [...ids].map((id) => ({ type, id }));
}

export function reconcileSequenceSelection(document: SequenceGuiDocument | null, selection: SequenceSelection | null): SequenceSelection | null {
  if (document === null || selection === null) return null;
  if (selection.type === "clips") {
    const existing = {
      clip: new Set(document.clips.map((clip) => clip.id)),
      automation: new Set(document.automationClips.map((automationClip) => automationClip.id))
    };
    const clips = selection.clips.filter((clip) => existing[clip.type].has(clip.id));
    if (clips.length === 0) return null;
    return clips.length === selection.clips.length ? selection : { type: "clips", clips };
  }
  const marks = selection.marks.filter((mark) => document.markCollections.some((collection) => collection.key === mark.collectionKey && collection.marksSeconds[mark.index] !== undefined));
  return marks.length === 0 ? null : marks.length === selection.marks.length ? selection : { type: "marks", marks };
}

export function selectionFromSingle(selected: GuiFocus): SequenceSelection | null {
  const clipId = selectedClipId(selected);
  if (clipId !== null) return { type: "clips", clips: [{ type: "clip", id: clipId }] };
  if (selected?.type === "automationClip") return { type: "clips", clips: [{ type: "automation", id: selected.id }] };
  if (selected?.type === "mark") return { type: "marks", marks: [{ collectionKey: selected.collectionKey, index: selected.index }] };
  return null;
}

export function singleSelectionFocus(selection: SequenceSelection | null): GuiFocus {
  if (selection?.type === "clips") {
    const clip = selection.clips[0];
    if (selection.clips.length !== 1 || clip === undefined) return null;
    return clip.type === "automation" ? { type: "automationClip", id: clip.id } : { type: "clip", id: clip.id };
  }
  if (selection?.type === "marks" && selection.marks.length === 1) {
    const mark = selection.marks[0];
    return mark === undefined ? null : { type: "mark", collectionKey: mark.collectionKey, index: mark.index };
  }
  return null;
}

export function selectionCount(selection: SequenceSelection) {
  return selection.type === "clips" ? selection.clips.length : selection.marks.length;
}

export function selectionCompatibleWithFocusedItem(selection: SequenceSelection, selected: GuiFocus) {
  const clipId = selectedClipId(selected);
  if (clipId !== null) return selection.type === "clips" && hasClip(selection.clips, "clip", clipId);
  if (selected?.type === "automationClip") return selection.type === "clips" && hasClip(selection.clips, "automation", selected.id);
  if (selected?.type === "mark") {
    const mark = { collectionKey: selected.collectionKey, index: selected.index };
    return selection.type === "marks" && markLookupHas(markRefLookup(selection.marks), mark);
  }
  return true;
}

export function nextClipSelection(current: SequenceSelection | null, id: number, shift: boolean, ctrl: boolean): SequenceSelection {
  return mergeSequenceSelection(current, { type: "clips", clips: [{ type: "clip", id }] }, shift, ctrl);
}

export function nextAutomationSelection(current: SequenceSelection | null, id: number, shift: boolean, ctrl: boolean): SequenceSelection {
  return mergeSequenceSelection(current, { type: "clips", clips: [{ type: "automation", id }] }, shift, ctrl);
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
    const chosen = clipIdSets(current.type === "clips" ? current.clips : []);
    for (const clip of next.clips) {
      const ids = chosen[clip.type];
      if (ctrl && ids.has(clip.id)) ids.delete(clip.id);
      else ids.add(clip.id);
    }
    return { type: "clips", clips: [...clipRefs("clip", chosen.clip), ...clipRefs("automation", chosen.automation)] };
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

export function selectionFromMarqueeClips(clips: SequenceClipLayout[], automation: AutomationClipLayout[], marquee: MarqueeBox): SequenceSelection {
  const box = normalizedRect(marquee.startX, marquee.startY, marquee.x, marquee.y);
  // A clip drawn on several lanes is selected once.
  return { type: "clips", clips: [
    ...clipRefs("clip", new Set(clips.filter((clip) => rectsIntersect(box, clip.rect)).map((clip) => clip.clip.id))),
    ...clipRefs("automation", new Set(automation.filter((layout) => rectsIntersect(box, layout.rect)).map((layout) => layout.clip.id)))
  ] };
}

/** Every clip overlapping the time span, including clips on collapsed lanes. */
export function selectionFromTimeSpan(document: SequenceGuiDocument, startSeconds: number, endSeconds: number): SequenceSelection {
  const start = Math.min(startSeconds, endSeconds);
  const end = Math.max(startSeconds, endSeconds);
  const overlaps = (clip: { startSeconds: number; durationSeconds: number }) => clip.startSeconds < end && clip.startSeconds + clip.durationSeconds > start;
  return { type: "clips", clips: [
    ...clipRefs("clip", document.clips.filter(overlaps).map((clip) => clip.id)),
    ...clipRefs("automation", document.automationClips.filter(overlaps).map((automationClip) => automationClip.id))
  ] };
}

/** A mark-ruler marquee selects the marks its time span covers. */
export function selectionFromMarqueeMarks(
  collections: SequenceMarkCollection[],
  marquee: MarqueeBox,
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
/** The earliest start among clips, where the selection line sits for a clip selection. */
export function earliestStartSeconds(clips: Array<{ startSeconds: number }>): number | null {
  return clips.length === 0 ? null : Math.min(...clips.map((clip) => clip.startSeconds));
}

export function selectionStartSeconds(document: SequenceGuiDocument, selection: SequenceSelection | null): number | null {
  if (selection?.type !== "clips") return null;
  const { clip: clipIds, automation: automationIds } = clipIdSets(selection.clips);
  return earliestStartSeconds([
    ...document.clips.filter((clip) => clipIds.has(clip.id)),
    ...document.automationClips.filter((automationClip) => automationIds.has(automationClip.id))
  ]);
}

export function clipSelectionGesture(document: SequenceGuiDocument, selection: Extract<SequenceSelection, { type: "clips" }>, edge: "none" | "left" | "right", requestedTimeDelta: number, anchorLane: number, requestedLaneDelta: number, automationResize: SequenceAutomationResize) {
  const { clip: clipIds, automation: automationIds } = clipIdSets(selection.clips);
  const moving = [
    ...document.clips.filter((clip) => clipIds.has(clip.id)).map((clip) => ({ ...clip, rowTarget: clip.target, kind: "clips" as const })),
    ...document.automationClips.filter((automationClip) => automationIds.has(automationClip.id)).map((automationClip) => ({ ...automationClip, kind: "automation" as const }))
  ];
  let minTime = -Infinity;
  let maxTime = Infinity;
  let minLane = -Infinity;
  let maxLane = Infinity;
  for (const clip of moving) {
    const lane = nearestLane(document, clip.rowTarget, anchorLane);
    if (lane < 0) throw new Error("Clip row target is missing.");
    minLane = Math.max(minLane, -lane);
    maxLane = Math.min(maxLane, document.lanes.length - 1 - lane);
    minTime = Math.max(minTime, edge === "right" ? MIN_CLIP_DURATION_SECONDS - clip.durationSeconds : -clip.startSeconds);
    maxTime = Math.min(maxTime, edge === "left" ? clip.durationSeconds - MIN_CLIP_DURATION_SECONDS : document.durationSeconds - clip.startSeconds - clip.durationSeconds);
  }
  const timeDeltaSeconds = moving.length === 0 ? 0 : clamp(requestedTimeDelta, minTime, maxTime);
  const laneDelta = edge === "none" && moving.length > 0 ? Math.trunc(clamp(requestedLaneDelta, minLane, maxLane)) : 0;
  const clips: SequenceDraft[] = [];
  const automation: AutomationDraft[] = [];
  for (const clip of moving) {
    const laneIndex = nearestLane(document, clip.rowTarget, anchorLane) + laneDelta;
    const timing = { id: clip.id, startSeconds: clip.startSeconds + (edge === "right" ? 0 : timeDeltaSeconds), durationSeconds: clip.durationSeconds + (edge === "none" ? 0 : edge === "left" ? -timeDeltaSeconds : timeDeltaSeconds) };
    if (clip.kind === "clips") clips.push({ ...timing, laneIndex });
    else {
      const content = edge !== "none" && automationResize === "crop" ? clip : timing;
      automation.push({ ...timing, rowTarget: targetAtLane(document, laneIndex), contentStartSeconds: content.startSeconds, contentDurationSeconds: content.durationSeconds });
    }
  }
  const edit = edge === "none"
    ? { type: "moveClips" as const, clips: selection.clips, timeDeltaSeconds, anchorLane, laneDelta }
    : { type: "resizeClips" as const, clips: selection.clips, edge, automation: automationResize, timeDeltaSeconds };
  return { clips, automation, edit, changed: timeDeltaSeconds !== 0 || laneDelta !== 0 };
}

export function constrainMarkDelta(document: SequenceGuiDocument, marks: SequenceMarkRef[], deltaSeconds: number) {
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

export function markMoveDrafts(document: SequenceGuiDocument, marks: SequenceMarkRef[], deltaSeconds: number): MarkDraftLookup {
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

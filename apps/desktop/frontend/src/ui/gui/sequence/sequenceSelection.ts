import type { GuiClip, GuiPlacement, SequenceGuiDocument, SequenceAutomationResize, SequenceMarkCollection, SequenceMarkRef, SequenceSelection } from "../../../types";

import { clamp } from "../shared";
import { clipFocus, focusedClipId, useDocumentUi, type GuiFocus } from "../documentUi";

/** A clip placed at a time, as every clip of a sequence is. */
export type TimedClip = GuiClip & { placement: Extract<GuiPlacement, { type: "timed" }> };

export function isTimed(clip: GuiClip): clip is TimedClip {
  return clip.placement.type === "timed";
}

import { nearestLane } from "./sequenceTargets";
import { THEME_METRICS } from "../../../theme";
import { assignOverlapSlots, groupOverlappingClips, type SequenceRowKind, type SequenceRowLayout, type SequenceRowHeightMap } from "./sequenceRows";
import type { AutomationCurveDraft } from "./sequenceAutomationLayout";

/** A clip being moved or resized. An envelope's content window is the span its
 * curve covers: a crop moves the clip over fixed content, a stretch moves both. */
export type SequenceDraft = { id: number; startSeconds: number; durationSeconds: number; laneIndex: number; contentStartSeconds: number; contentDurationSeconds: number };

/** The row a clip is drawn in: an envelope's own row below its target's effects. */
export function clipRowKind(clip: GuiClip): SequenceRowKind {
  return clip.content.type === "envelope" ? "automation" : "clips";
}

export type MarkDraft = { collectionId: number; index: number; timeSeconds: number; committedIndex?: number };

export type MarkDraftLookup = Map<number, Map<number, MarkDraft>>;

export type MarkRefLookup = Map<number, Set<number>>;

export type SequenceContextMenu =
  | { kind: "blank"; laneIndex: number; startSeconds: number }
  | { kind: "clip"; laneIndex: number; startSeconds: number; clipId: number }
  // Mark menus open from the mark ruler, which belongs to no lane.
  | { kind: "markRuler"; startSeconds: number }
  | { kind: "mark"; startSeconds: number; collectionId: number; index: number };

export type SequenceHover =
  | null
  | { kind: "clip"; clipId: number; resize: "left" | "right" | "none" }
  | { kind: "mark"; collectionId: number; index: number };

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
  clip: TimedClip;
  /** The lane drawing this copy; a target shared by several groups has several. */
  laneIndex: number;
  rect: { x: number; y: number; width: number; height: number };
  /** Canvas span of an envelope's content window; shares the clip's vertical extent. */
  curveRect: { x: number; y: number; width: number; height: number };
};

export type SequenceClipLayoutBounds = {
  width: number;
  height: number;
};

type LaneClip = {
  clip: TimedClip;
  laneIndex: number;
  contentStartSeconds: number;
  contentDurationSeconds: number;
};


export type SequenceHit = SequenceClipLayout & {
  resize: "left" | "right" | "none";
};

export type SequenceMarkHit = {
  collectionId: number;
  index: number;
  timeSeconds: number;
};

/** Every clip in its target's rows: effect clips in the clips row, envelopes in the envelope row, each with any draft of its timing or curve. */
export function buildSequenceClipLayout(
  document: SequenceGuiDocument,
  drafts: SequenceDraft[],
  curveDraft: AutomationCurveDraft | null,
  viewport: SequenceViewport,
  left: number,
  top: number,
  bounds: SequenceClipLayoutBounds,
  rows: SequenceRowLayout[]
): SequenceClipLayout[] {
  // Rows scrolled out of view vertically lay out no clips.
  const visibleRows = new Map<string, SequenceRowLayout>();
  const rowKey = (laneIndex: number, kind: SequenceRowKind) => `${laneIndex}:${kind}`;
  for (const row of rows) {
    const rowTop = top + row.top - viewport.scrollY;
    if (row.height > 0 && rowTop + row.height >= top && rowTop <= bounds.height) visibleRows.set(rowKey(row.laneIndex, row.kind), row);
  }
  const lanesByTarget = new Map<number, number[]>();
  document.targets.forEach((lane, index) => {
    const lanes = lanesByTarget.get(lane.target.fixture);
    if (lanes === undefined) lanesByTarget.set(lane.target.fixture, [index]);
    else lanes.push(index);
  });
  const draftById = new Map(drafts.map((draft) => [draft.id, draft]));
  const visibleStartSeconds = viewport.scrollXSeconds;
  const visibleEndSeconds = viewport.scrollXSeconds + Math.max(1, bounds.width - left) / viewport.pxPerSecond;
  // A clip shows on every lane of its target, including a draft's destination target.
  const byRow = new Map<string, LaneClip[]>();
  for (const authored of document.clips.filter(isTimed)) {
    const activeDraft = draftById.get(authored.id);
    const original = curveDraft?.id === authored.id && authored.content.type === "envelope"
      ? { ...authored, content: { ...authored.content, curve: curveDraft.curve } }
      : authored;
    const clip = activeDraft === undefined ? original : clipFromDraft(document, original, activeDraft);
    if (!clipIntersectsTimeRange(clip, visibleStartSeconds, visibleEndSeconds)) continue;
    const content = { contentStartSeconds: activeDraft?.contentStartSeconds ?? clip.placement.startSeconds, contentDurationSeconds: activeDraft?.contentDurationSeconds ?? clip.placement.durationSeconds };
    for (const laneIndex of lanesByTarget.get(clip.target.fixture) ?? []) {
      const key = rowKey(laneIndex, clipRowKind(clip));
      if (!visibleRows.has(key)) continue;
      const rowClips = byRow.get(key);
      if (rowClips === undefined) byRow.set(key, [{ clip, laneIndex, ...content }]);
      else rowClips.push({ clip, laneIndex, ...content });
    }
  }

  const layouts: SequenceClipLayout[] = [];
  for (const [key, laneClips] of byRow) {
    const row = visibleRows.get(key);
    if (row === undefined) throw new Error("Clip has no timeline row.");
    const laneIndex = row.laneIndex;
    const timing = ({ clip }: LaneClip) => ({ id: clip.id, ...clip.placement });
    const groups = groupOverlappingClips(laneClips, timing);
    for (const group of groups) {
      const assigned = assignOverlapSlots(group, timing);
      const slotCount = Math.max(1, Math.max(...assigned.map((clip) => clip.slot)) + 1);
      const laneHeight = row.height;
      const slotHeight = laneHeight / slotCount;
      for (const clip of assigned) {
        const { startSeconds, durationSeconds } = clip.clip.placement;
        const endSeconds = startSeconds + durationSeconds;
        const x = left + (startSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
        const width = Math.max(THEME_METRICS.sequenceClipMinWidth, (endSeconds - startSeconds) * viewport.pxPerSecond);
        const rect = {
          x,
          y: top + row.top - viewport.scrollY + clip.slot * slotHeight + THEME_METRICS.sequenceClipSlotOffset,
          width,
          height: Math.max(0, slotHeight - THEME_METRICS.sequenceClipHandleInset)
        };
        layouts.push({
          clip: clip.clip,
          laneIndex,
          rect,
          curveRect: {
            ...rect,
            x: left + (clip.contentStartSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond,
            width: Math.max(THEME_METRICS.visualMinSize, clip.contentDurationSeconds * viewport.pxPerSecond)
          }
        });
      }
    }
  }
  return layouts;
}

function clipFromDraft(
  document: SequenceGuiDocument,
  clip: TimedClip,
  draft: SequenceDraft
): TimedClip {
  const draftLane = document.targets[draft.laneIndex];
  return {
    ...clip,
    placement: { type: "timed", startSeconds: draft.startSeconds, durationSeconds: draft.durationSeconds },
    target: draftLane?.target ?? clip.target,
    targetLabel: draftLane?.label ?? clip.targetLabel
  };
}

function clipIntersectsTimeRange(clip: TimedClip, startSeconds: number, endSeconds: number): boolean {
  const clipStart = clip.placement.startSeconds;
  const clipEnd = clipStart + clip.placement.durationSeconds;
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
      return { ...clip, resize };
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
    ...collections.filter((collection) => collection.name === activeCollectionKey),
    ...[...collections].reverse().filter((collection) => collection.name !== activeCollectionKey)
  ];
  for (const collection of ordered) {
    for (let index = collection.marksSeconds.length - 1; index >= 0; index -= 1) {
      const timeSeconds = collection.marksSeconds[index] ?? 0;
      const markX = left + (timeSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
      if (Math.abs(x - markX) <= SEQUENCE_HIT_RADII.markPx) {
        return { collectionId: collection.id, index, timeSeconds };
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
  return left.collectionId === right.collectionId && left.index === right.index;
}

/** Whether two selections hold the same items in the same order. */
export function sequenceSelectionsEqual(left: SequenceSelection | null, right: SequenceSelection | null) {
  if (left === right) return true;
  if (left === null || right === null) return false;
  if (left.type === "clips" && right.type === "clips") {
    return left.ids.length === right.ids.length && left.ids.every((id, index) => id === right.ids[index]);
  }
  if (left.type !== "marks" || right.type !== "marks") return false;
  return left.marks.length === right.marks.length && left.marks.every((mark, index) => mark.collectionId === right.marks[index]?.collectionId && mark.index === right.marks[index].index);
}


/** The open sequence's selection, without clips or marks it no longer has. */
export function useSequenceSelection(document: SequenceGuiDocument): SequenceSelection | null {
  return reconcileSequenceSelection(document, useDocumentUi((state) => state.selection));
}

export function reconcileSequenceSelection(document: SequenceGuiDocument | null, selection: SequenceSelection | null): SequenceSelection | null {
  if (document === null || selection === null) return null;
  if (selection.type === "clips") {
    const existing = new Set(document.clips.map((clip) => clip.id));
    const ids = selection.ids.filter((id) => existing.has(id));
    if (ids.length === 0) return null;
    return ids.length === selection.ids.length ? selection : { type: "clips", ids };
  }
  const marks = selection.marks.filter((mark) => document.markCollections.some((collection) => collection.id === mark.collectionId && collection.marksSeconds[mark.index] !== undefined));
  return marks.length === 0 ? null : marks.length === selection.marks.length ? selection : { type: "marks", marks };
}

export function selectionFromSingle(selected: GuiFocus): SequenceSelection | null {
  const clipId = focusedClipId(selected);
  if (clipId !== null) return { type: "clips", ids: [clipId] };
  if (selected?.type === "mark") return { type: "marks", marks: [{ collectionId: selected.collectionId, index: selected.index }] };
  return null;
}

export function singleSelectionFocus(selection: SequenceSelection | null): GuiFocus {
  if (selection?.type === "clips") {
    const id = selection.ids[0];
    if (selection.ids.length !== 1 || id === undefined) return null;
    return clipFocus(id);
  }
  if (selection?.type === "marks" && selection.marks.length === 1) {
    const mark = selection.marks[0];
    return mark === undefined ? null : { type: "mark", collectionId: mark.collectionId, index: mark.index };
  }
  return null;
}

export function selectionCount(selection: SequenceSelection) {
  return selection.type === "clips" ? selection.ids.length : selection.marks.length;
}

export function selectionCompatibleWithFocusedItem(selection: SequenceSelection, selected: GuiFocus) {
  const clipId = focusedClipId(selected);
  if (clipId !== null) return selection.type === "clips" && selection.ids.includes(clipId);
  if (selected?.type === "mark") {
    const mark = { collectionId: selected.collectionId, index: selected.index };
    return selection.type === "marks" && markLookupHas(markRefLookup(selection.marks), mark);
  }
  return true;
}

export function nextClipSelection(current: SequenceSelection | null, id: number, shift: boolean, ctrl: boolean): SequenceSelection {
  return mergeSequenceSelection(current, { type: "clips", ids: [id] }, shift, ctrl);
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
    const ids = new Set(current.type === "clips" ? current.ids : []);
    for (const id of next.ids) {
      if (ctrl && ids.has(id)) ids.delete(id);
      else ids.add(id);
    }
    return { type: "clips", ids: [...ids] };
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

export function selectionFromMarqueeClips(clips: SequenceClipLayout[], marquee: MarqueeBox): SequenceSelection {
  const box = normalizedRect(marquee.startX, marquee.startY, marquee.x, marquee.y);
  // A clip drawn on several lanes is selected once.
  return { type: "clips", ids: [...new Set(clips.filter((clip) => rectsIntersect(box, clip.rect)).map((clip) => clip.clip.id))] };
}

/** Every clip overlapping the time span, including clips on collapsed lanes. */
export function selectionFromTimeSpan(document: SequenceGuiDocument, startSeconds: number, endSeconds: number): SequenceSelection {
  const start = Math.min(startSeconds, endSeconds);
  const end = Math.max(startSeconds, endSeconds);
  const overlaps = ({ placement }: TimedClip) => placement.startSeconds < end && placement.startSeconds + placement.durationSeconds > start;
  return { type: "clips", ids: document.clips.filter(isTimed).filter(overlaps).map((clip) => clip.id) };
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
        marks.push({ collectionId: collection.id, index });
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
  return earliestStartSeconds(selectedClips(document, selection.ids).map((clip) => clip.placement));
}

/** The clips of `ids`, timed. */
export function selectedClips(document: SequenceGuiDocument, ids: number[]): TimedClip[] {
  const chosen = new Set(ids);
  return document.clips.filter(isTimed).filter((clip) => chosen.has(clip.id));
}

export function clipSelectionGesture(document: SequenceGuiDocument, selection: Extract<SequenceSelection, { type: "clips" }>, edge: "none" | "left" | "right", requestedTimeDelta: number, anchorLane: number, requestedLaneDelta: number, automationResize: SequenceAutomationResize) {
  const moving = selectedClips(document, selection.ids);
  let minTime = -Infinity;
  let maxTime = Infinity;
  let minLane = -Infinity;
  let maxLane = Infinity;
  for (const { target, placement: clip } of moving) {
    const lane = nearestLane(document, target, anchorLane);
    if (lane < 0) throw new Error("Clip row target is missing.");
    minLane = Math.max(minLane, -lane);
    maxLane = Math.min(maxLane, document.targets.length - 1 - lane);
    minTime = Math.max(minTime, edge === "right" ? MIN_CLIP_DURATION_SECONDS - clip.durationSeconds : -clip.startSeconds);
    maxTime = Math.min(maxTime, edge === "left" ? clip.durationSeconds - MIN_CLIP_DURATION_SECONDS : document.durationSeconds - clip.startSeconds - clip.durationSeconds);
  }
  const timeDeltaSeconds = moving.length === 0 ? 0 : clamp(requestedTimeDelta, minTime, maxTime);
  const laneDelta = edge === "none" && moving.length > 0 ? Math.trunc(clamp(requestedLaneDelta, minLane, maxLane)) : 0;
  const clips = moving.map(({ id, target, placement: clip }): SequenceDraft => {
    const laneIndex = nearestLane(document, target, anchorLane) + laneDelta;
    const timing = { startSeconds: clip.startSeconds + (edge === "right" ? 0 : timeDeltaSeconds), durationSeconds: clip.durationSeconds + (edge === "none" ? 0 : edge === "left" ? -timeDeltaSeconds : timeDeltaSeconds) };
    // A cropped envelope keeps its curve where it was; anything else carries its content along.
    const content = edge !== "none" && automationResize === "crop" ? clip : timing;
    return { id, ...timing, laneIndex, contentStartSeconds: content.startSeconds, contentDurationSeconds: content.durationSeconds };
  });
  const edit = edge === "none"
    ? { type: "moveClips" as const, ids: selection.ids, timeDeltaSeconds, anchorLane, laneDelta }
    : { type: "resizeClips" as const, ids: selection.ids, edge, automation: automationResize, timeDeltaSeconds };
  return { clips, edit, changed: timeDeltaSeconds !== 0 || laneDelta !== 0 };
}

export function constrainMarkDelta(document: SequenceGuiDocument, marks: SequenceMarkRef[], deltaSeconds: number) {
  let minDelta = -Infinity;
  let maxDelta = Infinity;
  for (const mark of marks) {
    const collection = document.markCollections.find((candidate) => candidate.id === mark.collectionId);
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
    const collection = document.markCollections.find((candidate) => candidate.id === mark.collectionId);
    const timeSeconds = collection?.marksSeconds[mark.index];
    if (collection === undefined || timeSeconds === undefined) continue;
    const nextTimeSeconds = clamp(timeSeconds + deltaSeconds, 0, document.durationSeconds);
    setMarkDraft(drafts, mark, {
      collectionId: mark.collectionId,
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
  return lookup.get(mark.collectionId)?.has(mark.index) ?? false;
}

function addMarkRef(lookup: MarkRefLookup, mark: SequenceMarkRef) {
  const collection = lookup.get(mark.collectionId) ?? new Set<number>();
  collection.add(mark.index);
  lookup.set(mark.collectionId, collection);
}

function removeMarkRef(lookup: MarkRefLookup, mark: SequenceMarkRef) {
  const collection = lookup.get(mark.collectionId);
  if (collection === undefined) return;
  collection.delete(mark.index);
  if (collection.size === 0) lookup.delete(mark.collectionId);
}

function markRefsFromLookup(lookup: MarkRefLookup): SequenceMarkRef[] {
  const marks: SequenceMarkRef[] = [];
  for (const [collectionId, indexes] of lookup) {
    for (const index of indexes) {
      marks.push({ collectionId, index });
    }
  }
  return marks;
}

export function getMarkDraft(lookup: MarkDraftLookup, mark: SequenceMarkRef): MarkDraft | undefined {
  return lookup.get(mark.collectionId)?.get(mark.index);
}

export function setMarkDraft(lookup: MarkDraftLookup, mark: SequenceMarkRef, draft: MarkDraft) {
  const collection = lookup.get(mark.collectionId) ?? new Map<number, MarkDraft>();
  collection.set(mark.index, draft);
  lookup.set(mark.collectionId, collection);
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

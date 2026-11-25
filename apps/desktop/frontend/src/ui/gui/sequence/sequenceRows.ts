import type { FixtureTarget, PersistedSequenceViewportState, SequenceLane } from "../../../types";

import { clamp, roundToNanosecond } from "../shared";
import { THEME_METRICS } from "../../../theme";

/** Every lane has a row of effect clips and a row of envelopes. */
export type SequenceRowKind = "clips" | "automation";
export type SequenceRowHeightMap = Record<number, Record<SequenceRowKind, number>>;

export type SequenceRowLayout = {
  laneIndex: number;
  target: FixtureTarget;
  kind: SequenceRowKind;
  top: number;
  height: number;
  bottom: number;
};

/** The least height a row of this kind keeps: an envelope row needs room to edit its curve. */
export function minRowHeight(kind: SequenceRowKind): number {
  return kind === "automation" ? THEME_METRICS.automationRowMinHeight : THEME_METRICS.sequenceMinLaneHeight;
}

export function automationLaneRowHeight(laneHeight: number): number {
  return clamp(
    laneHeight * THEME_METRICS.automationRowHeightRatio,
    THEME_METRICS.automationRowMinHeight,
    THEME_METRICS.automationRowMaxHeight
  );
}

// The persisted suffix denotes row kind, never a visible row index.
function persistedRowHeightKey(fixture: number, kind: SequenceRowKind): string {
  return `${fixture}:row:${kind === "clips" ? 0 : 1}`;
}

export function persistRowHeights(heights: SequenceRowHeightMap): PersistedSequenceViewportState["rowHeights"] {
  return Object.fromEntries(Object.entries(heights).flatMap(([fixture, sizes]) =>
    (["clips", "automation"] as const).map((kind) => [persistedRowHeightKey(Number(fixture), kind), sizes[kind]])));
}

export function restoreRowHeights(lanes: SequenceLane[], persisted: PersistedSequenceViewportState["rowHeights"] | undefined, defaultHeight: number): SequenceRowHeightMap {
  return Object.fromEntries(lanes.map((lane) => [lane.target.fixture, {
    clips: clamp(persisted?.[persistedRowHeightKey(lane.target.fixture, "clips")] ?? defaultHeight, THEME_METRICS.sequenceMinLaneHeight, THEME_METRICS.sequenceMaxLaneHeight),
    automation: clamp(persisted?.[persistedRowHeightKey(lane.target.fixture, "automation")] ?? automationLaneRowHeight(defaultHeight), minRowHeight("automation"), THEME_METRICS.sequenceMaxLaneHeight)
  }]));
}

export function rowHeightAt(rowHeights: SequenceRowHeightMap, target: FixtureTarget, kind: SequenceRowKind, defaultHeight: number): number {
  return rowHeights[target.fixture]?.[kind] ?? defaultHeight;
}

/** A target's envelope row shows when it holds an envelope, or when `revealAutomation` shows them all. */
export function sequenceRowLayout(lanes: SequenceLane[], envelopeTargets: FixtureTarget[], rowHeights: SequenceRowHeightMap, defaultMainRowHeight: number, defaultAutomationRowHeight: number, revealAutomation: boolean): SequenceRowLayout[] {
  const occupied = new Set(envelopeTargets.map((target) => target.fixture));
  const rows: SequenceRowLayout[] = [];
  let top = 0;
  for (const [laneIndex, lane] of lanes.entries()) {
    for (const kind of ["clips", "automation"] as const) {
      const visible = kind === "clips" || revealAutomation || occupied.has(lane.target.fixture);
      const height = visible ? rowHeightAt(rowHeights, lane.target, kind, kind === "clips" ? defaultMainRowHeight : defaultAutomationRowHeight) : 0;
      rows.push({ laneIndex, target: lane.target, kind, top, height, bottom: top + height });
      top += height;
    }
  }
  return rows;
}

export function expandedTimelineHeight(rows: SequenceRowLayout[]): number {
  return rows[rows.length - 1]?.bottom ?? 0;
}

export function rowFromCanvasY(y: number, top: number, scrollY: number, rows: SequenceRowLayout[]): SequenceRowLayout | null {
  const contentY = Math.max(0, y - top + scrollY);
  return rows.find((row) => contentY < row.bottom) ?? null;
}

export function laneIndexFromCanvasY(y: number, top: number, scrollY: number, laneCount: number, rows: SequenceRowLayout[]): number {
  return rowFromCanvasY(y, top, scrollY, rows)?.laneIndex ?? Math.max(0, laneCount - 1);
}

type ClipTiming = { id: number; startSeconds: number; durationSeconds: number };

/** A clip's edges at whole nanoseconds, so clips that touch in the model touch here. */
function clipSpan(timing: ClipTiming) {
  return { start: roundToNanosecond(timing.startSeconds), end: roundToNanosecond(timing.startSeconds + timing.durationSeconds) };
}

function compareClipsByTime(left: ClipTiming, right: ClipTiming) {
  const leftSpan = clipSpan(left);
  const rightSpan = clipSpan(right);
  return leftSpan.start - rightSpan.start || leftSpan.end - rightSpan.end || left.id - right.id;
}

/** A row's clips in runs that overlap in time; each run shares the row's height. */
export function groupOverlappingClips<T>(clips: T[], timing: (clip: T) => ClipTiming) {
  const sorted = [...clips].sort((left, right) => compareClipsByTime(timing(left), timing(right)));
  const groups: T[][] = [];
  let current: T[] = [];
  let currentEnd = -Infinity;
  for (const clip of sorted) {
    const { start, end } = clipSpan(timing(clip));
    if (current.length === 0 || start < currentEnd) {
      current.push(clip);
      currentEnd = Math.max(currentEnd, end);
    } else {
      groups.push(current);
      current = [clip];
      currentEnd = end;
    }
  }
  if (current.length > 0) groups.push(current);
  return groups;
}

/** The slot of each clip in an overlapping run: the first slot free at its start. */
export function assignOverlapSlots<T>(group: T[], timing: (clip: T) => ClipTiming) {
  const slotEnds: number[] = [];
  return [...group].sort((left, right) => compareClipsByTime(timing(left), timing(right))).map((clip) => {
    const { start, end } = clipSpan(timing(clip));
    let slot = slotEnds.findIndex((slotEnd) => slotEnd <= start);
    if (slot === -1) slot = slotEnds.length;
    slotEnds[slot] = end;
    return { ...clip, slot };
  });
}

/** Indices of lanes inside collapsed groups. Lanes are depth-first with their depth; collapsing
 * a group collapses every copy of it. */
export function collapsedLanes(lanes: SequenceLane[], collapsedGroups: ReadonlySet<number>): Set<number> {
  const hidden = new Set<number>();
  let collapsedDepth: number | null = null;
  for (const [index, lane] of lanes.entries()) {
    if (collapsedDepth !== null && lane.depth > collapsedDepth) {
      hidden.add(index);
      continue;
    }
    collapsedDepth = lane.kind === "group" && collapsedGroups.has(lane.target.fixture) ? lane.depth : null;
  }
  return hidden;
}

/** Rows of hidden lanes take no height; the rows below move up. */
export function collapseRows(rows: SequenceRowLayout[], hiddenLanes: ReadonlySet<number>): SequenceRowLayout[] {
  if (hiddenLanes.size === 0) return rows;
  let top = 0;
  return rows.map((row) => {
    const height = hiddenLanes.has(row.laneIndex) ? 0 : row.height;
    const collapsed = { ...row, top, height, bottom: top + height };
    top += height;
    return collapsed;
  });
}

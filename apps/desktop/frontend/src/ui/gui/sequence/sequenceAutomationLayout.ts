import type { FixtureTarget, SequenceLane, SequenceAutomationClip, PersistedSequenceViewportState } from "../../../editor/types";

import { clamp, roundToNanosecond } from "../shared";
import { THEME_COLORS, THEME_METRICS, THEME_TYPOGRAPHY } from "../../../theme";
import type { SequenceViewport } from "./sequenceSelection";

/** The content window is the time span the curve positions cover. A crop moves the clip window over fixed content. */
export type AutomationDraft = {
  id: number;
  startSeconds: number;
  durationSeconds: number;
  rowTarget: FixtureTarget;
  contentStartSeconds: number;
  contentDurationSeconds: number;
};

export type AutomationHover = { kind: "automation"; clipId: number; resize: "left" | "right" | "none" };

export type AutomationCurvePoint = { time: number; value: number };

/** Clip-relative time and normalized value the dragged point aligned to. */
export type AutomationGuide = { time: number | null; value: number | null };

export type AutomationCurveDraft = {
  id: number;
  curve: AutomationCurvePoint[];
  guide: AutomationGuide;
};

export type AutomationClipView = SequenceAutomationClip & { contentStartSeconds: number; contentDurationSeconds: number };

type CanvasRect = { x: number; y: number; width: number; height: number };

export type AutomationClipLayout = {
  clip: AutomationClipView;
  /** The lane drawing this copy; a target shared by several groups has several. */
  laneIndex: number;
  rect: CanvasRect;
  /** Canvas span of the content window; shares the clip's vertical extent. */
  curveRect: CanvasRect;
};

export type AutomationClipVisualState = {
  label: string;
  selected: boolean;
  hovered: boolean;
  choosing: boolean;
  resize: "left" | "right" | "none";
  activePointIndex: number | null;
  guide: AutomationGuide | null;
  markXs: number[];
};

export type SequenceRowKind = "effects" | "automation";
export type SequenceRowHeightMap = Record<number, Record<SequenceRowKind, number>>;

export type SequenceRowLayout = {
  laneIndex: number;
  target: FixtureTarget;
  kind: SequenceRowKind;
  top: number;
  height: number;
  bottom: number;
};

export function automationLaneRowHeight(laneHeight: number): number {
  return clamp(
    laneHeight * THEME_METRICS.automationRowHeightRatio,
    THEME_METRICS.automationRowMinHeight,
    THEME_METRICS.automationRowMaxHeight
  );
}

// The persisted suffix denotes row kind, never a visible row index.
function persistedRowHeightKey(fixture: number, kind: SequenceRowKind): string {
  return `${fixture}:row:${kind === "effects" ? 0 : 1}`;
}

export function persistRowHeights(heights: SequenceRowHeightMap): PersistedSequenceViewportState["rowHeights"] {
  return Object.fromEntries(Object.entries(heights).flatMap(([fixture, sizes]) =>
    (["effects", "automation"] as const).map((kind) => [persistedRowHeightKey(Number(fixture), kind), sizes[kind]])));
}

export function restoreRowHeights(lanes: SequenceLane[], persisted: PersistedSequenceViewportState["rowHeights"] | undefined, defaultHeight: number): SequenceRowHeightMap {
  return Object.fromEntries(lanes.map((lane) => [lane.target.fixture, {
    effects: clamp(persisted?.[persistedRowHeightKey(lane.target.fixture, "effects")] ?? defaultHeight, THEME_METRICS.sequenceMinLaneHeight, THEME_METRICS.sequenceMaxLaneHeight),
    automation: clamp(persisted?.[persistedRowHeightKey(lane.target.fixture, "automation")] ?? automationLaneRowHeight(defaultHeight), THEME_METRICS.sequenceMinLaneHeight, THEME_METRICS.sequenceMaxLaneHeight)
  }]));
}

export function rowHeightAt(rowHeights: SequenceRowHeightMap, target: FixtureTarget, kind: SequenceRowKind, defaultHeight: number): number {
  return rowHeights[target.fixture]?.[kind] ?? defaultHeight;
}

export function sequenceRowLayout(lanes: SequenceLane[], clips: SequenceAutomationClip[], rowHeights: SequenceRowHeightMap, defaultMainRowHeight: number, defaultAutomationRowHeight: number, revealAutomation: boolean): SequenceRowLayout[] {
  const occupied = new Set(clips.map((clip) => clip.rowTarget.fixture));
  const rows: SequenceRowLayout[] = [];
  let top = 0;
  for (const [laneIndex, lane] of lanes.entries()) {
    for (const kind of ["effects", "automation"] as const) {
      const visible = kind === "effects" || revealAutomation || occupied.has(lane.target.fixture);
      const height = visible ? rowHeightAt(rowHeights, lane.target, kind, kind === "effects" ? defaultMainRowHeight : defaultAutomationRowHeight) : 0;
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

export function buildAutomationClipLayout(clips: AutomationClipView[], rows: SequenceRowLayout[], viewport: SequenceViewport, left: number, top: number, bounds: { width: number; height: number }): AutomationClipLayout[] {
  const visibleStartSeconds = viewport.scrollXSeconds;
  const visibleEndSeconds = viewport.scrollXSeconds + Math.max(1, bounds.width - left) / viewport.pxPerSecond;
  const byAutomationLane = new Map<number, AutomationClipView[]>();
  for (const clip of clips) {
    if (clip.startSeconds + clip.durationSeconds < visibleStartSeconds || clip.startSeconds > visibleEndSeconds) continue;
    const key = clip.rowTarget.fixture;
    const laneClips = byAutomationLane.get(key) ?? [];
    laneClips.push(clip);
    byAutomationLane.set(key, laneClips);
  }

  const layouts: AutomationClipLayout[] = [];
  for (const [target, laneClips] of byAutomationLane) {
    const targetRows = rows.filter((row) => row.target.fixture === target && row.kind === "automation");
    if (targetRows.length === 0) throw new Error("Automation clip has no timeline row.");
    for (const row of targetRows) for (const group of groupOverlappingClips(laneClips)) {
      const assigned = assignOverlapSlots(group);
      const slotCount = Math.max(1, Math.max(...assigned.map((clip) => clip.slot)) + 1);
      for (const clip of assigned) {
        const slotHeight = row.height / slotCount;
        const x = left + (clip.startSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
        const rect = {
          x,
          y: top + row.top - viewport.scrollY + clip.slot * slotHeight + THEME_METRICS.sequenceClipSlotOffset,
          width: Math.max(THEME_METRICS.sequenceClipMinWidth, clip.durationSeconds * viewport.pxPerSecond),
          height: Math.max(THEME_METRICS.sequenceClipMinHeight, slotHeight - THEME_METRICS.sequenceClipHandleInset)
        };
        layouts.push({
          clip,
          laneIndex: row.laneIndex,
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

export function automationClipsWithDrafts(clips: SequenceAutomationClip[], drafts: AutomationDraft[], curveDraft: AutomationCurveDraft | null): AutomationClipView[] {
  const byId = new Map(drafts.map((draft) => [draft.id, draft]));
  return clips.map((clip) => {
    const draft = byId.get(clip.id);
    return {
      ...clip,
      contentStartSeconds: clip.startSeconds,
      contentDurationSeconds: clip.durationSeconds,
      ...draft,
      curve: curveDraft?.id === clip.id ? curveDraft.curve : clip.curve
    };
  });
}

export function automationHoverEqual(left: AutomationHover | null, right: AutomationHover | null) {
  if (left === right) return true;
  if (left === null || right === null) return false;
  return left.clipId === right.clipId && left.resize === right.resize;
}

type TimedClip = { id: number; startSeconds: number; durationSeconds: number };

function compareAutomationClipsByTime(left: TimedClip, right: TimedClip) {
  return left.startSeconds - right.startSeconds || left.startSeconds + left.durationSeconds - (right.startSeconds + right.durationSeconds) || left.id - right.id;
}

export function groupOverlappingClips<T extends TimedClip>(clips: T[]) {
  const sorted = [...clips].sort(compareAutomationClipsByTime);
  const groups: T[][] = [];
  let current: T[] = [];
  let currentEnd = -Infinity;
  for (const clip of sorted) {
    const end = clip.startSeconds + clip.durationSeconds;
    if (current.length === 0 || clip.startSeconds < currentEnd) {
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

export function assignOverlapSlots<T extends TimedClip>(group: T[]) {
  const slotEnds: number[] = [];
  return [...group].sort(compareAutomationClipsByTime).map((clip) => {
    const start = clip.startSeconds;
    const end = clip.startSeconds + clip.durationSeconds;
    let slot = slotEnds.findIndex((slotEnd) => slotEnd <= start);
    if (slot === -1) slot = slotEnds.length;
    slotEnds[slot] = end;
    return { ...clip, slot };
  });
}

export function hitTimelineClip<T extends { rect: AutomationClipLayout["rect"] }>(clips: T[], x: number, y: number) {
  for (const clip of [...clips].reverse()) {
    const { rect } = clip;
    if (x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height) {
      const resize: "left" | "right" | "none" = x - rect.x < THEME_METRICS.sequenceEffectResizeHitWidth ? "left" : rect.x + rect.width - x < THEME_METRICS.sequenceEffectResizeHitWidth ? "right" : "none";
      return { ...clip, resize };
    }
  }
  return null;
}

/** The curve spans the full clip width so curve time matches timeline time. */
function automationCurveGraphRect(rect: CanvasRect) {
  const padding = Math.min(THEME_METRICS.automationGraphPaddingMax, Math.max(THEME_METRICS.automationGraphPaddingMin, rect.height * THEME_METRICS.automationGraphPaddingRatio));
  const headerHeight = Math.min(THEME_METRICS.automationClipHeaderHeight, rect.height);
  return {
    x: rect.x,
    y: rect.y + headerHeight + padding,
    width: Math.max(THEME_METRICS.visualMinSize, rect.width),
    height: Math.max(THEME_METRICS.visualMinSize, rect.height - headerHeight - padding * 2)
  };
}

function automationCurveCanvasPoints(curve: AutomationCurvePoint[], curveRect: CanvasRect) {
  const graph = automationCurveGraphRect(curveRect);
  return curve.map((point) => ({ x: graph.x + clamp(point.time, 0, 1) * graph.width, y: graph.y + (1 - clamp(point.value, 0, 1)) * graph.height }));
}

export function hitAutomationCurvePoint(clip: AutomationClipLayout, x: number, y: number): number | null {
  const points = automationCurveCanvasPoints(clip.clip.curve, clip.curveRect);
  for (let index = points.length - 1; index >= 0; index -= 1) {
    const point = points[index];
    if (point !== undefined && Math.hypot(point.x - x, point.y - y) <= THEME_METRICS.automationHitRadius) return index;
  }
  return null;
}

/** The point on the drawn line under the cursor, and the curve index it would be inserted at. */
export function hitAutomationCurveLine(clip: AutomationClipLayout, x: number, y: number): { index: number; point: AutomationCurvePoint } | null {
  const line = automationCurveDisplayPoints(automationCurveCanvasPoints(clip.clip.curve, clip.curveRect), clip.rect);
  // Segment i runs from the point before curve index i to curve index i.
  for (let index = 0; index < line.length - 1; index += 1) {
    const start = line[index];
    const end = line[index + 1];
    if (start === undefined || end === undefined) continue;
    if (distanceToSegment(x, y, start, end) > THEME_METRICS.automationHitRadius) continue;
    const vertical = Math.abs(end.x - start.x) < THEME_METRICS.visualLineWidth;
    const lineY = vertical ? y : start.y + ((end.y - start.y) * (x - start.x)) / (end.x - start.x);
    return { index, point: automationCurvePointFromCanvas(clip.curveRect, x, lineY) };
  }
  return null;
}

function distanceToSegment(x: number, y: number, start: { x: number; y: number }, end: { x: number; y: number }) {
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const lengthSquared = dx * dx + dy * dy;
  const amount = lengthSquared === 0 ? 0 : clamp(((x - start.x) * dx + (y - start.y) * dy) / lengthSquared, 0, 1);
  return Math.hypot(x - (start.x + amount * dx), y - (start.y + amount * dy));
}

export function automationCurvePointFromCanvas(curveRect: CanvasRect, x: number, y: number): AutomationCurvePoint {
  const graph = automationCurveGraphRect(curveRect);
  return { time: roundToNanosecond(clamp((x - graph.x) / graph.width, 0, 1)), value: Math.round(clamp(1 - (y - graph.y) / graph.height, 0, 1) * 1000) / 1000 };
}

/** Points keep their order: a moved point stays between its neighbors, so stacked steps never swap. */
export function moveAutomationCurvePoint(curve: AutomationCurvePoint[], index: number, point: AutomationCurvePoint) {
  const time = clamp(point.time, curve[index - 1]?.time ?? 0, curve[index + 1]?.time ?? 1);
  return curve.map((candidate, candidateIndex) => candidateIndex === index ? { time, value: point.value } : candidate);
}

export function insertAutomationCurvePoint(curve: AutomationCurvePoint[], index: number, point: AutomationCurvePoint) {
  const time = clamp(point.time, curve[index - 1]?.time ?? 0, curve[index]?.time ?? 1);
  return [...curve.slice(0, index), { time, value: point.value }, ...curve.slice(index)];
}

export function removeAutomationCurvePoint(curve: AutomationCurvePoint[], index: number) {
  return curve.filter((_, candidateIndex) => candidateIndex !== index);
}

/** Align the dragged point's time or value with a neighbor that is close on screen. */
export function alignAutomationCurvePoint(curve: AutomationCurvePoint[], index: number, point: AutomationCurvePoint, curveRect: CanvasRect): { point: AutomationCurvePoint; guide: AutomationGuide } {
  const graph = automationCurveGraphRect(curveRect);
  const neighbors = [curve[index - 1], curve[index + 1]].filter((neighbor): neighbor is AutomationCurvePoint => neighbor !== undefined);
  const nearest = (axis: "time" | "value", scale: number) => {
    const candidate = neighbors.reduce<AutomationCurvePoint | null>((best, neighbor) => best === null || Math.abs(neighbor[axis] - point[axis]) < Math.abs(best[axis] - point[axis]) ? neighbor : best, null);
    return candidate !== null && Math.abs(candidate[axis] - point[axis]) * scale <= THEME_METRICS.automationAlignDistance ? candidate[axis] : null;
  };
  const time = nearest("time", graph.width);
  const value = nearest("value", graph.height);
  return { point: { time: time ?? point.time, value: value ?? point.value }, guide: { time, value } };
}

export function drawAutomationClip(
  ctx: CanvasRenderingContext2D,
  layout: AutomationClipLayout,
  state: AutomationClipVisualState
) {
  const { clip, rect } = layout;
  const graph = automationCurveGraphRect(rect);
  const curveGraph = automationCurveGraphRect(layout.curveRect);
  const headerHeight = Math.min(THEME_METRICS.automationClipHeaderHeight, rect.height);
  const radius = Math.min(THEME_METRICS.automationClipRadius, rect.width / 2, rect.height / 2);

  ctx.save();
  ctx.beginPath();
  ctx.roundRect(rect.x, rect.y, rect.width, rect.height, radius);
  ctx.clip();

  ctx.fillStyle = THEME_COLORS.automationClipSurface;
  ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
  ctx.fillStyle = state.selected ? THEME_COLORS.automationClipHeaderSelected : THEME_COLORS.automationClipHeader;
  ctx.fillRect(rect.x, rect.y, rect.width, headerHeight);
  ctx.fillStyle = THEME_COLORS.automationGraph;
  ctx.fillRect(rect.x, rect.y + headerHeight, rect.width, Math.max(0, rect.height - headerHeight));

  ctx.lineWidth = THEME_METRICS.visualLineWidth;
  ctx.strokeStyle = THEME_COLORS.automationGraphGrid;
  ctx.beginPath();
  for (const markX of state.markXs) {
    if (markX <= rect.x || markX >= rect.x + rect.width) continue;
    const x = Math.round(markX) + THEME_METRICS.visualHairlineOffset;
    ctx.moveTo(x, graph.y); ctx.lineTo(x, graph.y + graph.height);
  }
  for (let row = 1; row < THEME_METRICS.automationGridRows; row += 1) {
    const y = graph.y + (graph.height * row) / THEME_METRICS.automationGridRows + THEME_METRICS.visualHairlineOffset;
    ctx.moveTo(graph.x, y); ctx.lineTo(graph.x + graph.width, y);
  }
  ctx.stroke();
  ctx.strokeStyle = THEME_COLORS.automationGraphMajorGrid;
  ctx.beginPath();
  ctx.moveTo(graph.x, graph.y + graph.height / 2 + THEME_METRICS.visualHairlineOffset);
  ctx.lineTo(graph.x + graph.width, graph.y + graph.height / 2 + THEME_METRICS.visualHairlineOffset);
  ctx.stroke();

  const points = automationCurveCanvasPoints(clip.curve, layout.curveRect);
  if (points.length > 0) {
    const displayPoints = automationCurveDisplayPoints(points, rect);
    ctx.beginPath();
    ctx.moveTo(displayPoints[0]?.x ?? graph.x, graph.y + graph.height);
    for (const point of displayPoints) ctx.lineTo(point.x, point.y);
    ctx.lineTo(displayPoints[displayPoints.length - 1]?.x ?? graph.x + graph.width, graph.y + graph.height);
    ctx.closePath();
    ctx.fillStyle = THEME_COLORS.automationCurveFill;
    ctx.fill();

    if (state.guide !== null && state.activePointIndex !== null) {
      ctx.setLineDash([THEME_METRICS.automationGuideDash]);
      ctx.strokeStyle = THEME_COLORS.automation;
      ctx.lineWidth = THEME_METRICS.visualLineWidth;
      ctx.beginPath();
      if (state.guide.time !== null) {
        const x = curveGraph.x + state.guide.time * curveGraph.width;
        ctx.moveTo(x, graph.y); ctx.lineTo(x, graph.y + graph.height);
      }
      if (state.guide.value !== null) {
        const y = curveGraph.y + (1 - state.guide.value) * curveGraph.height;
        ctx.moveTo(rect.x, y); ctx.lineTo(rect.x + rect.width, y);
      }
      ctx.stroke();
      ctx.setLineDash([]);
    }

    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    drawAutomationLine(ctx, displayPoints, THEME_COLORS.automationCurveShadow, THEME_METRICS.automationCurveShadowWidth);
    drawAutomationLine(ctx, displayPoints, THEME_COLORS.automation, THEME_METRICS.automationCurveWidth);

    if (state.selected || state.hovered) {
      points.forEach((point, index) => {
        const active = state.activePointIndex === index;
        ctx.fillStyle = THEME_COLORS.automationPointFill;
        ctx.strokeStyle = active ? THEME_COLORS.automation : THEME_COLORS.automationPointStroke;
        ctx.lineWidth = active ? THEME_METRICS.visualLineWidthStrong : THEME_METRICS.visualLineWidth;
        ctx.beginPath();
        ctx.arc(point.x, point.y, active ? THEME_METRICS.automationPointRadiusSelected : THEME_METRICS.automationPointRadius, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      });
    }
    ctx.lineCap = "butt";
    ctx.lineJoin = "miter";

    const activePoint = state.activePointIndex === null ? undefined : points[state.activePointIndex];
    const activeValue = state.activePointIndex === null ? undefined : clip.curve[state.activePointIndex]?.value;
    if (activePoint !== undefined && activeValue !== undefined) {
      const label = `${Math.round(activeValue * 100)}%`;
      ctx.font = THEME_TYPOGRAPHY.canvasLabel;
      ctx.fillStyle = THEME_COLORS.automationClipLabel;
      ctx.textBaseline = "middle";
      const width = ctx.measureText(label).width;
      const rightX = activePoint.x + THEME_METRICS.automationValueLabelOffset;
      ctx.fillText(label, rightX + width > rect.x + rect.width ? activePoint.x - THEME_METRICS.automationValueLabelOffset - width : rightX, activePoint.y);
    }
  }

  ctx.strokeStyle = THEME_COLORS.automationGraphMajorGrid;
  ctx.lineWidth = THEME_METRICS.visualLineWidth;
  ctx.beginPath();
  ctx.moveTo(rect.x, rect.y + headerHeight + THEME_METRICS.visualHairlineOffset);
  ctx.lineTo(rect.x + rect.width, rect.y + headerHeight + THEME_METRICS.visualHairlineOffset);
  ctx.stroke();

  if (state.choosing) {
    ctx.fillStyle = THEME_COLORS.accentSubtle;
    ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
  }
  if (clip.detachedBindings.length > 0) {
    ctx.fillStyle = THEME_COLORS.automationDetached;
    ctx.fillRect(rect.x, rect.y, THEME_METRICS.automationDetachedStripeWidth, rect.height);
  }

  if (rect.width >= THEME_METRICS.automationClipLabelMinWidth) {
    ctx.font = THEME_TYPOGRAPHY.canvasLabel;
    ctx.fillStyle = clip.bindings.length > 0 ? THEME_COLORS.automationClipLabel : THEME_COLORS.automationClipLabelMuted;
    ctx.textBaseline = "alphabetic";
    const labelX = rect.x + THEME_METRICS.automationClipLabelInset + (clip.detachedBindings.length > 0 ? THEME_METRICS.automationDetachedStripeWidth : 0);
    const labelWidth = Math.max(0, rect.x + rect.width - THEME_METRICS.automationClipLabelInset - labelX);
    ctx.fillText(fitCanvasLabel(ctx, state.label, labelWidth), labelX, rect.y + THEME_METRICS.automationClipLabelBaseline);
  }

  ctx.restore();

  ctx.strokeStyle = state.choosing
    ? THEME_COLORS.accent
    : state.selected
      ? THEME_COLORS.clipSelected
      : state.hovered
        ? THEME_COLORS.clipHover
        : THEME_COLORS.clipBorder;
  ctx.lineWidth = state.choosing || state.selected || state.hovered
    ? THEME_METRICS.visualLineWidthStrong
    : THEME_METRICS.visualLineWidth;
  ctx.beginPath();
  ctx.roundRect(
    rect.x + THEME_METRICS.visualHairlineOffset,
    rect.y + THEME_METRICS.visualHairlineOffset,
    Math.max(0, rect.width - THEME_METRICS.visualLineWidth),
    Math.max(0, rect.height - THEME_METRICS.visualLineWidth),
    radius
  );
  ctx.stroke();

  if (!state.choosing && (state.resize === "left" || state.resize === "right")) {
    const handleX = state.resize === "left" ? rect.x : rect.x + rect.width;
    ctx.fillStyle = THEME_COLORS.automation;
    ctx.fillRect(
      handleX - THEME_METRICS.sequenceClipHandleHalfWidth,
      rect.y + THEME_METRICS.sequenceClipHandleInset,
      THEME_METRICS.sequenceClipHandleHalfWidth * 2,
      Math.max(THEME_METRICS.sequenceClipHandleHeight, rect.height - THEME_METRICS.sequenceClipHandleInset * 2)
    );
  }
}

/** The authored curve holds its first and last values out to the clip edges. */
function automationCurveDisplayPoints(points: Array<{ x: number; y: number }>, rect: CanvasRect) {
  const first = points[0];
  const last = points[points.length - 1];
  if (first === undefined || last === undefined) return [];
  return [
    { x: Math.min(rect.x, first.x), y: first.y },
    ...points,
    { x: Math.max(rect.x + rect.width, last.x), y: last.y }
  ];
}

function drawAutomationLine(
  ctx: CanvasRenderingContext2D,
  points: Array<{ x: number; y: number }>,
  color: string,
  width: number
) {
  ctx.strokeStyle = color;
  ctx.lineWidth = width;
  ctx.beginPath();
  points.forEach((point, index) => {
    if (index === 0) ctx.moveTo(point.x, point.y);
    else ctx.lineTo(point.x, point.y);
  });
  ctx.stroke();
}

export function fitCanvasLabel(ctx: CanvasRenderingContext2D, label: string, maxWidth: number) {
  if (maxWidth <= 0 || ctx.measureText(label).width <= maxWidth) return label;
  const ellipsis = "...";
  let fitted = label;
  while (fitted.length > 0 && ctx.measureText(`${fitted}${ellipsis}`).width > maxWidth) {
    fitted = fitted.slice(0, -1);
  }
  return fitted.length > 0 ? `${fitted}${ellipsis}` : ellipsis;
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

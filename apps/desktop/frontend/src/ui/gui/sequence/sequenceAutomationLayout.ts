import type { FixtureTarget, SequenceLane, SequenceAutomationClip, PersistedSequenceViewportState } from "../../../editor/types";

import { clamp, roundToNanosecond } from "../shared";
import { THEME_COLORS, THEME_METRICS, THEME_TYPOGRAPHY } from "../../../theme";
import type { SequenceViewport } from "./sequenceSelection";

export type AutomationDraft = {
  id: number;
  startSeconds: number;
  durationSeconds: number;
  rowTarget: FixtureTarget;
};

export type AutomationHover = { kind: "automation"; clipId: number; resize: "left" | "right" | "none" };

export type AutomationCurveDraft = {
  id: number;
  curve: Array<{ time: number; value: number }>;
};

export type AutomationClipLayout = {
  clip: SequenceAutomationClip;
  rect: { x: number; y: number; width: number; height: number };
};

export type AutomationClipVisualState = {
  label: string;
  selected: boolean;
  hovered: boolean;
  choosing: boolean;
  resize: "left" | "right" | "none";
  activePointIndex: number | null;
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

export function buildAutomationClipLayout(clips: SequenceAutomationClip[], rows: SequenceRowLayout[], viewport: SequenceViewport, left: number, top: number, bounds: { width: number; height: number }): AutomationClipLayout[] {
  const visibleStartSeconds = viewport.scrollXSeconds;
  const visibleEndSeconds = viewport.scrollXSeconds + Math.max(1, bounds.width - left) / viewport.pxPerSecond;
  const byAutomationLane = new Map<number, SequenceAutomationClip[]>();
  for (const clip of clips) {
    if (clip.startSeconds + clip.durationSeconds < visibleStartSeconds || clip.startSeconds > visibleEndSeconds) continue;
    const key = clip.rowTarget.fixture;
    const laneClips = byAutomationLane.get(key) ?? [];
    laneClips.push(clip);
    byAutomationLane.set(key, laneClips);
  }

  const layouts: AutomationClipLayout[] = [];
  for (const laneClips of byAutomationLane.values()) {
    const first = laneClips[0];
    if (first === undefined) continue;
    const row = rows.find((row) => row.target.fixture === first.rowTarget.fixture && row.kind === "automation");
    if (row === undefined) throw new Error("Automation clip has no timeline row.");
    for (const group of groupOverlappingClips(laneClips)) {
      const assigned = assignOverlapSlots(group);
      const slotCount = Math.max(1, Math.max(...assigned.map((clip) => clip.slot)) + 1);
      for (const clip of assigned) {
        const slotHeight = row.height / slotCount;
        const x = left + (clip.startSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
        layouts.push({
          clip,
          rect: {
            x,
            y: top + row.top - viewport.scrollY + clip.slot * slotHeight + THEME_METRICS.sequenceClipSlotOffset,
            width: Math.max(THEME_METRICS.sequenceClipMinWidth, clip.durationSeconds * viewport.pxPerSecond),
            height: Math.max(THEME_METRICS.sequenceClipMinHeight, slotHeight - THEME_METRICS.sequenceClipHandleInset)
          }
        });
      }
    }
  }
  return layouts;
}

export function automationClipsWithDrafts(clips: SequenceAutomationClip[], drafts: AutomationDraft[], curveDraft: AutomationCurveDraft | null): SequenceAutomationClip[] {
  const byId = new Map(drafts.map((draft) => [draft.id, draft]));
  return clips.map((clip) => {
    const draft = byId.get(clip.id);
    return { ...clip, ...draft, curve: curveDraft?.id === clip.id ? curveDraft.curve : clip.curve };
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

function automationCurveGraphRect(rect: { x: number; y: number; width: number; height: number }) {
  const padding = Math.min(THEME_METRICS.automationGraphPaddingMax, Math.max(THEME_METRICS.automationGraphPaddingMin, rect.height * THEME_METRICS.automationGraphPaddingRatio));
  const headerHeight = Math.min(THEME_METRICS.automationClipHeaderHeight, rect.height);
  return {
    x: rect.x + padding,
    y: rect.y + headerHeight + padding,
    width: Math.max(THEME_METRICS.visualMinSize, rect.width - padding * 2),
    height: Math.max(THEME_METRICS.visualMinSize, rect.height - headerHeight - padding * 2)
  };
}

export function sortAutomationCurve(curve: Array<{ time: number; value: number }>) {
  return [...curve].sort((left, right) => left.time - right.time);
}

function automationCurveCanvasPoints(curve: Array<{ time: number; value: number }>, rect: { x: number; y: number; width: number; height: number }) {
  const graph = automationCurveGraphRect(rect);
  return sortAutomationCurve(curve).map((point) => ({ x: graph.x + clamp(point.time, 0, 1) * graph.width, y: graph.y + (1 - clamp(point.value, 0, 1)) * graph.height }));
}

export function hitAutomationCurvePoint(clip: AutomationClipLayout, x: number, y: number): number | null {
  const points = automationCurveCanvasPoints(clip.clip.curve, clip.rect);
  for (let index = points.length - 1; index >= 0; index -= 1) {
    const point = points[index];
    if (point !== undefined && Math.hypot(point.x - x, point.y - y) <= 7) return index;
  }
  return null;
}

export function automationCurvePointFromCanvas(rect: { x: number; y: number; width: number; height: number }, x: number, y: number) {
  const graph = automationCurveGraphRect(rect);
  return { time: roundToNanosecond(clamp((x - graph.x) / graph.width, 0, 1)), value: Math.round(clamp(1 - (y - graph.y) / graph.height, 0, 1) * 1000) / 1000 };
}

export function replaceAutomationCurvePointByIdentity(
  curve: Array<{ time: number; value: number }>,
  identity: { pointTime: number; pointValue: number; pointOccurrence: number },
  point: { time: number; value: number }
) {
  let occurrence = 0;
  return curve
    .map((candidate) => {
      const matches = candidate.time === identity.pointTime && candidate.value === identity.pointValue;
      const isTarget = matches && occurrence++ === identity.pointOccurrence;
      return isTarget ? point : candidate;
    })
    .filter((candidate) => Number.isFinite(candidate.time) && Number.isFinite(candidate.value))
    .sort((left, right) => left.time - right.time);
}

export function removeAutomationCurvePoint(curve: Array<{ time: number; value: number }>, index: number) {
  return sortAutomationCurve(curve).filter((_, candidateIndex) => candidateIndex !== index).filter((candidate) => Number.isFinite(candidate.time) && Number.isFinite(candidate.value));
}

export function drawAutomationClip(
  ctx: CanvasRenderingContext2D,
  clip: SequenceAutomationClip,
  rect: { x: number; y: number; width: number; height: number },
  state: AutomationClipVisualState
) {
  const graph = automationCurveGraphRect(rect);
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
  for (let column = 1; column < THEME_METRICS.automationGridColumns; column += 1) {
    const x = graph.x + (graph.width * column) / THEME_METRICS.automationGridColumns + THEME_METRICS.visualHairlineOffset;
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
  ctx.moveTo(graph.x + graph.width / 2 + THEME_METRICS.visualHairlineOffset, graph.y);
  ctx.lineTo(graph.x + graph.width / 2 + THEME_METRICS.visualHairlineOffset, graph.y + graph.height);
  ctx.stroke();

  const points = automationCurveCanvasPoints(clip.curve, rect);
  if (points.length > 0) {
    const displayPoints = automationCurveDisplayPoints(points, graph);
    ctx.beginPath();
    ctx.moveTo(displayPoints[0]?.x ?? graph.x, graph.y + graph.height);
    for (const point of displayPoints) ctx.lineTo(point.x, point.y);
    ctx.lineTo(displayPoints[displayPoints.length - 1]?.x ?? graph.x + graph.width, graph.y + graph.height);
    ctx.closePath();
    ctx.fillStyle = THEME_COLORS.automationCurveFill;
    ctx.fill();

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

function automationCurveDisplayPoints(
  points: Array<{ x: number; y: number }>,
  graph: { x: number; y: number; width: number; height: number }
) {
  const first = points[0];
  const last = points[points.length - 1];
  if (first === undefined || last === undefined) return [];
  return [
    { x: graph.x, y: first.y },
    ...points,
    { x: graph.x + graph.width, y: last.y }
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

/** Fixture ids of lanes inside collapsed groups. Lanes are depth-first with their depth. */
export function collapsedLaneTargets(lanes: SequenceLane[], collapsedGroups: ReadonlySet<number>): Set<number> {
  const hidden = new Set<number>();
  let collapsedDepth: number | null = null;
  for (const lane of lanes) {
    if (collapsedDepth !== null && lane.depth > collapsedDepth) {
      hidden.add(lane.target.fixture);
      continue;
    }
    collapsedDepth = lane.kind === "group" && collapsedGroups.has(lane.target.fixture) ? lane.depth : null;
  }
  return hidden;
}

/** Rows of hidden lanes take no height; the rows below move up. */
export function collapseRows(rows: SequenceRowLayout[], hiddenTargets: ReadonlySet<number>): SequenceRowLayout[] {
  if (hiddenTargets.size === 0) return rows;
  let top = 0;
  return rows.map((row) => {
    const height = hiddenTargets.has(row.target.fixture) ? 0 : row.height;
    const collapsed = { ...row, top, height, bottom: top + height };
    top += height;
    return collapsed;
  });
}

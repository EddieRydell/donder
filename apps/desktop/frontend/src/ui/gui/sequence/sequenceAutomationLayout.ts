import type { SequenceCurvePoint } from "../../../types";

import { clamp, envelopeOf, roundToNanosecond } from "../shared";
import { THEME_COLORS, THEME_METRICS, THEME_TYPOGRAPHY } from "../../../theme";
import type { SequenceClipLayout } from "./sequenceSelection";

/** An envelope's curve while it is edited, by clip id. */
export type AutomationCurveDraft = {
  id: number;
  curve: SequenceCurvePoint[];
  guide: AutomationGuide;
};

/** Clip-relative time and normalized value the dragged point aligned to. */
export type AutomationGuide = { time: number | null; value: number | null };

type CanvasRect = { x: number; y: number; width: number; height: number };

export type EnvelopeVisualState = {
  label: string;
  selected: boolean;
  hovered: boolean;
  choosing: boolean;
  resize: "left" | "right" | "none";
  activePointIndex: number | null;
  guide: AutomationGuide | null;
  markXs: number[];
};

function curveOf(layout: SequenceClipLayout): SequenceCurvePoint[] {
  return envelopeOf(layout.clip)?.curve ?? [];
}

/** The curve spans the full clip width so curve time matches timeline time. */
function automationCurveGraphRect(rect: CanvasRect) {
  const padding = Math.min(THEME_METRICS.automationGraphPaddingMax, Math.max(THEME_METRICS.automationGraphPaddingMin, rect.height * THEME_METRICS.automationGraphPaddingRatio));
  const headerHeight = Math.min(THEME_METRICS.envelopeHeaderHeight, rect.height);
  return {
    x: rect.x,
    y: rect.y + headerHeight + padding,
    width: Math.max(THEME_METRICS.visualMinSize, rect.width),
    height: Math.max(THEME_METRICS.visualMinSize, rect.height - headerHeight - padding * 2)
  };
}

function automationCurveCanvasPoints(curve: SequenceCurvePoint[], curveRect: CanvasRect) {
  const graph = automationCurveGraphRect(curveRect);
  return curve.map((point) => ({ x: graph.x + clamp(point.time, 0, 1) * graph.width, y: graph.y + (1 - clamp(point.value, 0, 1)) * graph.height }));
}

function hitAutomationCurvePointAt(clip: SequenceClipLayout, index: number, x: number, y: number): boolean {
  const point = automationCurveCanvasPoints(curveOf(clip), clip.curveRect)[index];
  return point !== undefined && Math.hypot(point.x - x, point.y - y) <= THEME_METRICS.automationHitRadius;
}

export function hitAutomationCurvePoint(clip: SequenceClipLayout, x: number, y: number): number | null {
  const points = automationCurveCanvasPoints(curveOf(clip), clip.curveRect);
  for (let index = points.length - 1; index >= 0; index -= 1) {
    const point = points[index];
    if (point !== undefined && Math.hypot(point.x - x, point.y - y) <= THEME_METRICS.automationHitRadius) return index;
  }
  return null;
}

/** The curve point under the cursor in any envelope, found before the envelopes themselves so a point on a clip's edge is grabbed, not the edge. */
export function hitAutomationPoint<T extends SequenceClipLayout>(clips: T[], x: number, y: number): { layout: T; index: number } | null {
  for (let clipIndex = clips.length - 1; clipIndex >= 0; clipIndex -= 1) {
    const layout = clips[clipIndex];
    if (layout === undefined) continue;
    const index = hitAutomationCurvePoint(layout, x, y);
    if (index !== null) return { layout, index };
  }
  return null;
}

export function automationCurvePointFromCanvas(curveRect: CanvasRect, x: number, y: number): SequenceCurvePoint {
  const graph = automationCurveGraphRect(curveRect);
  return { time: roundToNanosecond(clamp((x - graph.x) / graph.width, 0, 1)), value: Math.round(clamp(1 - (y - graph.y) / graph.height, 0, 1) * 1000) / 1000 };
}

/** Points keep their order: a moved point stays between its neighbors, so stacked steps never swap. */
export function moveAutomationCurvePoint(curve: SequenceCurvePoint[], index: number, point: SequenceCurvePoint) {
  const time = clamp(point.time, curve[index - 1]?.time ?? 0, curve[index + 1]?.time ?? 1);
  return curve.map((candidate, candidateIndex) => candidateIndex === index ? { time, value: point.value } : candidate);
}

/** A new point lands where it was clicked, after every point at or before its time. */
export function insertAutomationCurvePoint(curve: SequenceCurvePoint[], point: SequenceCurvePoint): { curve: SequenceCurvePoint[]; index: number } {
  const after = curve.findIndex((candidate) => candidate.time > point.time);
  const index = after === -1 ? curve.length : after;
  return { curve: [...curve.slice(0, index), point, ...curve.slice(index)], index };
}

/** The curve without the points under the cursor, as a right-drag sweeps them away; a curve keeps one point. */
export function eraseAutomationCurvePoints(curve: SequenceCurvePoint[], clip: SequenceClipLayout, x: number, y: number): SequenceCurvePoint[] {
  const kept = curve.filter((_, index) => !hitAutomationCurvePointAt(clip, index, x, y));
  return kept.length === 0 ? curve.slice(0, 1) : kept;
}

export function removeAutomationCurvePoint(curve: SequenceCurvePoint[], index: number) {
  return curve.filter((_, candidateIndex) => candidateIndex !== index);
}

/** Align the dragged point's time or value with a neighbor that is close on screen. */
export function alignAutomationCurvePoint(curve: SequenceCurvePoint[], index: number, point: SequenceCurvePoint, curveRect: CanvasRect): { point: SequenceCurvePoint; guide: AutomationGuide } {
  const graph = automationCurveGraphRect(curveRect);
  const neighbors = [curve[index - 1], curve[index + 1]].filter((neighbor): neighbor is SequenceCurvePoint => neighbor !== undefined);
  const nearest = (axis: "time" | "value", scale: number) => {
    const candidate = neighbors.reduce<SequenceCurvePoint | null>((best, neighbor) => best === null || Math.abs(neighbor[axis] - point[axis]) < Math.abs(best[axis] - point[axis]) ? neighbor : best, null);
    return candidate !== null && Math.abs(candidate[axis] - point[axis]) * scale <= THEME_METRICS.automationAlignDistance ? candidate[axis] : null;
  };
  const time = nearest("time", graph.width);
  const value = nearest("value", graph.height);
  return { point: { time: time ?? point.time, value: value ?? point.value }, guide: { time, value } };
}

export function drawEnvelope(
  ctx: CanvasRenderingContext2D,
  layout: SequenceClipLayout,
  state: EnvelopeVisualState
) {
  const { rect } = layout;
  const curve = curveOf(layout);
  const bindings = envelopeOf(layout.clip)?.bindings ?? [];
  const graph = automationCurveGraphRect(rect);
  const curveGraph = automationCurveGraphRect(layout.curveRect);
  const headerHeight = Math.min(THEME_METRICS.envelopeHeaderHeight, rect.height);
  const radius = Math.min(THEME_METRICS.envelopeRadius, rect.width / 2, rect.height / 2);

  ctx.save();
  ctx.beginPath();
  ctx.roundRect(rect.x, rect.y, rect.width, rect.height, radius);
  ctx.clip();

  ctx.fillStyle = THEME_COLORS.envelopeSurface;
  ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
  ctx.fillStyle = state.selected ? THEME_COLORS.envelopeHeaderSelected : THEME_COLORS.envelopeHeader;
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

  const points = automationCurveCanvasPoints(curve, layout.curveRect);
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
    const activeValue = state.activePointIndex === null ? undefined : curve[state.activePointIndex]?.value;
    if (activePoint !== undefined && activeValue !== undefined) {
      const label = `${Math.round(activeValue * 100)}%`;
      ctx.font = THEME_TYPOGRAPHY.canvasLabel;
      ctx.fillStyle = THEME_COLORS.envelopeLabel;
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

  if (rect.width >= THEME_METRICS.envelopeLabelMinWidth) {
    ctx.font = THEME_TYPOGRAPHY.canvasLabel;
    ctx.fillStyle = bindings.length > 0 ? THEME_COLORS.envelopeLabel : THEME_COLORS.envelopeLabelMuted;
    ctx.textBaseline = "alphabetic";
    const labelX = rect.x + THEME_METRICS.envelopeLabelInset;
    const labelWidth = Math.max(0, rect.x + rect.width - THEME_METRICS.envelopeLabelInset - labelX);
    ctx.fillText(fitCanvasLabel(ctx, state.label, labelWidth), labelX, rect.y + THEME_METRICS.envelopeLabelBaseline);
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


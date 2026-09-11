import type {
  ActiveGuiDocument,
  AppSnapshot,
  GeometryRenderBounds,
  Point3Meters,
  SequenceAutomationMapping,
  SequenceAutomationTarget,
  SequenceSelection as WireSequenceSelection
} from "../../types";
import { THEME_COLORS, THEME_METRICS, THEME_TYPOGRAPHY } from "../../theme";

export type Point3 = { x: number; y: number; z: number };


export type AudioTransportViewSnapshot = AppSnapshot["audioTransport"];

export type ReadyGuiDocument = Exclude<ActiveGuiDocument, { type: "blocked" }>;

export type SequenceSelection = WireSequenceSelection | null;

export type AutomationClipChooser = {
  target: SequenceAutomationTarget;
  mapping: SequenceAutomationMapping;
} | null;

export function automationTargetsEqual(
  left: SequenceAutomationTarget,
  right: SequenceAutomationTarget
) {
  if (left.type !== right.type) return false;
  return left.type === "effectParam"
    ? left.effectId === (right.type === "effectParam" ? right.effectId : undefined) && left.param === right.param
    : left.nodeId === (right.type === "compositionNodeParam" ? right.nodeId : undefined) && left.param === right.param;
}

export type GuiFocus =
  | { type: "effect"; id: number }
  | { type: "graphNode"; nodeId: string }
  | { type: "graphEdge"; edgeId: string }
  | { type: "automationClip"; id: number }
  | { type: "mark"; collectionKey: string; index: number }
  | null;

export const GUI_CANVAS = {
  spatialPaddingPx: THEME_METRICS.canvasPadding,
  pointHitMeters: THEME_METRICS.canvasPointHitRadius,
  nanosecondRoundScale: 1_000_000_000
} as const;

const GUI_COLORS = {
  canvasBackground: THEME_COLORS.canvasBackground,
  canvasGrid: THEME_COLORS.canvasGrid,
  guide: THEME_COLORS.canvasGuide,
  axis: THEME_COLORS.canvasAxis,
  majorGrid: THEME_COLORS.canvasMajorGrid,
  label: THEME_COLORS.canvasLabel
} as const;

export function normalizePoint(point: Point3Meters): Point3 {
  return {
    x: point.xMeters,
    y: point.yMeters,
    z: point.zMeters
  };
}




export type RenderBounds = {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
};

export function normalizeBounds(bounds: GeometryRenderBounds): RenderBounds {
  return {
    minX: bounds.minXMeters,
    minY: bounds.minYMeters,
    maxX: bounds.maxXMeters,
    maxY: bounds.maxYMeters
  };
}

export function drawSpatialCanvas(
  canvas: HTMLCanvasElement | null,
  bounds: RenderBounds,
  draw: (ctx: CanvasRenderingContext2D, project: (point: Point3) => { x: number; y: number }) => void,
  viewport?: SpatialViewport
) {
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.floor(rect.width * dpr));
  canvas.height = Math.max(1, Math.floor(rect.height * dpr));
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, rect.width, rect.height);
  ctx.fillStyle = GUI_COLORS.canvasBackground;
  ctx.fillRect(0, 0, rect.width, rect.height);
  ctx.font = THEME_TYPOGRAPHY.canvas;
  const view = viewport ?? fitViewport(bounds, rect.width, rect.height);
  const project = (point: Point3) => projectPoint(point, rect.width, rect.height, view);
  drawGrid(ctx, rect.width, rect.height, view);
  draw(ctx, project);
}

export type SpatialViewport = { scale: number; fitScale: number; offsetX: number; offsetY: number };

export function fitViewport(bounds: RenderBounds, width: number, height: number): SpatialViewport {
  const spanX = Math.max(1, bounds.maxX - bounds.minX);
  const spanY = Math.max(1, bounds.maxY - bounds.minY);
  const padding = THEME_METRICS.canvasPadding;
  const scale = Math.min((width - padding * 2) / spanX, (height - padding * 2) / spanY);
  return { scale, fitScale: scale, offsetX: padding - bounds.minX * scale, offsetY: height - padding + bounds.minY * scale };
}

function drawGrid(ctx: CanvasRenderingContext2D, width: number, height: number, view: SpatialViewport) {
  const meters = view.scale > 180 ? 1 : view.scale > 70 ? 2 : view.scale > 25 ? 5 : view.scale > 8 ? 10 : 20;
  const left = -view.offsetX / view.scale;
  const right = (width - view.offsetX) / view.scale;
  const bottom = (view.offsetY - height) / view.scale;
  const top = view.offsetY / view.scale;
  ctx.font = THEME_TYPOGRAPHY.canvasLabel;
  for (let value = Math.floor(left / meters) * meters; value <= right; value += meters) {
    const x = view.offsetX + value * view.scale;
    ctx.strokeStyle = Math.abs(value) < 1e-8 ? GUI_COLORS.axis : (Math.round(value / meters) % 5 === 0 ? GUI_COLORS.majorGrid : GUI_COLORS.canvasGrid);
    ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, height); ctx.stroke();
    if (x > THEME_METRICS.canvasLabelLeftInset && x < width - THEME_METRICS.canvasLabelRightInset) { ctx.fillStyle = GUI_COLORS.label; ctx.fillText(`${value}m`, x + THEME_METRICS.canvasLabelLeftInset - 1, height - THEME_METRICS.canvasLabelBottomInset); }
  }
  for (let value = Math.floor(bottom / meters) * meters; value <= top; value += meters) {
    const y = view.offsetY - value * view.scale;
    ctx.strokeStyle = Math.abs(value) < 1e-8 ? GUI_COLORS.axis : (Math.round(value / meters) % 5 === 0 ? GUI_COLORS.majorGrid : GUI_COLORS.canvasGrid);
    ctx.beginPath();
    ctx.moveTo(0, y); ctx.lineTo(width, y);
    ctx.stroke();
    if (y > THEME_METRICS.canvasLabelTopInset && y < height - THEME_METRICS.canvasLabelLeftInset) { ctx.fillStyle = GUI_COLORS.label; ctx.fillText(`${value}m`, THEME_METRICS.canvasLabelXOffset, y - THEME_METRICS.canvasLabelYOffset); }
  }
}

function projectPoint(point: Point3, _width: number, _height: number, view: SpatialViewport) {
  return { x: view.offsetX + point.x * view.scale, y: view.offsetY - point.y * view.scale };
}

export function unproject(x: number, y: number, canvas: HTMLCanvasElement | null, bounds: RenderBounds, viewport?: SpatialViewport): Point3 {
  const rect = canvas?.getBoundingClientRect();
  const width = rect?.width ?? 1;
  const height = rect?.height ?? 1;
  const view = viewport ?? fitViewport(bounds, width, height);
  return {
    x: (x - view.offsetX) / view.scale,
    y: (view.offsetY - y) / view.scale,
    z: 0
  };
}

export function nearestPoint(points: Point3[], point: Point3, hitRadiusMeters: number = GUI_CANVAS.pointHitMeters) {
  let best: number | null = null;
  let bestDistance = Infinity;
  for (let index = 0; index < points.length; index += 1) {
    const candidate = points[index];
    if (candidate === undefined) continue;
    const distance = Math.hypot(candidate.x - point.x, candidate.y - point.y);
    if (distance < bestDistance && distance < hitRadiusMeters) {
      best = index;
      bestDistance = distance;
    }
  }
  return best;
}


export function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

export function roundToNanosecond(value: number) {
  return Math.round(value * GUI_CANVAS.nanosecondRoundScale) / GUI_CANVAS.nanosecondRoundScale;
}

export function formatSeconds(value: number) {
  const totalSeconds = Math.max(0, Math.floor(value));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

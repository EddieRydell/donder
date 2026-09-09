import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { commands } from "../../../api";
import type { GuiDocumentRequest, PropDocument } from "../../../types";
import { runGuiEditCommand, useAppStore } from "../../../store";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import { denormalizePoint, drawSpatialCanvas, nearestPoint, normalizeBounds, normalizePoint, round6, unproject, type GuiFocus, type Point3 } from "../shared";
import { SpatialControls, useSpacePressed, useSpatialViewport } from "../SpatialViewport";

function distanceToSegment(point: Point3, from: Point3, to: Point3) {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const lengthSquared = dx * dx + dy * dy;
  const t = lengthSquared === 0 ? 0 : Math.max(0, Math.min(1, ((point.x - from.x) * dx + (point.y - from.y) * dy) / lengthSquared));
  return Math.hypot(point.x - (from.x + t * dx), point.y - (from.y + t * dy));
}

function pointNearGuide(point: Point3, fixture: PropDocument["fixture"], hitRadius: number) {
  if (fixture.renderPlan.guides.some((guide) => guide.type === "line" && distanceToSegment(point, normalizePoint(guide.from), normalizePoint(guide.to)) <= hitRadius)) return true;
  if (fixture.geometry.type === "arc") {
    const emitters = fixture.renderPlan.emitters.map(normalizePoint);
    return emitters.some((from, index) => {
      const to = emitters[index + 1];
      return to !== undefined && distanceToSegment(point, from, to) <= hitRadius;
    });
  }
  return false;
}

type Gesture =
  | { type: "empty"; x: number; y: number }
  | { type: "pan"; x: number; y: number }
  | { type: "box"; x: number; y: number; currentX: number; currentY: number }
  | { type: "point"; pointIndex: number; draft: Point3 }
  | null;

export function FixtureCanvas({ document, selected, setSelected }: { document: PropDocument; selected: GuiFocus; setSelected: (id: GuiFocus) => void }) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const gesture = useRef<Gesture>(null);
  const gestureRequest = useRef<GuiDocumentRequest | null>(null);
  const pendingDraft = useRef<{ pointIndex: number; draft: Point3 } | null>(null);
  const [revision, render] = useState(0);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [selectedIndices, setSelectedIndices] = useState<Set<number>>(new Set());
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);
  const [hoveredGeometry, setHoveredGeometry] = useState(false);
  const fixture = document.fixture;
  const bounds = useMemo(() => normalizeBounds(fixture.renderPlan.bounds), [fixture]);
  const spatial = useSpatialViewport(bounds, fixture.objectKey, `${document.path}:${fixture.objectKey}`);
  const spacePressed = useSpacePressed();

  useEffect(() => {
    const rect = canvas.current?.getBoundingClientRect(); if (rect) spatial.resize(rect.width, rect.height);
    drawSpatialCanvas(canvas.current, bounds, (ctx, project) => {
      const geometrySelected = selectedIndices.size > 0 || selected?.type === "point";
      const drawRenderedArc = () => {
        if (fixture.geometry.type !== "arc") return;
        fixture.renderPlan.emitters.forEach((emitter, index) => {
          const projected = project(normalizePoint(emitter));
          if (index === 0) ctx.moveTo(projected.x, projected.y);
          else ctx.lineTo(projected.x, projected.y);
        });
      };
      if (geometrySelected || hoveredGeometry) {
        ctx.save();
        ctx.strokeStyle = geometrySelected ? THEME_COLORS.layoutSelected : THEME_COLORS.layoutLabel;
        ctx.globalAlpha = geometrySelected ? THEME_METRICS.layoutSelectedAlpha : THEME_METRICS.layoutUnselectedAlpha;
        ctx.lineWidth = geometrySelected ? THEME_METRICS.layoutSelectedLineWidth : THEME_METRICS.layoutUnselectedLineWidth;
        ctx.lineCap = "round";
        ctx.lineJoin = "round";
        for (const guide of fixture.renderPlan.guides) {
          if (guide.type !== "line") continue;
          const from = project(normalizePoint(guide.from)); const to = project(normalizePoint(guide.to));
          ctx.beginPath(); ctx.moveTo(from.x, from.y); ctx.lineTo(to.x, to.y); ctx.stroke();
        }
        if (fixture.geometry.type === "arc") { ctx.beginPath(); drawRenderedArc(); ctx.stroke(); }
        ctx.globalAlpha = THEME_METRICS.layoutLabelAlpha;
        ctx.lineWidth = geometrySelected ? THEME_METRICS.layoutLineWidthSelected : THEME_METRICS.layoutLineWidth;
        for (const guide of fixture.renderPlan.guides) {
          if (guide.type !== "line") continue;
          const from = project(normalizePoint(guide.from)); const to = project(normalizePoint(guide.to));
          ctx.beginPath(); ctx.moveTo(from.x, from.y); ctx.lineTo(to.x, to.y); ctx.stroke();
        }
        if (fixture.geometry.type === "arc") { ctx.beginPath(); drawRenderedArc(); ctx.stroke(); }
        ctx.restore();
      }
      for (const guide of fixture.renderPlan.guides) {
        if (guide.type !== "line") continue;
        const from = project(normalizePoint(guide.from)); const to = project(normalizePoint(guide.to));
        ctx.strokeStyle = THEME_COLORS.canvasGuide; ctx.beginPath(); ctx.moveTo(from.x, from.y); ctx.lineTo(to.x, to.y); ctx.stroke();
      }
      if (fixture.geometry.type === "arc") { ctx.strokeStyle = THEME_COLORS.canvasGuide; ctx.beginPath(); drawRenderedArc(); ctx.stroke(); }
      fixture.renderPlan.emitters.forEach((point, index) => {
        const source = gesture.current?.type === "point" && gesture.current.pointIndex === index
          ? gesture.current.draft
          : pendingDraft.current?.pointIndex === index
            ? pendingDraft.current.draft
            : normalizePoint(point);
        const projected = project(source);
        const isSelected = selectedIndices.has(index) || (selected?.type === "point" && selected.index === index);
        const isHovered = hoveredIndex === index;
        if (isSelected || isHovered) {
          ctx.save();
          ctx.strokeStyle = isSelected ? THEME_COLORS.layoutSelected : THEME_COLORS.layoutLabel;
          ctx.globalAlpha = isSelected ? THEME_METRICS.layoutSelectedAlpha : THEME_METRICS.layoutUnselectedAlpha;
          ctx.lineWidth = isSelected ? THEME_METRICS.layoutSelectedLineWidth : THEME_METRICS.layoutUnselectedLineWidth;
          ctx.beginPath(); ctx.arc(projected.x, projected.y, THEME_METRICS.layoutSelectedPointRadius, 0, Math.PI * 2); ctx.stroke();
          ctx.globalAlpha = THEME_METRICS.layoutLabelAlpha; ctx.lineWidth = isSelected ? THEME_METRICS.layoutLineWidthSelected : THEME_METRICS.layoutLineWidth; ctx.stroke(); ctx.restore();
        }
        ctx.fillStyle = THEME_COLORS.playhead;
        ctx.beginPath(); ctx.arc(projected.x, projected.y, THEME_METRICS.spatialPointRadius, 0, Math.PI * 2); ctx.fill();
      });
      const box = gesture.current?.type === "box" ? gesture.current : null;
      if (box) {
        ctx.strokeStyle = THEME_COLORS.accent; ctx.setLineDash([THEME_METRICS.layoutSelectionDash, THEME_METRICS.layoutSelectionGap]);
        ctx.strokeRect(Math.min(box.x, box.currentX), Math.min(box.y, box.currentY), Math.abs(box.currentX - box.x), Math.abs(box.currentY - box.y));
        ctx.setLineDash([]);
      }
    }, spatial.view);
  }, [bounds, fixture, hoveredGeometry, hoveredIndex, revision, selected, selectedIndices, spatial]);

  const worldAt = (event: ReactPointerEvent<HTMLCanvasElement>) => { const rect = event.currentTarget.getBoundingClientRect(); return unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view); };
  return <div className="spatial-canvas-shell">
    <canvas ref={canvas} className="gui-canvas" tabIndex={0}
      onKeyDown={(event) => { if (event.key === "Home") { event.preventDefault(); spatial.reset(); } }}
      onContextMenu={(event) => { event.preventDefault(); setMenu({ x: event.nativeEvent.offsetX, y: event.nativeEvent.offsetY }); }}
      onPointerDown={(event) => {
        gestureRequest.current = useAppStore.getState().guiRequest;
        if (event.button === 2) return;
        event.currentTarget.setPointerCapture(event.pointerId); setMenu(null);
        if (event.button === 1 || spacePressed.current) { gesture.current = { type: "pan", x: event.clientX, y: event.clientY }; return; }
        if (event.shiftKey) { gesture.current = { type: "box", x: event.nativeEvent.offsetX, y: event.nativeEvent.offsetY, currentX: event.nativeEvent.offsetX, currentY: event.nativeEvent.offsetY }; return; }
        if (fixture.geometry.type === "points") {
          const points = fixture.geometry.points.map(normalizePoint); const index = nearestPoint(points, worldAt(event), THEME_METRICS.spatialHitRadius / spatial.view.scale);
          if (index !== null) {
            const point = points[index];
            if (point) {
              setSelectedIndices(new Set([index])); setSelected({ type: "point", index }); if (useAppStore.getState().snapshot?.activeBuffer?.readOnly === true) return; gesture.current = { type: "point", pointIndex: index, draft: point };
              return;
            }
          }
        }
        gesture.current = { type: "empty", x: event.clientX, y: event.clientY };
      }}
      onPointerMove={(event) => {
        const current = gesture.current; if (!current) return;
        if (current.type === "box") { const rect = event.currentTarget.getBoundingClientRect(); gesture.current = { ...current, currentX: event.clientX - rect.left, currentY: event.clientY - rect.top }; render((value) => value + 1); return; }
        if (current.type === "empty" && Math.hypot(event.clientX - current.x, event.clientY - current.y) > THEME_METRICS.spatialPanThreshold) gesture.current = { type: "pan", x: current.x, y: current.y };
        const active = gesture.current;
        if (active?.type === "pan") { spatial.panBy(event.clientX - active.x, event.clientY - active.y); gesture.current = { ...active, x: event.clientX, y: event.clientY }; return; }
        if (active?.type !== "point") return;
        const world = worldAt(event); active.draft = { x: round6(world.x), y: round6(world.y), z: active.draft.z }; render((value) => value + 1);
      }}
      onPointerUp={() => {
        const current = gesture.current; gesture.current = null;
        if (current?.type === "box") {
          const minX = Math.min(current.x, current.currentX); const maxX = Math.max(current.x, current.currentX); const minY = Math.min(current.y, current.currentY); const maxY = Math.max(current.y, current.currentY);
          const indices = fixture.geometry.type === "points" ? fixture.geometry.points.map(normalizePoint).flatMap((point, index) => { const projected = { x: spatial.view.offsetX + point.x * spatial.view.scale, y: spatial.view.offsetY - point.y * spatial.view.scale }; return projected.x >= minX && projected.x <= maxX && projected.y >= minY && projected.y <= maxY ? [index] : []; }) : [];
          setSelectedIndices(new Set(indices)); const primary = indices[0]; setSelected(primary === undefined ? null : { type: "point", index: primary }); render((value) => value + 1);
        }
        if (current?.type === "empty") setSelected(null);
        if (current?.type === "point") {
          pendingDraft.current = { pointIndex: current.pointIndex, draft: current.draft };
          render((value) => value + 1);
          void runGuiEditCommand((request) => commands.applyPropGuiEdit(request, { type: "movePoint", pointIndex: current.pointIndex, point: denormalizePoint(current.draft) }), gestureRequest.current).finally(() => {
            pendingDraft.current = null;
            render((value) => value + 1);
          });
        }
      }}
      onPointerCancel={() => { gesture.current = null; }}
      onPointerLeave={() => { if (!gesture.current) { setHoveredIndex(null); setHoveredGeometry(false); } }}
      onPointerMoveCapture={(event) => {
        if (gesture.current) return;
        const world = worldAt(event);
        const hitRadius = THEME_METRICS.spatialHitRadius / spatial.view.scale;
        setHoveredGeometry(pointNearGuide(world, fixture, hitRadius));
        setHoveredIndex(fixture.geometry.type === "points" ? nearestPoint(fixture.geometry.points.map(normalizePoint), world, hitRadius) : null);
      }}
      onWheel={(event) => { event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect(); spatial.zoomAt(Math.exp(-event.deltaY * 0.0015), event.clientX - rect.left, event.clientY - rect.top); }}
    />
    {menu && <div className="spatial-context-menu" style={{ left: menu.x, top: menu.y }}><button onClick={() => { spatial.reset(); setMenu(null); }}>Fit view</button><button onClick={() => { setSelected(null); setMenu(null); }}>Deselect</button></div>}
    <SpatialControls view={spatial.view} reset={spatial.reset} zoomAt={(factor) => { const rect = canvas.current?.getBoundingClientRect(); spatial.zoomAt(factor, (rect?.width ?? 0) / 2, (rect?.height ?? 0) / 2); }} />
  </div>;
}

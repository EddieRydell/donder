import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { commands } from "../../../api";
import type { GuiDocumentRequest, PreviewDocument } from "../../../types";
import { runGuiEditCommand, useAppStore } from "../../../store";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import { denormalizeTransform, drawSpatialCanvas, normalizeBounds, normalizePoint, normalizeTransform, round6, unproject, type GuiFocus, type Point3, type Transform } from "../shared";
import { SpatialControls, useSpacePressed, useSpatialViewport } from "../SpatialViewport";

type Gesture =
  | { type: "empty"; x: number; y: number }
  | { type: "pan"; x: number; y: number }
  | { type: "box"; x: number; y: number; currentX: number; currentY: number }
  | { type: "object"; id: number; startX: number; startY: number; original: Transform; draft: Transform }
  | null;

function distanceToSegment(point: Point3, from: Point3, to: Point3) {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const lengthSquared = dx * dx + dy * dy;
  const t = lengthSquared === 0 ? 0 : Math.max(0, Math.min(1, ((point.x - from.x) * dx + (point.y - from.y) * dy) / lengthSquared));
  return Math.hypot(point.x - (from.x + t * dx), point.y - (from.y + t * dy));
}

function segmentIntersectsBox(from: { x: number; y: number }, to: { x: number; y: number }, minX: number, minY: number, maxX: number, maxY: number) {
  const inside = (point: { x: number; y: number }) => point.x >= minX && point.x <= maxX && point.y >= minY && point.y <= maxY;
  if (inside(from) || inside(to)) return true;
  const intersects = (a: { x: number; y: number }, b: { x: number; y: number }, c: { x: number; y: number }, d: { x: number; y: number }) => {
    const cross = (first: { x: number; y: number }, second: { x: number; y: number }, third: { x: number; y: number }) => (second.x - first.x) * (third.y - first.y) - (second.y - first.y) * (third.x - first.x);
    const first = cross(a, b, c); const second = cross(a, b, d); const third = cross(c, d, a); const fourth = cross(c, d, b);
    return ((first >= 0 && second <= 0) || (first <= 0 && second >= 0)) && ((third >= 0 && fourth <= 0) || (third <= 0 && fourth >= 0));
  };
  const topLeft = { x: minX, y: minY }; const topRight = { x: maxX, y: minY }; const bottomRight = { x: maxX, y: maxY }; const bottomLeft = { x: minX, y: maxY };
  return intersects(from, to, topLeft, topRight) || intersects(from, to, topRight, bottomRight) || intersects(from, to, bottomRight, bottomLeft) || intersects(from, to, bottomLeft, topLeft);
}

export function LayoutCanvas({ document, selected, setSelected }: { document: PreviewDocument; selected: GuiFocus; setSelected: (id: GuiFocus) => void }) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const gesture = useRef<Gesture>(null);
  const gestureRequest = useRef<GuiDocumentRequest | null>(null);
  const pendingDraft = useRef<{ id: number; draft: Transform } | null>(null);
  const [revision, render] = useState(0);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [hoveredId, setHoveredId] = useState<number | null>(null);
  const bounds = useMemo(() => normalizeBounds(document.renderBounds), [document.renderBounds]);
  const spatial = useSpatialViewport(bounds, undefined, `${document.path}:${document.objectKey}`);
  const spacePressed = useSpacePressed();

  const pointsFor = (fixture: PreviewDocument["fixtures"][number], transform = normalizeTransform(fixture.transform)): Point3[] => fixture.resolvedFixture.renderPlan.emitters.map((emitter) => {
    const point = normalizePoint(emitter);
    return { x: transform.position.x + point.x * transform.scale.x, y: transform.position.y + point.y * transform.scale.y, z: transform.position.z + point.z * transform.scale.z };
  });

  const hitFixture = (point: Point3) => {
    const radius = THEME_METRICS.spatialHitRadius / spatial.view.scale;
    let closest: { fixture: PreviewDocument["fixtures"][number]; distance: number } | null = null;
    for (const fixture of document.fixtures) {
      const points = pointsFor(fixture);
      for (let index = 0; index < points.length; index += 1) {
        const from = points[index];
        if (!from) continue;
        const to = points[index + 1];
        const distance = to ? distanceToSegment(point, from, to) : Math.hypot(point.x - from.x, point.y - from.y);
        if (distance <= radius && (!closest || distance < closest.distance)) closest = { fixture, distance };
      }
    }
    return closest?.fixture ?? null;
  };

  useEffect(() => {
    const rect = canvas.current?.getBoundingClientRect();
    if (rect) spatial.resize(rect.width, rect.height);
    drawSpatialCanvas(canvas.current, bounds, (ctx, project) => {
      for (const fixture of document.fixtures) {
        const active = gesture.current?.type === "object" && gesture.current.id === fixture.id
          ? gesture.current.draft
          : pendingDraft.current?.id === fixture.id
            ? pendingDraft.current.draft
            : normalizeTransform(fixture.transform);
        const points = pointsFor(fixture, active);
        const isSelected = selectedIds.has(fixture.id) || (selected?.type === "placement" && selected.id === fixture.id);
        const isHovered = hoveredId === fixture.id;
        if (points.length > 0 && (isSelected || isHovered)) {
          const firstPoint = points[0];
          ctx.save();
          ctx.strokeStyle = isSelected ? THEME_COLORS.layoutSelected : THEME_COLORS.layoutLabel;
          ctx.globalAlpha = isSelected ? THEME_METRICS.layoutSelectedAlpha : THEME_METRICS.layoutUnselectedAlpha;
          ctx.lineWidth = isSelected ? THEME_METRICS.layoutSelectedLineWidth : THEME_METRICS.layoutUnselectedLineWidth;
          ctx.lineCap = "round";
          ctx.lineJoin = "round";
          ctx.beginPath();
          points.forEach((point, index) => { const projected = project(point); if (index === 0) ctx.moveTo(projected.x, projected.y); else ctx.lineTo(projected.x, projected.y); });
          if (points.length === 1 && firstPoint) { const projected = project(firstPoint); ctx.arc(projected.x, projected.y, THEME_METRICS.layoutSelectedPointRadius, 0, Math.PI * 2); }
          ctx.stroke();
          ctx.globalAlpha = THEME_METRICS.layoutLabelAlpha;
          ctx.lineWidth = isSelected ? THEME_METRICS.layoutLineWidthSelected : THEME_METRICS.layoutLineWidth;
          ctx.stroke();
          ctx.restore();
        }
        if (isSelected || isHovered) {
          const labelPoint = points[0] === undefined ? { x: active.position.x, y: active.position.y, z: active.position.z } : points[0];
          const projectedLabelPoint = project(labelPoint);
          ctx.fillStyle = THEME_COLORS.text; ctx.fillText(fixture.name, projectedLabelPoint.x + THEME_METRICS.layoutLabelOffsetX, projectedLabelPoint.y - THEME_METRICS.layoutLabelOffsetY);
        }
        for (const [index] of fixture.resolvedFixture.renderPlan.emitters.entries()) {
          const point = points[index];
          if (!point) continue;
          const projected = project(point);
          const halfSize = THEME_METRICS.layoutEmitterHalfSize;
          ctx.fillStyle = THEME_COLORS.automation; ctx.fillRect(projected.x - halfSize, projected.y - halfSize, halfSize * 2, halfSize * 2);
        }
      }
      const box = gesture.current?.type === "box" ? gesture.current : null;
      if (box) {
      ctx.strokeStyle = THEME_COLORS.accent; ctx.setLineDash([THEME_METRICS.layoutSelectionDash, THEME_METRICS.layoutSelectionGap]);
        ctx.strokeRect(Math.min(box.x, box.currentX), Math.min(box.y, box.currentY), Math.abs(box.currentX - box.x), Math.abs(box.currentY - box.y));
        ctx.setLineDash([]);
      }
    }, spatial.view);
  }, [bounds, document, hoveredId, revision, selected, selectedIds, spatial]);

  const worldAt = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    return unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view);
  };
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
        const world = worldAt(event);
        const hit = hitFixture(world);
        if (!hit) { gesture.current = { type: "empty", x: event.clientX, y: event.clientY }; return; }
        setSelectedIds(new Set([hit.id])); setSelected({ type: "placement", id: hit.id });
        if (useAppStore.getState().snapshot?.activeBuffer?.readOnly === true) return;
        gesture.current = { type: "object", id: hit.id, startX: world.x, startY: world.y, original: normalizeTransform(hit.transform), draft: normalizeTransform(hit.transform) };
      }}
      onPointerMove={(event) => {
        const current = gesture.current; if (!current) return;
        if (current.type === "box") { const rect = event.currentTarget.getBoundingClientRect(); gesture.current = { ...current, currentX: event.clientX - rect.left, currentY: event.clientY - rect.top }; render((value) => value + 1); return; }
        if (current.type === "empty" && Math.hypot(event.clientX - current.x, event.clientY - current.y) > THEME_METRICS.spatialPanThreshold) gesture.current = { type: "pan", x: current.x, y: current.y };
        const active = gesture.current;
        if (active?.type === "pan") { spatial.panBy(event.clientX - active.x, event.clientY - active.y); gesture.current = { ...active, x: event.clientX, y: event.clientY }; return; }
        if (active?.type !== "object") return;
        const world = worldAt(event);
        active.draft = { ...active.original, position: { ...active.original.position, x: round6(active.original.position.x + world.x - active.startX), y: round6(active.original.position.y + world.y - active.startY) } };
        render((value) => value + 1);
      }}
      onPointerUp={() => {
        const current = gesture.current; gesture.current = null;
        if (current?.type === "box") {
          const minX = Math.min(current.x, current.currentX); const maxX = Math.max(current.x, current.currentX); const minY = Math.min(current.y, current.currentY); const maxY = Math.max(current.y, current.currentY);
          const ids = document.fixtures.filter((fixture) => {
            const points = pointsFor(fixture).map((point) => ({ x: spatial.view.offsetX + point.x * spatial.view.scale, y: spatial.view.offsetY - point.y * spatial.view.scale }));
            return points.some((point) => point.x >= minX && point.x <= maxX && point.y >= minY && point.y <= maxY) || points.some((point, index) => { const next = points[index + 1]; return next !== undefined && segmentIntersectsBox(point, next, minX, minY, maxX, maxY); });
          }).map((fixture) => fixture.id);
          setSelectedIds(new Set(ids)); const primary = ids[0]; setSelected(primary === undefined ? null : { type: "placement", id: primary }); render((value) => value + 1);
        }
        if (current?.type === "empty") setSelected(null);
        if (current?.type === "object") {
          pendingDraft.current = { id: current.id, draft: current.draft };
          render((value) => value + 1);
          void runGuiEditCommand((request) => commands.applyPreviewGuiEdit(request, { type: "updatePlacementTransform", id: current.id, transform: denormalizeTransform(current.draft) }), gestureRequest.current).finally(() => {
            pendingDraft.current = null;
            render((value) => value + 1);
          });
        }
      }}
      onPointerCancel={() => { gesture.current = null; }}
      onPointerLeave={() => { if (!gesture.current) setHoveredId(null); }}
      onPointerMoveCapture={(event) => { if (!gesture.current) setHoveredId(hitFixture(worldAt(event))?.id ?? null); }}
      onWheel={(event) => { event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect(); spatial.zoomAt(Math.exp(-event.deltaY * 0.0015), event.clientX - rect.left, event.clientY - rect.top); }}
    />
    {menu && <div className="spatial-context-menu" style={{ left: menu.x, top: menu.y }}><button onClick={() => { spatial.reset(); setMenu(null); }}>Fit view</button><button onClick={() => { setSelected(null); setMenu(null); }}>Deselect</button></div>}
    <SpatialControls view={spatial.view} reset={spatial.reset} zoomAt={(factor) => { const rect = canvas.current?.getBoundingClientRect(); spatial.zoomAt(factor, (rect?.width ?? 0) / 2, (rect?.height ?? 0) / 2); }} />
  </div>;
}

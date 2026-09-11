import { useEffect, useMemo, useRef, useState } from "react";
import type { Point3Meters, SpatialRenderPlan } from "../../../types";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import { drawSpatialCanvas, nearestPoint, normalizeBounds, normalizePoint, unproject } from "../shared";
import { SpatialControls, useSpatialViewport } from "../SpatialViewport";

type Move = (delta: Point3Meters) => Promise<void>;
type Gesture = { type: "pan"; x: number; y: number } | { type: "move"; owner: number; x: number; y: number; scale: number; commit: Move };
type Offset = { owner: number; x: number; y: number };

export function SpatialCanvas({ plan, documentKey, selected, onSelect, onMoveStart }: { plan: SpatialRenderPlan; documentKey: string; selected: number | null; onSelect: (owner: number | null) => void; onMoveStart: (owner: number) => Move | null }) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const gesture = useRef<Gesture | null>(null);
  const [offset, setOffset] = useState<Offset | null>(null);
  const bounds = useMemo(() => normalizeBounds(plan.bounds), [plan.bounds]);
  const spatial = useSpatialViewport(bounds, documentKey, documentKey);
  const points = useMemo(() => plan.pixels.map((pixel) => normalizePoint(pixel.position)), [plan.pixels]);
  useEffect(() => {
    const element = canvas.current;
    if (element === null) return;
    const draw = () => {
      const rect = element.getBoundingClientRect();
      spatial.resize(rect.width, rect.height);
      drawSpatialCanvas(element, bounds, (context, project) => {
        plan.pixels.forEach((pixel) => {
          const position = normalizePoint(pixel.position);
          if (offset?.owner === pixel.owner) { position.x += offset.x; position.y += offset.y; }
          const point = project(position);
          const radius = Math.max(THEME_METRICS.spatialPointRadius, pixel.diameterMeters * spatial.view.scale / 2);
          context.fillStyle = selected === pixel.owner ? THEME_COLORS.layoutSelected : THEME_COLORS.playhead;
          context.beginPath(); context.arc(point.x, point.y, radius, 0, Math.PI * 2); context.fill();
        });
      }, spatial.view);
    };
    draw();
    const observer = new ResizeObserver(draw);
    observer.observe(element);
    return () => { observer.disconnect(); };
  }, [bounds, plan, selected, spatial, offset]);
  return <div className="spatial-canvas-shell">
    <canvas ref={canvas} className="gui-canvas" tabIndex={0} aria-label="Fixture pixels"
      onKeyDown={(event) => { if (event.key === "Home") { event.preventDefault(); spatial.reset(); } }}
      onPointerDown={(event) => {
        if (event.button !== 0 && event.button !== 1) return;
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        const rect = event.currentTarget.getBoundingClientRect();
        const world = unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view);
        const index = nearestPoint(points, world, THEME_METRICS.spatialHitRadius / spatial.view.scale);
        const owner = index === null ? null : plan.pixels[index]?.owner ?? null;
        onSelect(owner);
        const commit = owner !== null && event.button === 0 && !event.altKey ? onMoveStart(owner) : null;
        gesture.current = commit !== null && owner !== null
          ? { type: "move", owner, x: event.clientX, y: event.clientY, scale: spatial.view.scale, commit }
          : { type: "pan", x: event.clientX, y: event.clientY };
      }}
      onPointerMove={(event) => {
        const active = gesture.current;
        if (active === null) return;
        if (active.type === "pan") {
          spatial.panBy(event.clientX - active.x, event.clientY - active.y);
          gesture.current = { type: "pan", x: event.clientX, y: event.clientY };
        } else {
          setOffset({ owner: active.owner, x: (event.clientX - active.x) / active.scale, y: (active.y - event.clientY) / active.scale });
        }
      }}
      onPointerUp={(event) => {
        const active = gesture.current;
        gesture.current = null;
        if (active?.type === "move") {
          const delta = { xMeters: (event.clientX - active.x) / active.scale, yMeters: (active.y - event.clientY) / active.scale, zMeters: 0 };
          if (delta.xMeters !== 0 || delta.yMeters !== 0) void active.commit(delta).finally(() => { setOffset(null); });
          else setOffset(null);
        }
      }}
      onPointerCancel={() => { gesture.current = null; setOffset(null); }}
      onWheel={(event) => {
        event.preventDefault();
        if (gesture.current !== null) return;
        const rect = event.currentTarget.getBoundingClientRect();
        spatial.zoomAt(Math.exp(-event.deltaY * THEME_METRICS.spatialWheelZoomScale), event.clientX - rect.left, event.clientY - rect.top);
      }}
    />
    <SpatialControls view={spatial.view} reset={spatial.reset} zoomAt={spatial.zoomAt} />
  </div>;
}

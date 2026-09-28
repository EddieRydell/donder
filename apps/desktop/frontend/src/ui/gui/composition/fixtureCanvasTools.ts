import { useState } from "react";
import type { GuiFixtureHandle, Point3Meters, SpatialRenderPlan } from "../../../types";
import { THEME_COLORS, THEME_METRICS, THEME_TYPOGRAPHY } from "../../../theme";
import { normalizePoint, type Point3 } from "../shared";
import type { FixtureTool } from "./FixtureContextMenu";

export type FixtureCanvasTools = {
  enabled: boolean;
  tool: FixtureTool | null;
  session: number;
  handles: GuiFixtureHandle[];
  showOrder: boolean;
  onTool: (tool: FixtureTool) => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onCancel: () => void;
  onDraw: (tool: FixtureTool, points: Point3Meters[]) => void;
  onHandleStart: (id: number, index: number) => ((position: Point3Meters) => Promise<boolean>) | null;
};
type Draft = { tool: FixtureTool; session: number; points: Point3Meters[]; cursor: Point3Meters };
type HandleDrag = { handle: GuiFixtureHandle; position: Point3Meters; pointerOrigin: Point3Meters; handles: GuiFixtureHandle[]; phase: "dragging" | "committing"; commit: (position: Point3Meters) => Promise<boolean> };
type Project = (point: Point3) => { x: number; y: number };
const meters = (point: Point3): Point3Meters => ({ xMeters: point.x, yMeters: point.y, zMeters: point.z });
const draggedHandlePosition = (drag: HandleDrag, world: Point3): Point3Meters => ({
  xMeters: drag.handle.position.xMeters + world.x - drag.pointerOrigin.xMeters,
  yMeters: drag.handle.position.yMeters + world.y - drag.pointerOrigin.yMeters,
  zMeters: drag.handle.position.zMeters
});

export function useFixtureCanvasTools(tools: FixtureCanvasTools | undefined, selected: number | null, scale: number, plan: SpatialRenderPlan) {
  const [storedDraft, setDraft] = useState<Draft | null>(null);
  const [storedHandleDrag, setHandleDrag] = useState<HandleDrag | null>(null);
  const handleDrag = storedHandleDrag !== null && (storedHandleDrag.phase === "dragging" || storedHandleDrag.handles === tools?.handles) ? storedHandleDrag : null;
  const draft = storedDraft?.tool === tools?.tool && storedDraft?.session === tools?.session ? storedDraft : null;
  const handles = tools?.handles.filter((handle) => handle.element === selected) ?? [];
  const clear = () => { setDraft(null); setHandleDrag(null); };
  const finish = () => {
    if (tools?.enabled === true && draft !== null && draft.points.length >= 2) { tools.onDraw(draft.tool, draft.points); clear(); }
  };
  return {
    active: draft !== null || handleDrag !== null,
    cancel: clear,
    key: (key: string) => {
      if (key === "Escape") { clear(); tools?.onCancel(); return true; }
      if (key === "Enter" && draft?.tool === "polyline") { finish(); return true; }
      return false;
    },
    down: (world: Point3) => {
      if (tools === undefined || !tools.enabled) return false;
      const point = meters(world);
      if (tools.tool !== null) {
        if (tools.tool === "pixel") tools.onDraw("pixel", [point]);
        else if (tools.tool === "polyline") setDraft({ tool: "polyline", session: tools.session, points: [...(draft?.points ?? []), point], cursor: point });
        else setDraft({ tool: tools.tool, session: tools.session, points: [point], cursor: point });
        return true;
      }
      const hit = handles.find((handle) => Math.hypot(handle.position.xMeters - world.x, handle.position.yMeters - world.y) <= THEME_METRICS.spatialHitRadius / scale);
      if (hit === undefined) return false;
      const commit = tools.onHandleStart(hit.element, hit.index);
      if (commit === null) return false;
      setHandleDrag({ handle: hit, position: hit.position, pointerOrigin: point, handles: tools.handles, phase: "dragging", commit }); return true;
    },
    move: (world: Point3) => {
      if (handleDrag !== null) {
        if (handleDrag.phase === "dragging") setHandleDrag({ ...handleDrag, position: draggedHandlePosition(handleDrag, world) });
        return true;
      }
      if (draft !== null) { setDraft({ ...draft, cursor: meters(world) }); return true; }
      return false;
    },
    up: (world: Point3) => {
      if (handleDrag !== null) {
        if (handleDrag.phase === "dragging") {
          const position = draggedHandlePosition(handleDrag, world);
          if (position.xMeters === handleDrag.handle.position.xMeters && position.yMeters === handleDrag.handle.position.yMeters) { clear(); return true; }
          const committing: HandleDrag = { ...handleDrag, position, phase: "committing" };
          setHandleDrag(committing);
          void committing.commit(position).then((accepted) => { if (!accepted) setHandleDrag((current) => current === committing ? null : current); });
        }
        return true;
      }
      if (draft !== null && draft.tool !== "polyline") { if (tools?.enabled === true) tools.onDraw(draft.tool, [...draft.points, meters(world)]); clear(); return true; }
      return draft !== null;
    },
    draw: (context: CanvasRenderingContext2D, project: Project) => {
      if (tools === undefined) return;
      context.lineWidth = THEME_METRICS.fixtureGuideWidth;
      context.strokeStyle = THEME_COLORS.layoutSelected;
      context.fillStyle = THEME_COLORS.layoutSelected;
      context.font = THEME_TYPOGRAPHY.canvasLabel;
      if (tools.showOrder && selected !== null) {
        const pixels = plan.pixels.filter((pixel) => pixel.owner === selected);
        pixels.forEach((pixel, index) => {
          const point = project(normalizePoint(pixel.position));
          const next = pixels[index + 1];
          const previous = pixels[index - 1];
          const neighbor = next ?? previous;
          const other = neighbor === undefined ? undefined : project(normalizePoint(neighbor.position));
          if (index === 0 || other === undefined || Math.hypot(other.x - point.x, other.y - point.y) >= THEME_METRICS.fixtureLabelSpacing) context.fillText(index === 0 ? `Start · ${pixel.index + 1}` : String(pixel.index + 1), point.x + THEME_METRICS.canvasLabelXOffset, point.y - THEME_METRICS.canvasLabelYOffset);
          if (next !== undefined && other !== undefined) {
            context.beginPath(); context.moveTo(point.x, point.y); context.lineTo(other.x, other.y); context.stroke();
            const distance = Math.hypot(other.x - point.x, other.y - point.y);
            if (distance >= THEME_METRICS.fixtureLabelSpacing) {
              const dx = (other.x - point.x) / distance; const dy = (other.y - point.y) / distance;
              const x = (point.x + other.x) / 2; const y = (point.y + other.y) / 2; const size = THEME_METRICS.fixtureArrowSize;
              context.beginPath(); context.moveTo(x - dx * size - dy * size, y - dy * size + dx * size); context.lineTo(x, y); context.lineTo(x - dx * size + dy * size, y - dy * size - dx * size); context.stroke();
            }
          }
        });
      }
      handles.forEach((handle) => {
        const position = handleDrag?.handle.element === handle.element && handleDrag.handle.index === handle.index ? handleDrag.position : handle.position;
        const point = project(normalizePoint(position)); const radius = THEME_METRICS.fixtureHandleRadius;
        context.fillStyle = THEME_COLORS.canvasBackground;
        context.fillRect(point.x - radius, point.y - radius, radius * 2, radius * 2);
        context.strokeRect(point.x - radius, point.y - radius, radius * 2, radius * 2);
      });
      if (draft !== null) {
        const points = [...draft.points, draft.cursor].map((point) => project(normalizePoint(point)));
        const first = points[0]; const last = points[points.length - 1];
        if (first === undefined || last === undefined) return;
        context.beginPath();
        if (draft.tool === "grid") context.rect(first.x, first.y, last.x - first.x, last.y - first.y);
        else if (draft.tool === "arc" || draft.tool === "circle") { const angle = Math.atan2(last.y - first.y, last.x - first.x); context.arc(first.x, first.y, Math.hypot(last.x - first.x, last.y - first.y), angle, angle - (draft.tool === "circle" ? 2 * Math.PI : Math.PI), true); }
        else points.forEach((point, index) => { if (index === 0) context.moveTo(point.x, point.y); else context.lineTo(point.x, point.y); });
        context.stroke();
      }
    }
  };
}

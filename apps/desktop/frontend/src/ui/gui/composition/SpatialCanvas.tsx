import { useSpatialGuides } from "./spatialViewState";
import { boxSelection, selectedItems, selectionClick, unionBounds, type Box, type SpatialItem } from "./spatialSelection";
import { useAppStore } from "../../../store";
import { SpatialSnapControls } from "./SpatialSnapControls";
import { snapPoint, formatDistance, type Modifiers } from "./spatialSnapping";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import * as ContextMenu from "@radix-ui/react-context-menu";
import type { GuiObjectRef, Point3Meters, SpatialRenderPixel, SpatialRenderPlan } from "../../../types";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import { drawSpatialCanvas, nearestPoint, normalizeBounds, normalizePoint, unproject } from "../shared";
import { SpatialControls, useSpatialViewport } from "../SpatialViewport";
import { LayoutAddMenu } from "./LayoutAddMenu";
import { FixtureContextMenu } from "./FixtureContextMenu";
import { useFixtureCanvasTools, type FixtureCanvasTools } from "./fixtureCanvasTools";

type Move = { owners: number[]; origin: Point3Meters; commit: (delta: Point3Meters) => Promise<boolean> };
type Gesture = { type: "box"; start: Point3Meters; end: Point3Meters; selection: number[] } | { type: "pan"; x: number; y: number } | { type: "move"; clickSelection: number[]; x: number; y: number; scale: number; commit: Move; pixels: SpatialRenderPixel[] };
type Offset = { owners: number[]; x: number; y: number; pixels: SpatialRenderPixel[] };
type LayoutMenu = {
  availableFixtures: GuiObjectRef[];
  enabled: boolean;
  onAddFixture: (definition: GuiObjectRef, position: Point3Meters) => void;
  onCreateFixture: (position: Point3Meters) => void;
  onAddGroup: () => void;
};

export function SpatialCanvas({ plan, reference, documentKey, selection, items, onSelect, onMoveStart, onDelete, onDuplicate, layoutMenu, fixtureTools }: { plan: SpatialRenderPlan; reference: GuiObjectRef; documentKey: string; selection: number[]; items: SpatialItem[]; onSelect: (owners: number[]) => void; onMoveStart: (owners: number[], anchor: number) => Move | null; onDelete: () => void; onDuplicate: () => void; layoutMenu?: LayoutMenu; fixtureTools?: FixtureCanvasTools }) {
  const snapping = useAppStore((state) => state.snapshot?.settings.spatialSnap);
  if (snapping === undefined) throw new Error("Spatial editor settings are missing.");
  const guides = useSpatialGuides(reference);
  const chosen = selectedItems(items, selection);
  const selectedOwners = chosen.flatMap((item) => item.owners);
  const selected = selection.length === 1 ? selection[0] ?? null : null;
  const [marquee, setMarquee] = useState<Box | null>(null);
  const lastPointer = useRef({ clientX: 0, clientY: 0 });
  const pointerActive = useRef(false);
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const gesture = useRef<Gesture | null>(null);
  const settledOffsets = useRef(new WeakSet<Offset>());
  const [dragging, setDragging] = useState(false);
  const pending = useAppStore((state) => state.guiEditPending);
  const [offset, setOffset] = useState<Offset | null>(null);
  const [menuPosition, setMenuPosition] = useState<Point3Meters | null>(null);
  const bounds = useMemo(() => normalizeBounds(plan.bounds), [plan.bounds]);
  const spatial = useSpatialViewport(bounds, documentKey, documentKey);
  const targets = useMemo(() => fixtureTools === undefined ? plan.pixels.map((pixel) => ({ owner: pixel.owner, position: pixel.position })) : [...fixtureTools.handles.map((handle) => ({ owner: handle.element, position: handle.position })), ...plan.pixels.filter((pixel) => fixtureTools.elements.some((element) => element.id === pixel.owner && element.shape.type === "pixel")).map((pixel) => ({ owner: pixel.owner, position: pixel.position }))], [fixtureTools, plan.pixels]);
  const fixture = useFixtureCanvasTools(fixtureTools, selected, spatial.view.scale, plan, snapping, targets, guides.guides);
  useEffect(() => { if (fixtureTools?.tool !== null && fixtureTools?.tool !== undefined) canvas.current?.focus(); }, [fixtureTools?.tool]);
  const points = useMemo(() => plan.pixels.map((pixel) => normalizePoint(pixel.position)), [plan.pixels]);
  useLayoutEffect(() => {
    const element = canvas.current;
    if (element === null) return;
    let displayOffset = offset;
    if (offset !== null && (settledOffsets.current.has(offset) || offsetIsCommitted(offset, plan.pixels))) {
      settledOffsets.current.add(offset);
      displayOffset = null;
    }
    const draw = () => {
      const rect = element.getBoundingClientRect();
      spatial.resize(rect.width, rect.height);
      drawSpatialCanvas(element, bounds, (context, project) => {
        context.strokeStyle = THEME_COLORS.canvasGuide;
        context.lineWidth = THEME_METRICS.fixtureGuideWidth;
        context.setLineDash([THEME_METRICS.spatialSelectionDash]);
        for (const guide of guides.guides) {
          const point = project({ x: guide.axis === "x" ? guide.positionMeters : 0, y: guide.axis === "y" ? guide.positionMeters : 0, z: 0 });
          context.beginPath();
          if (guide.axis === "x") { context.moveTo(point.x, 0); context.lineTo(point.x, rect.height); }
          else { context.moveTo(0, point.y); context.lineTo(rect.width, point.y); }
          context.stroke();
        }
        context.setLineDash([]);
        plan.pixels.forEach((pixel) => {
          const position = normalizePoint(pixel.position);
          if (displayOffset?.owners.includes(pixel.owner) === true) { position.x += displayOffset.x; position.y += displayOffset.y; }
          const point = project(position);
          const radius = Math.max(THEME_METRICS.spatialPointRadius, pixel.diameterMeters * spatial.view.scale / 2);
          context.fillStyle = selectedOwners.includes(pixel.owner) ? THEME_COLORS.layoutSelected : THEME_COLORS.playhead;
          context.beginPath(); context.arc(point.x, point.y, radius, 0, Math.PI * 2); context.fill();
        });
        context.strokeStyle = THEME_COLORS.layoutSelected;
        context.lineWidth = THEME_METRICS.fixtureGuideWidth;
        if (chosen.length > 0) {
          const box = unionBounds(chosen.map((item) => item.bounds));
          const shift = displayOffset ?? { x: 0, y: 0 };
          const start = project({ x: box.left + shift.x, y: box.top + shift.y, z: 0 });
          const end = project({ x: box.right + shift.x, y: box.bottom + shift.y, z: 0 });
          context.setLineDash([THEME_METRICS.spatialSelectionDash]);
          context.strokeRect(start.x, start.y, end.x - start.x, end.y - start.y);
          context.setLineDash([]);
        }
        if (marquee !== null) {
          const start = project({ x: marquee.left, y: marquee.top, z: 0 });
          const end = project({ x: marquee.right, y: marquee.bottom, z: 0 });
          context.strokeRect(start.x, start.y, end.x - start.x, end.y - start.y);
        }
        const selectedOffset = selected !== null && displayOffset?.owners.includes(selected) === true ? displayOffset : null;
        fixture.draw(context, selectedOffset === null ? project : (point) => project({ ...point, x: point.x + selectedOffset.x, y: point.y + selectedOffset.y }));
      }, spatial.view, snapping.spacingMeters, snapping.unit);
    };
    draw();
    const observer = new ResizeObserver(draw);
    observer.observe(element);
    return () => { observer.disconnect(); };
  }, [bounds, plan, selected, selectedOwners, chosen, marquee, spatial, offset, fixture, snapping, guides.guides]);
  const moveDelta = (active: Extract<Gesture, { type: "move" }>, pointer: { clientX: number; clientY: number }, modifiers: Modifiers) => {
    const x = pointer.clientX - active.x; const y = pointer.clientY - active.y;
    const raw = Math.hypot(x, y) < THEME_METRICS.spatialPanThreshold ? { xMeters: 0, yMeters: 0, zMeters: 0 } : { xMeters: x / active.scale, yMeters: -y / active.scale, zMeters: 0 };
    if (raw.xMeters === 0 && raw.yMeters === 0) return raw;
    const origin = active.commit.origin;
    const point = snapPoint({ xMeters: origin.xMeters + raw.xMeters, yMeters: origin.yMeters + raw.yMeters, zMeters: origin.zMeters }, snapping, modifiers, targets.filter((target) => !active.commit.owners.includes(target.owner)), guides.guides, THEME_METRICS.spatialHitRadius / active.scale, origin);
    return { xMeters: point.xMeters - origin.xMeters, yMeters: point.yMeters - origin.yMeters, zMeters: 0 };
  };
  const refreshModifiers = (modifiers: Modifiers) => {
    const active = gesture.current;
    if (active?.type === "move") {
      const delta = moveDelta(active, lastPointer.current, modifiers);
      setOffset({ owners: active.commit.owners, x: delta.xMeters, y: delta.yMeters, pixels: active.pixels });
    } else if (active === null && canvas.current !== null) {
      const rect = canvas.current.getBoundingClientRect();
      fixture.move(unproject(lastPointer.current.clientX - rect.left, lastPointer.current.clientY - rect.top, canvas.current, bounds, spatial.view), modifiers);
    }
  };
  const cancel = () => { setMarquee(null); setDragging(false); pointerActive.current = false; gesture.current = null; setOffset(null); fixture.cancel(); };
  const canvasElement = <canvas ref={canvas} className="gui-canvas" tabIndex={0} aria-label="Spatial editor canvas"
      onKeyDown={(event) => {
        const enabled = fixtureTools?.enabled ?? layoutMenu?.enabled ?? false;
        if (gesture.current === null && !fixture.active && (fixtureTools?.tool === null || fixtureTools?.tool === undefined)) {
          if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a") { event.preventDefault(); onSelect(items.filter((item) => item.owners.length === 1 && item.owners[0] === item.id).map((item) => item.id)); return; }
          if (enabled && (event.key === "Delete" || event.key === "Backspace")) { event.preventDefault(); onDelete(); return; }
          if (enabled && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "d") { event.preventDefault(); onDuplicate(); return; }
          if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) {
            event.preventDefault();
            const anchor = selection[0];
            const move = anchor === undefined ? null : onMoveStart(selection, anchor);
            if (move !== null) {
              const step = snapping.spacingMeters * (event.shiftKey ? 10 : 1);
              void move.commit({ xMeters: event.key === "ArrowLeft" ? -step : event.key === "ArrowRight" ? step : 0, yMeters: event.key === "ArrowDown" ? -step : event.key === "ArrowUp" ? step : 0, zMeters: 0 });
            }
            return;
          }
        }
        if (event.key === "Escape") { event.preventDefault(); cancel(); fixture.key("Escape"); }
        else if (event.key === "Home") { event.preventDefault(); if (!pointerActive.current) spatial.reset(); }
        else if (fixtureTools !== undefined && fixture.key(event.key)) event.preventDefault();
        if (["Shift", "Control", "Meta"].includes(event.key)) refreshModifiers(event);
      }}
      onKeyUp={(event) => { if (["Shift", "Control", "Meta"].includes(event.key)) refreshModifiers(event); }}
      onPointerDown={(event) => {
        if (pending || (event.button !== 0 && event.button !== 1)) return;
        event.preventDefault();
        pointerActive.current = true; lastPointer.current = { clientX: event.clientX, clientY: event.clientY };
        event.currentTarget.setPointerCapture(event.pointerId);
        event.currentTarget.focus();
        const rect = event.currentTarget.getBoundingClientRect();
        const world = unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view);
        if (event.button === 0 && !event.altKey && (fixtureTools?.tool !== null || !(event.ctrlKey || event.metaKey)) && fixture.down(world, event)) return;
        if (event.button === 1 || event.altKey) { gesture.current = { type: "pan", x: event.clientX, y: event.clientY }; return; }
        const index = nearestPoint(points, world, THEME_METRICS.spatialHitRadius / spatial.view.scale);
        const owner = index === null ? null : plan.pixels[index]?.owner ?? null;
        const additive = event.shiftKey || event.ctrlKey || event.metaKey;
        if (owner === null) {
          const point = { xMeters: world.x, yMeters: world.y, zMeters: world.z };
          gesture.current = { type: "box", start: point, end: point, selection: additive ? selection : [] };
          setDragging(true); return;
        }
        const selectedRoot = chosen.find((item) => item.owners.includes(owner));
        const anchor = selectedRoot?.id ?? owner;
        const moveSelection = selectedRoot === undefined ? additive ? [...selection, owner] : [owner] : selection;
        const clickSelection = selectionClick(selection, anchor, additive);
        onSelect(moveSelection);
        const commit = onMoveStart(moveSelection, anchor);
        if (commit === null) onSelect(clickSelection);
        setDragging(commit !== null);
        gesture.current = commit === null ? null : { type: "move", clickSelection, x: event.clientX, y: event.clientY, scale: spatial.view.scale, commit, pixels: plan.pixels.filter((pixel) => commit.owners.includes(pixel.owner)) };
      }}
      onPointerMove={(event) => {
        lastPointer.current = { clientX: event.clientX, clientY: event.clientY };
        const rect = event.currentTarget.getBoundingClientRect();
        if (gesture.current === null && fixture.move(unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view), event)) return;
        const active = gesture.current;
        if (active === null) return;
        if (active.type === "pan") {
          spatial.panBy(event.clientX - active.x, event.clientY - active.y);
          gesture.current = { type: "pan", x: event.clientX, y: event.clientY };
        } else if (active.type === "box") {
          const world = unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view);
          active.end = { xMeters: world.x, yMeters: world.y, zMeters: world.z };
          setMarquee(boxFromPoints(active.start, active.end));
        } else {
          const delta = moveDelta(active, event, event);
          setOffset({ owners: active.commit.owners, x: delta.xMeters, y: delta.yMeters, pixels: active.pixels });
        }
      }}
      onPointerUp={(event) => {
        pointerActive.current = false; setDragging(false);
        const rect = event.currentTarget.getBoundingClientRect();
        if (gesture.current === null && fixture.up(unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view), event)) return;
        const active = gesture.current;
        gesture.current = null;
        if (active?.type === "box") {
          const world = unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view);
          const box = boxFromPoints(active.start, { xMeters: world.x, yMeters: world.y, zMeters: world.z });
          onSelect([...new Set([...active.selection, ...boxSelection(items, box)])]);
          setMarquee(null);
        }
        if (active?.type === "move") {
          const delta = moveDelta(active, event, event);
          if (delta.xMeters !== 0 || delta.yMeters !== 0) {
            setOffset({ owners: active.commit.owners, x: delta.xMeters, y: delta.yMeters, pixels: active.pixels });
            void active.commit.commit(delta).then((committed) => { if (!committed) setOffset(null); });
          }
          else { setOffset(null); onSelect(active.clickSelection); }
        }
      }}
      onPointerCancel={cancel}
      onLostPointerCapture={() => { if (pointerActive.current) cancel(); }}
      onWheel={(event) => {
        event.preventDefault();
        if (gesture.current !== null || fixture.active) return;
        const rect = event.currentTarget.getBoundingClientRect();
        spatial.zoomAt(Math.exp(-event.deltaY * THEME_METRICS.spatialWheelZoomScale), event.clientX - rect.left, event.clientY - rect.top);
      }}
      onContextMenu={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        const point = unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view);
        if (fixtureTools !== undefined || layoutMenu !== undefined) {
          const index = nearestPoint(points, point, THEME_METRICS.spatialHitRadius / spatial.view.scale);
          const owner = index === null ? null : plan.pixels[index]?.owner ?? null;
          if (owner !== null && !selectedOwners.includes(owner)) onSelect([owner]);
        }
        if (layoutMenu === undefined) return;
        setMenuPosition(snapPoint({ xMeters: point.x, yMeters: point.y, zMeters: point.z }, snapping, event, targets, guides.guides, THEME_METRICS.spatialHitRadius / spatial.view.scale));
      }}
    />;
  return <div className="spatial-canvas-shell">
    {layoutMenu === undefined ? fixtureTools === undefined ? canvasElement : <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{canvasElement}</ContextMenu.Trigger>
      <ContextMenu.Portal><ContextMenu.Content className="menu-content"><FixtureContextMenu enabled={fixtureTools.enabled} onTool={fixtureTools.onTool} selected={selection.length > 0} onDuplicate={fixtureTools.onDuplicate} onDelete={fixtureTools.onDelete} /></ContextMenu.Content></ContextMenu.Portal>
    </ContextMenu.Root> : <ContextMenu.Root onOpenChange={(open) => { if (!open) setMenuPosition(null); }}>
      <ContextMenu.Trigger asChild disabled={!layoutMenu.enabled}>{canvasElement}</ContextMenu.Trigger>
      {menuPosition !== null && <ContextMenu.Portal><ContextMenu.Content className="menu-content">
        <LayoutAddMenu
          availableFixtures={layoutMenu.availableFixtures}
          enabled={layoutMenu.enabled}
          onAddFixture={(fixture) => { layoutMenu.onAddFixture(fixture, menuPosition); }}
          onCreateFixture={() => { layoutMenu.onCreateFixture(menuPosition); }}
          onAddGroup={layoutMenu.onAddGroup}
        />
        {selection.length > 0 && <><ContextMenu.Item className="menu-item" disabled={!layoutMenu.enabled} onSelect={onDuplicate}>Duplicate selection</ContextMenu.Item><ContextMenu.Item className="menu-item danger" disabled={!layoutMenu.enabled} onSelect={onDelete}>Delete selection</ContextMenu.Item></>}
      </ContextMenu.Content></ContextMenu.Portal>}
    </ContextMenu.Root>}
    <SpatialSnapControls settings={snapping} guides={guides.guides} onGuides={guides.save} disabled={dragging || fixture.active || pending} />
    {(fixture.measurement !== null || dragging && offset !== null) && <output className="spatial-measurement">{fixture.measurement ?? (offset === null ? "" : `X ${formatDistance(offset.x, snapping.unit)} | Y ${formatDistance(offset.y, snapping.unit)}`)}</output>}
    {guides.error !== null && <p className="spatial-measurement" role="alert">{guides.error}</p>}
    <SpatialControls view={spatial.view} reset={spatial.reset} zoomAt={spatial.zoomAt} />
  </div>;
}

function offsetIsCommitted(offset: Offset, pixels: SpatialRenderPixel[]) {
  const byIndex = new Map(pixels.filter((pixel) => offset.owners.includes(pixel.owner)).map((pixel) => [`${pixel.owner}:${pixel.index}`, pixel]));
  return offset.pixels.length > 0 && offset.pixels.every((pixel) => {
    const current = byIndex.get(`${pixel.owner}:${pixel.index}`);
    return current !== undefined
      && closeEnough(current.position.xMeters, pixel.position.xMeters + offset.x)
      && closeEnough(current.position.yMeters, pixel.position.yMeters + offset.y);
  });
}

function closeEnough(left: number, right: number) {
  return Math.abs(left - right) < 0.0001;
}

function boxFromPoints(a: Point3Meters, b: Point3Meters): Box {
  return { left: Math.min(a.xMeters, b.xMeters), right: Math.max(a.xMeters, b.xMeters), bottom: Math.min(a.yMeters, b.yMeters), top: Math.max(a.yMeters, b.yMeters) };
}

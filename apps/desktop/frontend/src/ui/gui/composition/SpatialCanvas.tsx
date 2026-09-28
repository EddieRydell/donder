import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import * as ContextMenu from "@radix-ui/react-context-menu";
import type { GuiObjectRef, Point3Meters, SpatialRenderPixel, SpatialRenderPlan } from "../../../types";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import { drawSpatialCanvas, nearestPoint, normalizeBounds, normalizePoint, unproject } from "../shared";
import { SpatialControls, useSpatialViewport } from "../SpatialViewport";
import { LayoutAddMenu } from "./LayoutAddMenu";
import { FixtureContextMenu } from "./FixtureContextMenu";
import { useFixtureCanvasTools, type FixtureCanvasTools } from "./fixtureCanvasTools";

type Move = (delta: Point3Meters) => Promise<boolean>;
type Gesture = { type: "pan"; x: number; y: number } | { type: "move"; owner: number; x: number; y: number; scale: number; commit: Move; pixels: SpatialRenderPixel[] };
type Offset = { owner: number; x: number; y: number; pixels: SpatialRenderPixel[] };
type LayoutMenu = {
  availableFixtures: GuiObjectRef[];
  enabled: boolean;
  onAddFixture: (definition: GuiObjectRef, position: Point3Meters) => void;
  onCreateFixture: (position: Point3Meters) => void;
  onAddGroup: () => void;
};

export function SpatialCanvas({ plan, documentKey, selected, onSelect, onMoveStart, layoutMenu, fixtureTools }: { plan: SpatialRenderPlan; documentKey: string; selected: number | null; onSelect: (owner: number | null) => void; onMoveStart: (owner: number) => Move | null; layoutMenu?: LayoutMenu; fixtureTools?: FixtureCanvasTools }) {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const gesture = useRef<Gesture | null>(null);
  const settledOffsets = useRef(new WeakSet<Offset>());
  const [offset, setOffset] = useState<Offset | null>(null);
  const [menuPosition, setMenuPosition] = useState<Point3Meters | null>(null);
  const bounds = useMemo(() => normalizeBounds(plan.bounds), [plan.bounds]);
  const spatial = useSpatialViewport(bounds, documentKey, documentKey);
  const fixture = useFixtureCanvasTools(fixtureTools, selected, spatial.view.scale, plan);
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
        plan.pixels.forEach((pixel) => {
          const position = normalizePoint(pixel.position);
          if (displayOffset?.owner === pixel.owner) { position.x += displayOffset.x; position.y += displayOffset.y; }
          const point = project(position);
          const radius = Math.max(THEME_METRICS.spatialPointRadius, pixel.diameterMeters * spatial.view.scale / 2);
          context.fillStyle = selected === pixel.owner ? THEME_COLORS.layoutSelected : THEME_COLORS.playhead;
          context.beginPath(); context.arc(point.x, point.y, radius, 0, Math.PI * 2); context.fill();
        });
        const selectedOffset = displayOffset?.owner === selected ? displayOffset : null;
        fixture.draw(context, selectedOffset === null ? project : (point) => project({ ...point, x: point.x + selectedOffset.x, y: point.y + selectedOffset.y }));
      }, spatial.view);
    };
    draw();
    const observer = new ResizeObserver(draw);
    observer.observe(element);
    return () => { observer.disconnect(); };
  }, [bounds, plan, selected, spatial, offset, fixture]);
  const canvasElement = <canvas ref={canvas} className="gui-canvas" tabIndex={0} aria-label="Fixture pixels"
      onKeyDown={(event) => { if (event.key === "Home") { event.preventDefault(); spatial.reset(); } else if (fixtureTools !== undefined && fixture.key(event.key)) event.preventDefault(); }}
      onPointerDown={(event) => {
        if (event.button !== 0 && event.button !== 1) return;
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        event.currentTarget.focus();
        const rect = event.currentTarget.getBoundingClientRect();
        const world = unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view);
        if (event.button === 0 && !event.altKey && fixture.down(world)) return;
        const index = nearestPoint(points, world, THEME_METRICS.spatialHitRadius / spatial.view.scale);
        const owner = index === null ? null : plan.pixels[index]?.owner ?? null;
        onSelect(owner);
        const commit = owner !== null && event.button === 0 && !event.altKey ? onMoveStart(owner) : null;
        gesture.current = commit !== null && owner !== null
          ? { type: "move", owner, x: event.clientX, y: event.clientY, scale: spatial.view.scale, commit, pixels: plan.pixels.filter((pixel) => pixel.owner === owner) }
          : { type: "pan", x: event.clientX, y: event.clientY };
      }}
      onPointerMove={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        if (gesture.current === null && fixture.move(unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view))) return;
        const active = gesture.current;
        if (active === null) return;
        if (active.type === "pan") {
          spatial.panBy(event.clientX - active.x, event.clientY - active.y);
          gesture.current = { type: "pan", x: event.clientX, y: event.clientY };
        } else {
          setOffset({ owner: active.owner, x: (event.clientX - active.x) / active.scale, y: (active.y - event.clientY) / active.scale, pixels: active.pixels });
        }
      }}
      onPointerUp={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        if (gesture.current === null && fixture.up(unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view))) return;
        const active = gesture.current;
        gesture.current = null;
        if (active?.type === "move") {
          const delta = { xMeters: (event.clientX - active.x) / active.scale, yMeters: (active.y - event.clientY) / active.scale, zMeters: 0 };
          if (delta.xMeters !== 0 || delta.yMeters !== 0) void active.commit(delta).then((committed) => { if (!committed) setOffset(null); });
          else setOffset(null);
        }
      }}
      onPointerCancel={() => { gesture.current = null; setOffset(null); fixture.cancel(); }}
      onWheel={(event) => {
        event.preventDefault();
        if (gesture.current !== null || fixture.active) return;
        const rect = event.currentTarget.getBoundingClientRect();
        spatial.zoomAt(Math.exp(-event.deltaY * THEME_METRICS.spatialWheelZoomScale), event.clientX - rect.left, event.clientY - rect.top);
      }}
      onContextMenu={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        const point = unproject(event.clientX - rect.left, event.clientY - rect.top, canvas.current, bounds, spatial.view);
        if (fixtureTools !== undefined) {
          const index = nearestPoint(points, point, THEME_METRICS.spatialHitRadius / spatial.view.scale);
          onSelect(index === null ? null : plan.pixels[index]?.owner ?? null);
        }
        if (layoutMenu === undefined) return;
        setMenuPosition({ xMeters: point.x, yMeters: point.y, zMeters: point.z });
      }}
    />;
  return <div className="spatial-canvas-shell">
    {layoutMenu === undefined ? fixtureTools === undefined ? canvasElement : <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{canvasElement}</ContextMenu.Trigger>
      <ContextMenu.Portal><ContextMenu.Content className="menu-content"><FixtureContextMenu enabled={fixtureTools.enabled} onTool={fixtureTools.onTool} selected={selected !== null} onDuplicate={fixtureTools.onDuplicate} onDelete={fixtureTools.onDelete} /></ContextMenu.Content></ContextMenu.Portal>
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
      </ContextMenu.Content></ContextMenu.Portal>}
    </ContextMenu.Root>}
    <SpatialControls view={spatial.view} reset={spatial.reset} zoomAt={spatial.zoomAt} />
  </div>;
}

function offsetIsCommitted(offset: Offset, pixels: SpatialRenderPixel[]) {
  const byIndex = new Map(pixels.filter((pixel) => pixel.owner === offset.owner).map((pixel) => [pixel.index, pixel]));
  return offset.pixels.length > 0 && offset.pixels.every((pixel) => {
    const current = byIndex.get(pixel.index);
    return current !== undefined
      && closeEnough(current.position.xMeters, pixel.position.xMeters + offset.x)
      && closeEnough(current.position.yMeters, pixel.position.yMeters + offset.y);
  });
}

function closeEnough(left: number, right: number) {
  return Math.abs(left - right) < 0.0001;
}

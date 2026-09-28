import type { GuiFixtureElement, GuiLayoutFixture, Point3Meters, SpatialRenderPlan } from "../../../types";

export type Box = { left: number; right: number; bottom: number; top: number };
export type SpatialItem = { id: number; owners: number[]; descendants: number[]; origin: Point3Meters; bounds: Box };
export type SpatialMove = { id: number; delta: Point3Meters };
export type Arrangement = "left" | "centerX" | "right" | "bottom" | "centerY" | "top" | "spaceX" | "spaceY";
export const plus = (point: Point3Meters, delta: Point3Meters): Point3Meters => ({ xMeters: point.xMeters + delta.xMeters, yMeters: point.yMeters + delta.yMeters, zMeters: point.zMeters + delta.zMeters });
export const selectionClick = (selected: number[], id: number, additive: boolean) => additive ? selected.includes(id) ? selected.filter((item) => item !== id) : [...selected, id] : [id];
export function unionBounds(boxes: Box[]): Box {
  if (boxes.length === 0) throw new Error("Select at least one object.");
  const first = boxes[0];
  if (first === undefined) throw new Error("Select at least one object.");
  return boxes.reduce((result, box) => ({ left: Math.min(result.left, box.left), right: Math.max(result.right, box.right), bottom: Math.min(result.bottom, box.bottom), top: Math.max(result.top, box.top) }), first);
}
function pixelBounds(plan: SpatialRenderPlan): Map<number, Box> {
  const result = new Map<number, Box>();
  for (const pixel of plan.pixels) {
    const p = pixel.position; const radius = pixel.diameterMeters / 2;
    const box = { left: p.xMeters - radius, right: p.xMeters + radius, bottom: p.yMeters - radius, top: p.yMeters + radius };
    const previous = result.get(pixel.owner);
    result.set(pixel.owner, previous === undefined ? box : unionBounds([previous, box]));
  }
  return result;
}
function item(id: number, origin: Point3Meters, boxes: Map<number, Box>): SpatialItem {
  return { id, owners: [id], descendants: [], origin, bounds: boxes.get(id) ?? { left: origin.xMeters, right: origin.xMeters, bottom: origin.yMeters, top: origin.yMeters } };
}
export function fixtureItems(elements: GuiFixtureElement[], plan: SpatialRenderPlan): SpatialItem[] {
  const boxes = pixelBounds(plan);
  return elements.map((element) => item(element.id, element.transform.position, boxes));
}
export function layoutItems(fixtures: GuiLayoutFixture[], plan: SpatialRenderPlan): SpatialItem[] {
  const boxes = pixelBounds(plan);
  const visit = (fixtures: GuiLayoutFixture[]): SpatialItem[] => fixtures.flatMap((fixture) => {
    if (fixture.kind.type === "fixture") return [item(fixture.id, fixture.kind.transform.position, boxes)];
    const children = visit(fixture.kind.children);
    const owners = [...new Set(children.flatMap((child) => child.owners))];
    const bounds = children.length === 0 ? { left: 0, right: 0, bottom: 0, top: 0 } : unionBounds(children.map((child) => child.bounds));
    return [{ id: fixture.id, owners, descendants: children.map((child) => child.id), origin: { xMeters: bounds.left, yMeters: bounds.bottom, zMeters: 0 }, bounds }, ...children];
  });
  return visit(fixtures);
}
/** Prefer selected ancestor groups; never move a descendant twice. */
export function selectedItems(items: SpatialItem[], selection: number[]): SpatialItem[] {
  const selected = items.filter((item) => selection.includes(item.id));
  return selected.filter((item) => !selected.some((other) => other !== item && other.descendants.includes(item.id)));
}
export function boxSelection(items: SpatialItem[], box: Box): number[] {
  return items.filter((item) => item.owners.length === 1 && item.owners[0] === item.id && item.bounds.left >= box.left && item.bounds.right <= box.right && item.bounds.bottom >= box.bottom && item.bounds.top <= box.top).map((item) => item.id);
}
export function arrange(items: SpatialItem[], action: Arrangement): SpatialMove[] {
  if (items.length < 2) return [];
  const bounds = unionBounds(items.map((item) => item.bounds));
  const delta = (id: number, x: number, y: number) => ({ id, delta: { xMeters: x, yMeters: y, zMeters: 0 } });
  if (action === "spaceX" || action === "spaceY") {
    if (items.length < 3) return [];
    const horizontal = action === "spaceX";
    const start = (box: Box) => horizontal ? box.left : box.bottom;
    const end = (box: Box) => horizontal ? box.right : box.top;
    const sorted = [...items].sort((a, b) => start(a.bounds) - start(b.bounds));
    const extent = end(bounds) - start(bounds);
    const sizes = sorted.map((item) => end(item.bounds) - start(item.bounds));
    const gap = (extent - sizes.reduce((sum, size) => sum + size, 0)) / (items.length - 1);
    let position = start(bounds);
    return sorted.map((item, index) => { const distance = position - start(item.bounds); position += (sizes[index] ?? 0) + gap; return delta(item.id, horizontal ? distance : 0, horizontal ? 0 : distance); });
  }
  return items.map((item) => {
    const box = item.bounds;
    switch (action) {
      case "left": return delta(item.id, bounds.left - box.left, 0);
      case "right": return delta(item.id, bounds.right - box.right, 0);
      case "centerX": return delta(item.id, (bounds.left + bounds.right - box.left - box.right) / 2, 0);
      case "bottom": return delta(item.id, 0, bounds.bottom - box.bottom);
      case "top": return delta(item.id, 0, bounds.top - box.top);
      case "centerY": return delta(item.id, 0, (bounds.bottom + bounds.top - box.bottom - box.top) / 2);
    }
  });
}
export function repeatOffsets(rows: number, columns: number, x: number, y: number): Point3Meters[] {
  if (![rows, columns].every((value) => Number.isInteger(value) && value >= 1) || rows * columns > 1001 || !Number.isFinite(x) || !Number.isFinite(y)) throw new Error("Use positive row and column counts, at most 1,000 copies, and finite spacing.");
  return Array.from({ length: rows * columns }, (_, index) => ({ xMeters: index % columns * x + 0, yMeters: Math.floor(index / columns) * y + 0, zMeters: 0 })).slice(1);
}
export function moveLayout(fixtures: GuiLayoutFixture[], moves: SpatialMove[]): GuiLayoutFixture[] {
  const byOwner = new Map(moves.map((move) => [move.id, move.delta]));
  const visit = (items: GuiLayoutFixture[], inherited?: Point3Meters): GuiLayoutFixture[] => items.map((item) => {
    const delta = inherited ?? byOwner.get(item.id);
    return { ...item, kind: item.kind.type === "group" ? { ...item.kind, children: visit(item.kind.children, delta) } : delta === undefined ? item.kind : { ...item.kind, transform: { ...item.kind.transform, position: plus(item.kind.transform.position, delta) } } };
  });
  return visit(fixtures);
}

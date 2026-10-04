import type { Point3Meters, SpatialGuide, SpatialSnapSettings, SpatialUnit } from "../../../editor/types";

export type Modifiers = { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean };
export const spatialUnits: Record<SpatialUnit, { label: string; meters: number }> = {
  meters: { label: "m", meters: 1 }, centimeters: { label: "cm", meters: 0.01 },
  millimeters: { label: "mm", meters: 0.001 }, inches: { label: "in", meters: 0.0254 }, feet: { label: "ft", meters: 0.3048 }
};
const directions = [[1, 0], [1, 1], [0, 1], [-1, 1], [-1, 0], [-1, -1], [0, -1], [1, -1]] as const;
const clean = (value: number) => Number(value.toPrecision(12));

/** A Shift constraint takes precedence over the absolute grid for off-grid anchors. */
export function constrainPoint(point: Point3Meters, settings: SpatialSnapSettings, modifiers: Modifiers, anchor?: Point3Meters, square = false): Point3Meters {
  const snap = settings.enabled && !modifiers.ctrlKey && !modifiers.metaKey;
  const step = settings.spacingMeters;
  if (!Number.isFinite(step) || step <= 0) throw new Error("Snap distance must be positive and finite.");
  const round = (value: number) => snap ? clean(Math.round(value / step) * step) : value;
  if (modifiers.shiftKey && anchor !== undefined) {
    const x = point.xMeters - anchor.xMeters; const y = point.yMeters - anchor.yMeters;
    const octant = ((Math.round(Math.atan2(y, x) / (Math.PI / 4)) % 8) + 8) % 8;
    const direction: readonly [number, number] | undefined = square ? [x < 0 ? -1 : 1, y < 0 ? -1 : 1] : directions[octant];
    if (direction === undefined) throw new Error("Invalid angle constraint.");
    const [dx, dy] = direction;
    const distance = round((x * dx + y * dy) / (dx * dx + dy * dy));
    return { xMeters: clean(anchor.xMeters + distance * dx), yMeters: clean(anchor.yMeters + distance * dy), zMeters: point.zMeters };
  }
  return { xMeters: round(point.xMeters), yMeters: round(point.yMeters), zMeters: point.zMeters };
}

export function constrainedMove(origin: Point3Meters, delta: Point3Meters, settings: SpatialSnapSettings, modifiers: Modifiers): Point3Meters {
  if (delta.xMeters === 0 && delta.yMeters === 0) return delta;
  const point = constrainPoint({ xMeters: origin.xMeters + delta.xMeters, yMeters: origin.yMeters + delta.yMeters, zMeters: origin.zMeters }, settings, modifiers, origin);
  return { xMeters: clean(point.xMeters - origin.xMeters), yMeters: clean(point.yMeters - origin.yMeters), zMeters: 0 };
}

export function visibleGridStep(spacing: number, scale: number, minimumPixels: number): number {
  return spacing * Math.max(1, 10 ** Math.ceil(Math.log10(minimumPixels / (spacing * scale))));
}

export type SnapTarget = { owner: number; position: Point3Meters };
export function snapPoint(point: Point3Meters, settings: SpatialSnapSettings, modifiers: Modifiers, targets: SnapTarget[], guides: SpatialGuide[], radius: number, anchor?: Point3Meters, square = false): Point3Meters {
  const constrained = constrainPoint(point, settings, modifiers, anchor, square);
  if (!settings.enabled || modifiers.ctrlKey || modifiers.metaKey) return constrained;
  const distance = (a: Point3Meters, b: Point3Meters) => Math.hypot(a.xMeters - b.xMeters, a.yMeters - b.yMeters);
  const onConstraint = (candidate: Point3Meters) => {
    if (!modifiers.shiftKey || anchor === undefined) return true;
    const projected = constrainPoint(candidate, { ...settings, enabled: false }, modifiers, anchor, square);
    return distance(candidate, projected) < 1e-8;
  };
  let best = radius; let endpoint: Point3Meters | undefined;
  for (const target of targets) {
    const candidate = { ...target.position, zMeters: point.zMeters };
    const d = distance(point, candidate);
    if (d < best && onConstraint(candidate)) { best = d; endpoint = candidate; }
  }
  if (endpoint !== undefined) return endpoint;
  let result = constrained;
  for (const axis of ["x", "y"] as const) {
    let nearest = radius; let candidate: Point3Meters | undefined;
    for (const guide of guides) {
      if (guide.axis !== axis) continue;
      const d = Math.abs((axis === "x" ? point.xMeters : point.yMeters) - guide.positionMeters);
      if (d >= nearest) continue;
      let snapped = axis === "x" ? { ...result, xMeters: guide.positionMeters } : { ...result, yMeters: guide.positionMeters };
      if (modifiers.shiftKey && anchor !== undefined) {
        const dx = constrained.xMeters - anchor.xMeters; const dy = constrained.yMeters - anchor.yMeters;
        const component = axis === "x" ? dx : dy;
        if (Math.abs(component) < 1e-12) continue;
        const ratio = (guide.positionMeters - (axis === "x" ? anchor.xMeters : anchor.yMeters)) / component;
        snapped = { ...result, xMeters: anchor.xMeters + dx * ratio, yMeters: anchor.yMeters + dy * ratio };
        if (distance(point, snapped) > radius) continue;
      }
      nearest = d; candidate = snapped;
    }
    if (candidate !== undefined) result = candidate;
  }
  return result;
}
export function formatDistance(meters: number, unit: SpatialUnit): string {
  const spec = spatialUnits[unit];
  return `${Number((meters / spec.meters).toPrecision(6))} ${spec.label}`;
}

import { THEME_METRICS } from "../../../theme";

/** Sorted mark times of the visible collections; Alt-drags snap to these. */
export function markSnapTimes(collections: Array<{ marksSeconds: number[] }>): number[] {
  return collections.flatMap((collection) => collection.marksSeconds).sort((left, right) => left - right);
}

/** The mark nearest `seconds`, when it is within the snap distance on screen. */
export function snapToMark(seconds: number, marks: number[], pxPerSecond: number): number | null {
  let nearest: number | null = null;
  for (const mark of marks) {
    if (nearest === null || Math.abs(mark - seconds) < Math.abs(nearest - seconds)) nearest = mark;
  }
  return nearest !== null && Math.abs(nearest - seconds) * pxPerSecond <= THEME_METRICS.sequenceMarkSnapDistance ? nearest : null;
}

/** Adjust a drag delta so the moving edge closest to a mark lands on it. */
export function snapDeltaToMarks(edges: number[], deltaSeconds: number, marks: number[], pxPerSecond: number): number {
  let snapped = deltaSeconds;
  let nearestDistance = Infinity;
  for (const edge of edges) {
    const mark = snapToMark(edge + deltaSeconds, marks, pxPerSecond);
    if (mark === null) continue;
    const distance = Math.abs(mark - edge - deltaSeconds);
    if (distance < nearestDistance) {
      nearestDistance = distance;
      snapped = mark - edge;
    }
  }
  return snapped;
}

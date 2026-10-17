import { THEME_METRICS } from "../../../theme";

/** Sorted mark times of the visible collections; timeline drags snap to these. */
export function markSnapTimes(collections: Array<{ marksSeconds: number[] }>): number[] {
  return collections.flatMap((collection) => collection.marksSeconds).sort((left, right) => left - right);
}

/** Start and end times of `clips`; a clip dragged into their row snaps to these as to marks. */
export function clipEdgeSnapTimes(clips: Array<{ startSeconds: number; durationSeconds: number }>): number[] {
  return clips.flatMap((clip) => [clip.startSeconds, clip.startSeconds + clip.durationSeconds]);
}

/** The time nearest `seconds`, when it is within the snap distance on screen. */
export function snapToTime(seconds: number, times: number[], pxPerSecond: number): number | null {
  let nearest: number | null = null;
  for (const time of times) {
    if (nearest === null || Math.abs(time - seconds) < Math.abs(nearest - seconds)) nearest = time;
  }
  return nearest !== null && Math.abs(nearest - seconds) * pxPerSecond <= THEME_METRICS.sequenceSnapDistance ? nearest : null;
}

/** Adjust a drag delta so the moving edge closest to a snap time lands on it. */
export function snapDeltaToTimes(edges: number[], deltaSeconds: number, times: number[], pxPerSecond: number): number {
  let snapped = deltaSeconds;
  let nearestDistance = Infinity;
  for (const edge of edges) {
    const time = snapToTime(edge + deltaSeconds, times, pxPerSecond);
    if (time === null) continue;
    const distance = Math.abs(time - edge - deltaSeconds);
    if (distance < nearestDistance) {
      nearestDistance = distance;
      snapped = time - edge;
    }
  }
  return snapped;
}

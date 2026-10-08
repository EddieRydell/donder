import type { FixtureTarget, SequenceEditorDocument } from "../../../editor/types";

export function targetsEqual(left: FixtureTarget, right: FixtureTarget) {
  return left.fixture === right.fixture;
}

export function targetAtLane(document: SequenceEditorDocument, index: number): FixtureTarget {
  const lane = document.lanes[index];
  if (lane === undefined) throw new Error("Timeline target is missing.");
  return lane.target;
}

/** Every lane showing `target`. A member of several groups has a lane under each, and
 * every copy shows the same clips. */
export function lanesForTarget(document: SequenceEditorDocument, target: FixtureTarget): number[] {
  return document.lanes.flatMap((lane, index) => targetsEqual(lane.target, target) ? [index] : []);
}

/** The lane of `target` nearest `anchor`, the earlier on a tie; -1 when it has none.
 * Vertical moves start from this lane, as the backend does. */
export function nearestLane(document: SequenceEditorDocument, target: FixtureTarget, anchor: number): number {
  let nearest = -1;
  for (const lane of lanesForTarget(document, target)) {
    if (nearest < 0 || Math.abs(lane - anchor) < Math.abs(nearest - anchor)) nearest = lane;
  }
  return nearest;
}

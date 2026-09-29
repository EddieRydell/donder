import type { FixtureTarget, SequenceEditorDocument } from "../../../types";

export function targetsEqual(left: FixtureTarget, right: FixtureTarget) {
  return left.fixture === right.fixture;
}

export function targetAtLane(document: SequenceEditorDocument, index: number): FixtureTarget {
  const lane = document.lanes[index];
  if (lane === undefined) throw new Error("Timeline target is missing.");
  return lane.target;
}

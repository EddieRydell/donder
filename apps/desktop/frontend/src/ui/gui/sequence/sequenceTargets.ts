import type { ElementTarget, SequenceControlTarget } from "../../../types";

export function targetsEqual(left: ElementTarget, right: ElementTarget) {
  return left.kind === right.kind && left.name === right.name;
}

export function sameControlChannel(left: SequenceControlTarget, right: SequenceControlTarget): boolean {
  return left.type === right.type && left.node === right.node && (left.type !== "fixtureFunction" || right.type === "fixtureFunction" && left.function === right.function);
}

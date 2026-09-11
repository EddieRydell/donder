import type { FixtureTarget } from "../../../types";

export function targetsEqual(left: FixtureTarget, right: FixtureTarget) {
  return left.fixture === right.fixture;
}

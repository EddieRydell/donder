import assert from "node:assert/strict";
import test from "node:test";

import { graphOperatorDefinition, graphOperatorKey } from "./graphOperator.ts";

const definition = (path, displayName) => ({
  operator: { type: "custom", path, objectKey: "Gain" },
  sourceName: "Gain",
  displayName,
  inputs: [],
  outputs: [],
  params: []
});

test("custom operator identity includes its declaring document", () => {
  const first = definition("operators/first.operator.donder", "First Gain");
  const second = definition("operators/second.operator.donder", "Second Gain");

  assert.notEqual(graphOperatorKey(first.operator), graphOperatorKey(second.operator));
  assert.equal(
    graphOperatorDefinition([first, second], second.operator).displayName,
    "Second Gain"
  );
});

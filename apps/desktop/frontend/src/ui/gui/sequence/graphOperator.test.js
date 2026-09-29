import assert from "node:assert/strict";
import test from "node:test";

import { graphOperatorDefinition, graphOperatorKey } from "./graphOperator.ts";

const definition = (path, displayName) => ({
  operator: { type: "custom", moduleId: "project", path, objectKey: "Gain" },
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
  assert.equal(graphOperatorKey(second.operator), "custom:project:operators/second.operator.donder:Gain");
  assert.equal(
    graphOperatorDefinition([first, second], second.operator).displayName,
    "Second Gain"
  );
});

test("operator lookup rejects a reference absent from the project catalog", () => {
  const available = definition("operators/first.operator.donder", "First Gain");
  const missing = definition("operators/second.operator.donder", "Second Gain");

  assert.throws(
    () => graphOperatorDefinition([available], missing.operator),
    /Missing graph operator catalog entry/
  );
});

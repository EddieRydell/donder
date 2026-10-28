import assert from "node:assert/strict";
import test from "node:test";

import { GRAPH_NEUTRAL_EDGE_COLOR, graphEdgeId, graphEdgeLineages, parseGraphEdgeId } from "./graphEdge.ts";

test("edge ids roundtrip and reject malformed input", () => {
  const edge = { fromNode: "layer:1", fromPort: "output", toNode: "root", toPort: "input" };
  assert.deepEqual(parseGraphEdgeId(graphEdgeId(edge)), edge);
  assert.equal(parseGraphEdgeId('["only", "three", "parts"]'), null);
  assert.equal(parseGraphEdgeId("not json"), null);
});

test("edge lineage reports single, mixed, and absent upstream layers", () => {
  const graph = {
    nodes: [
      { id: "layer:1", x: 0, y: 0, inputs: [], outputs: [], kind: { type: "layer", layerId: 1, layerName: "One", layerColor: "layer-one", enabled: true } },
      { id: "layer:2", x: 0, y: 0, inputs: [], outputs: [], kind: { type: "layer", layerId: 2, layerName: "Two", layerColor: "layer-two", enabled: true } },
      { id: "mix", x: 0, y: 0, inputs: [], outputs: [], kind: { type: "root" } },
      { id: "orphan", x: 0, y: 0, inputs: [], outputs: [], kind: { type: "root" } }
    ],
    edges: [
      { fromNode: "layer:1", fromPort: "output", toNode: "mix", toPort: "a" },
      { fromNode: "layer:2", fromPort: "output", toNode: "mix", toPort: "b" },
      { fromNode: "mix", fromPort: "output", toNode: "orphan", toPort: "input" },
      { fromNode: "empty", fromPort: "output", toNode: "sink", toPort: "input" }
    ]
  };
  const lineages = graphEdgeLineages(graph);

  assert.equal(lineages.get(graphEdgeId(graph.edges[0])).color, "layer-one");
  assert.equal(lineages.get(graphEdgeId(graph.edges[2])).color, GRAPH_NEUTRAL_EDGE_COLOR);
  assert.equal(lineages.get(graphEdgeId(graph.edges[3])).label, "No upstream layer");
});

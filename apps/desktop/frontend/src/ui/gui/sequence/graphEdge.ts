import type { SequenceCompositionGraph, SequenceGraphEdge } from "../../../types";

export const GRAPH_NEUTRAL_EDGE_COLOR = "var(--donder-graph-edge-neutral)";

export type GraphEdgeIdParts = {
  fromNode: string;
  fromPort: string;
  toNode: string;
  toPort: string;
};

export type GraphEdgeLineage = {
  color: string;
  label: string;
};

export function graphEdgeId(edge: GraphEdgeIdParts) {
  return JSON.stringify([edge.fromNode, edge.fromPort, edge.toNode, edge.toPort]);
}

export function parseGraphEdgeId(edgeId: string): GraphEdgeIdParts | null {
  try {
    const value: unknown = JSON.parse(edgeId);
    if (!Array.isArray(value) || value.length !== 4 || !value.every((part) => typeof part === "string")) {
      return null;
    }
    const [fromNode, fromPort, toNode, toPort] = value as [string, string, string, string];
    return { fromNode, fromPort, toNode, toPort };
  } catch {
    return null;
  }
}

export function graphEdgeLineages(graph: SequenceCompositionGraph) {
  const incoming = new Map<string, SequenceGraphEdge[]>();
  const layerColors = new Map<string, string>();
  for (const node of graph.nodes) {
    if (node.kind.type === "layer") layerColors.set(node.id, node.kind.layerColor);
  }
  for (const edge of graph.edges) {
    const existing = incoming.get(edge.toNode) ?? [];
    existing.push(edge);
    incoming.set(edge.toNode, existing);
  }

  const result = new Map<string, GraphEdgeLineage>();
  for (const edge of graph.edges) {
    const colors = traceUpstreamLayerColors(edge.fromNode, incoming, layerColors, new Set([edge.toNode]));
    if (colors.size === 1) {
      const [color] = Array.from(colors) as [string];
      result.set(graphEdgeId(edge), { color, label: "Single upstream layer" });
    } else if (colors.size > 1) {
      result.set(graphEdgeId(edge), { color: GRAPH_NEUTRAL_EDGE_COLOR, label: "Mixed upstream layers" });
    } else {
      result.set(graphEdgeId(edge), { color: GRAPH_NEUTRAL_EDGE_COLOR, label: "No upstream layer" });
    }
  }
  return result;
}

function traceUpstreamLayerColors(
  nodeId: string,
  incoming: Map<string, SequenceGraphEdge[]>,
  layerColors: Map<string, string>,
  visited: Set<string>
) {
  const layerColor = layerColors.get(nodeId);
  if (layerColor !== undefined) return new Set([layerColor]);
  if (visited.has(nodeId)) return new Set<string>();
  visited.add(nodeId);
  const colors = new Set<string>();
  for (const edge of incoming.get(nodeId) ?? []) {
    for (const color of traceUpstreamLayerColors(edge.fromNode, incoming, layerColors, new Set(visited))) {
      colors.add(color);
    }
  }
  return colors;
}

import type { SequenceCompositionGraph, SequenceGraphEdge } from "../../../types";

export const GRAPH_NEUTRAL_EDGE_COLOR = "var(--donder-graph-edge-neutral)";

export type GraphEdgeLineage = {
  color: string;
  label: string;
};

export function graphEdgeId(edge: SequenceGraphEdge) {
  return JSON.stringify([edge.fromNode, edge.toNode, edge.input]);
}

export function graphEdgeLineages(graph: SequenceCompositionGraph) {
  const incoming = new Map<number, SequenceGraphEdge[]>();
  const layerColors = new Map<number, string>();
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
  nodeId: number,
  incoming: Map<number, SequenceGraphEdge[]>,
  layerColors: Map<number, string>,
  visited: Set<number>
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

import { useAppStore } from "../../../store";
import { matchesCommand } from "../../../commandRegistry";
import { isMac } from "../../../platform";
import { useEditorCommands } from "../../../shortcuts";
import type { EditorCommandId } from "../../../types";
import { OverlayPortal } from "../../OverlayPortal";
import { ContextMenuEntries, Tip } from "../../kit";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { Layers, SlidersHorizontal, Monitor, Trash2 } from "lucide-react";
import {
  Background, Controls, Handle, MarkerType, MiniMap, NodeResizeControl, Position, ReactFlow,
  useEdgesState, useNodesState, useUpdateNodeInternals, type Edge, type Node, type NodeProps, type ReactFlowInstance
} from "@xyflow/react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode, useContext } from "react";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import { definitionEntries, definitionTree } from "./definitionMenu";
import type { GuiDocumentRequest, SequenceGraphEdge, SequenceGraphNode, SequenceGraphOperator, SequenceGraphOperatorDefinition } from "../../../types";
import type { CompositionEditing } from "./composition";
import { GRAPH_NEUTRAL_EDGE_COLOR, graphEdgeId, graphEdgeLineages } from "./graphEdge";
import { graphOperatorDefinition } from "./graphOperator";
import { useGraphViewState } from "./graphViewState";
import { GraphFlowEdge } from "./GraphFlowEdge";
import { GraphNodeControls } from "./GraphNodeControls";
import { defaultLayerColor, deletableGraphNodes, useSequenceEditErrorReporter, useSequenceEditable } from "./sequenceLayers";

export type SelectedGraphItem = { type: "node"; id: number } | { type: "edge"; id: string } | null;
/** A port's handle; `input` is the operator input it feeds, or null for the root's one input. */
type GraphPort = { input: string | null; label: string };
type GraphNodeData = {
  label: string;
  kind: SequenceGraphNode["kind"]["type"];
  color: string | null;
  enabled: boolean;
  controls: ReactNode;
  minHeight: number;
  minWidth: number;
  inputs: GraphPort[];
  output: boolean;
  saveSize: (id: string, width: number, height: number) => void;
};
type GraphFlowNode = Node<GraphNodeData, "donder">;
type Selection = { nodes: string[]; edges: string[] };
type ContextTarget = { type: "pane"; position: { x: number; y: number } } | { type: "selection"; selection: Selection } | null;
const NODE_TYPES = { donder: GraphNodeView };
const EDGE_TYPES = { donder: GraphFlowEdge };
/** The graph's command keys; a node's controls keep them, so operating a control never moves or deletes nodes. */
const GRAPH_COMMAND_KEYS: EditorCommandId[] = ["edit.selectAll", "edit.delete", "edit.moveLeft", "edit.moveRight", "edit.moveUp", "edit.moveDown"];

export function GraphCanvas({ composition, setSelectedItem, requestDelete }: {
  composition: CompositionEditing;
  setSelectedItem: (item: SelectedGraphItem) => void;
  requestDelete: (nodeIds: number[], edgeIds?: string[]) => void;
}) {
  const overlayContainer = useContext(OverlayPortal);
  const { document } = composition;
  const reportSequenceEditError = useSequenceEditErrorReporter();

  const editable = useSequenceEditable();
  const request = useAppStore((state) => state.guiRequest);
  const graph = document.compositionGraph;
  const operatorTree = useMemo(
    () => definitionTree(graph.operatorCatalog, (definition) => definition.operator.path),
    [graph.operatorCatalog]
  );
  const { initial, view, save, saveSize } = useGraphViewState(document.sourceRef);
  const flow = useRef<ReactFlowInstance<GraphFlowNode> | null>(null);
  const pane = useRef<HTMLDivElement | null>(null);
  const connectionOrigin = useRef<GuiDocumentRequest | null>(null);
  const gesture = useRef<{ origin: GuiDocumentRequest | null; ids: Set<string>; committing: boolean } | null>(null);
  const [selection, setSelection] = useState<Selection>({ nodes: [], edges: [] });
  const [context, setContext] = useState<ContextTarget>(null);
  const nodes = useMemo<GraphFlowNode[]>(() => graph.nodes.map((node) => {
    const id = String(node.id);
    const size = view.nodeSizes[id];
    const inputs = graphNodeInputs(node, graph.operatorCatalog);
    const output = node.kind.type !== "root";
    const portHeight = THEME_METRICS.graphNodeHeaderHeight + THEME_METRICS.graphNodeBodyPadding * 2
      + Math.max(inputs.length, output ? 1 : 0) * THEME_METRICS.graphPortRowHeight;
    const hasControls = node.kind.type === "layer" || (node.kind.type === "operator" && node.kind.params.length > 0);
    const controlsMinHeight = !hasControls ? 0 : node.kind.type === "layer"
      ? THEME_METRICS.graphLayerControlsHeight : THEME_METRICS.graphNodeControlsMinHeight;
    const controlsHeight = node.kind.type === "operator" && hasControls
      ? Math.max(controlsMinHeight, Math.min(node.kind.params.length * THEME_METRICS.graphNodeParamRowHeight, THEME_METRICS.graphNodeControlsMaxHeight))
      : controlsMinHeight;
    const minHeight = Math.max(THEME_METRICS.graphNodeMinHeight, portHeight + controlsMinHeight);
    const minWidth = hasControls ? THEME_METRICS.graphNodeEditorMinWidth : THEME_METRICS.graphNodeMinWidth;
    return {
      id, type: "donder", position: { x: node.x, y: node.y },
      width: Math.max(size?.width ?? (hasControls ? THEME_METRICS.graphNodeEditorWidth : THEME_METRICS.graphNodeInitialWidth), minWidth),
      height: Math.max(size?.height ?? Math.max(THEME_METRICS.graphNodeInitialHeight, portHeight + controlsHeight), minHeight),
      draggable: editable, connectable: editable,
      data: {
        kind: node.kind.type,
        label: node.kind.type === "layer" ? node.kind.layerName : node.kind.type === "root" ? "Root" : graphOperatorDefinition(graph.operatorCatalog, node.kind.operator).displayName,
        color: node.kind.type === "layer" ? node.kind.layerColor : null,
        enabled: node.kind.type !== "layer" || node.kind.enabled,
        minHeight, minWidth,
        controls: hasControls ? <GraphNodeControls node={node} composition={composition} /> : null,
        inputs, output, saveSize
      }
    };
  }), [editable, graph, saveSize, view.nodeSizes, composition]);
  const edges = useMemo<Edge[]>(() => {
    const lineages = graphEdgeLineages(graph);
    return graph.edges.map((edge) => {
      const id = graphEdgeId(edge);
      const color = lineages.get(id)?.color ?? GRAPH_NEUTRAL_EDGE_COLOR;
      return {
        id, source: String(edge.fromNode), target: String(edge.toNode), targetHandle: edge.input,
        markerEnd: { type: MarkerType.ArrowClosed, color },
        style: { stroke: color }
      };
    });
  }, [graph]);
  const [flowNodes, setFlowNodes, onNodesChange] = useNodesState(nodes);
  const [flowEdges, setFlowEdges, onEdgesChange] = useEdgesState(edges);
  const authoritativeNodes = useRef(nodes);
  useEffect(() => {
    authoritativeNodes.current = nodes;
    setFlowNodes((current) => {
      const localNodes = new Map(current.map((node) => [node.id, node]));
      return nodes.map((node) => {
        const local = localNodes.get(node.id);
        const active = gesture.current;
        // The edit's own snapshot event advances the request before its document arrives; finishMove settles a committing gesture.
        const keepDraft = active !== null && active.ids.has(node.id) && (active.committing || active.origin === request);
        return { ...node, ...(local?.measured === undefined ? {} : { measured: local.measured }), selected: local?.selected ?? false,
          position: keepDraft && local !== undefined ? local.position : node.position,
          ...(local?.resizing === true ? { width: local.width, height: local.height, resizing: true } : {})
        };
      });
    });
  }, [nodes, request, setFlowNodes]);
  useEffect(() => {
    setFlowEdges((current) => {
      const selected = new Set(current.filter((edge) => edge.selected === true).map((edge) => edge.id));
      return edges.map((edge) => ({ ...edge, selected: selected.has(edge.id) }));
    });
  }, [edges, setFlowEdges]);

  const beginMove = (moving: GraphFlowNode[]) => {
    if (!editable) return;
    gesture.current = { origin: useAppStore.getState().guiRequest, ids: new Set(moving.map((node) => node.id)), committing: false };
  };
  const finishMove = async (moving: GraphFlowNode[]) => {
    const active = gesture.current;
    if (active === null || active.committing) return;
    active.committing = true;
    const originalNodes = new Map(authoritativeNodes.current.map((node) => [node.id, node]));
    const positions = moving.filter((node) => {
      const original = originalNodes.get(node.id);
      return original !== undefined && (node.position.x !== original.position.x || node.position.y !== original.position.y);
    }).map((node) => ({ nodeId: Number(node.id), ...node.position }));
    try {
      if (positions.length > 0) await composition.apply({ type: "moveGraphNodes", positions }, active.origin);
    } catch (error: unknown) {
      reportSequenceEditError(error);
    } finally {
      gesture.current = null;
      // A failed/stale gesture must restore the document, even when no new projection arrived.
      const latest = useAppStore.getState().guiDocument;
      const latestNodes = new Map((latest?.type === "sequence" || latest?.type === "console" ? latest.document.compositionGraph.nodes : []).map((node) => [String(node.id), node]));
      setFlowNodes((current) => {
        const localNodes = new Map(current.map((node) => [node.id, node]));
        return authoritativeNodes.current.map((node) => {
          const position = latestNodes.get(node.id);
          const local = localNodes.get(node.id);
          return { ...node, ...(local?.measured === undefined ? {} : { measured: local.measured }), selected: local?.selected ?? false,
            position: position === undefined ? node.position : { x: position.x, y: position.y } };
        });
      });
    }
  };
  const add = (operator: SequenceGraphOperator | null, position: { x: number; y: number }) => {
    void composition.apply(operator === null ? {
      type: "createLayerAt", name: "Layer", color: defaultLayerColor(document.layers.length), ...position
    } : { type: "addGraphOperatorNode", operator, initialColor: THEME_COLORS.white, ...position }).catch(reportSequenceEditError);
  };
  const removable = (items: Selection) => items.edges.length > 0 || deletableGraphNodes(document, items.nodes.map(Number)).length > 0;
  const deleteSelection = (items: Selection) => { requestDelete(items.nodes.map(Number), items.edges); };
  const selectionChanged = useCallback(({ nodes, edges }: { nodes: GraphFlowNode[]; edges: Edge[] }) => {
    setSelection({ nodes: nodes.map((node) => node.id), edges: edges.map((edge) => edge.id) });
    setSelectedItem(nodes.length + edges.length !== 1 ? null : nodes[0] !== undefined ? { type: "node", id: Number(nodes[0].id) } : edges[0] !== undefined ? { type: "edge", id: edges[0].id } : null);
  }, [setSelectedItem]);
  const moveSelection = (x: number, y: number, shift: boolean) => {
    if (flow.current === null) return;
    const moving = flow.current.getNodes().filter((node) => selection.nodes.includes(node.id));
    const distance = THEME_METRICS.graphGridGap * (shift ? THEME_METRICS.graphKeyboardLargeStep : 1);
    beginMove(moving);
    void finishMove(moving.map((node) => ({ ...node, position: { x: node.position.x + x * distance, y: node.position.y + y * distance } })));
  };
  const movable = () => editable && selection.nodes.length > 0;
  useEditorCommands(pane, {
    "edit.selectAll": { run: () => {
      setFlowNodes((nodes) => nodes.map((node) => ({ ...node, selected: true })));
      setFlowEdges((edges) => edges.map((edge) => ({ ...edge, selected: true })));
    } },
    "edit.delete": { enabled: () => editable && removable(selection), run: () => { deleteSelection(selection); } },
    "edit.moveLeft": { enabled: movable, run: ({ shift }) => { moveSelection(-1, 0, shift); } },
    "edit.moveRight": { enabled: movable, run: ({ shift }) => { moveSelection(1, 0, shift); } },
    "edit.moveUp": { enabled: movable, run: ({ shift }) => { moveSelection(0, -1, shift); } },
    "edit.moveDown": { enabled: movable, run: ({ shift }) => { moveSelection(0, 1, shift); } }
  });
  return <div className="graph-canvas-workspace">
    <ContextMenu.Root onOpenChange={(open) => { if (!open) setContext(null); }}>
      <ContextMenu.Trigger asChild>
        <div ref={pane} className="graph-flow-pane" tabIndex={0} aria-label="Composition graph canvas"
          onContextMenuCapture={(event) => {
            if (!(event.target instanceof Element) || flow.current === null) return;
            if (event.target.closest("input, textarea, select, button, [contenteditable=true]") !== null) return;
            const nodeId = event.target.closest(".react-flow__node")?.getAttribute("data-id");
            const edgeId = event.target.closest(".react-flow__edge")?.getAttribute("data-id");
            const current = { nodes: flow.current.getNodes().filter((node) => node.selected === true).map((node) => node.id), edges: flow.current.getEdges().filter((edge) => edge.selected === true).map((edge) => edge.id) };
            const items = nodeId !== null && nodeId !== undefined ? current.nodes.includes(nodeId) ? current : { nodes: [nodeId], edges: [] }
              : edgeId !== null && edgeId !== undefined ? current.edges.includes(edgeId) ? current : { nodes: [], edges: [edgeId] }
              : event.target.closest(".react-flow__nodesselection-rect") !== null ? current : null;
            if (items !== null) {
              setFlowNodes((nodes) => nodes.map((node) => ({ ...node, selected: items.nodes.includes(node.id) })));
              setFlowEdges((edges) => edges.map((edge) => ({ ...edge, selected: items.edges.includes(edge.id) })));
              setContext({ type: "selection", selection: items });
            } else setContext({ type: "pane", position: flow.current.screenToFlowPosition({ x: event.clientX, y: event.clientY }) });
          }}>
          <ReactFlow nodes={flowNodes} edges={flowEdges} nodeTypes={NODE_TYPES} edgeTypes={EDGE_TYPES}
            fitView={initial.viewport === null} {...(initial.viewport === null ? {} : { defaultViewport: initial.viewport })}
            minZoom={THEME_METRICS.graphMinZoom} maxZoom={THEME_METRICS.graphMaxZoom} panOnScroll={isMac}
            deleteKeyCode={null} disableKeyboardA11y nodesDraggable={editable} nodesConnectable={editable} edgesReconnectable={editable}
            defaultEdgeOptions={{ type: "donder", interactionWidth: THEME_METRICS.graphEdgeInteractionWidth, className: "graph-flow-edge" }}
            onInit={(instance) => { flow.current = instance; }}
            onNodesChange={onNodesChange} onEdgesChange={onEdgesChange} onSelectionChange={selectionChanged}
            onNodeDragStart={(_, node, nodes) => { beginMove(nodes.length > 0 ? nodes : [node]); }}
            onNodeDragStop={(_, node, nodes) => { void finishMove(nodes.length > 0 ? nodes : [node]); }}
            onSelectionDragStart={(_, nodes) => { beginMove(nodes); }}
            onSelectionDragStop={(_, nodes) => { void finishMove(nodes); }}
            onMoveEnd={(_, viewport) => { save({ viewport }); }}
            onConnectStart={() => { connectionOrigin.current = useAppStore.getState().guiRequest; }}
            onReconnectStart={() => { connectionOrigin.current = useAppStore.getState().guiRequest; }}
            onReconnect={(edge, connection) => {
              if (!editable) return;
              const previous = graph.edges.find((item) => graphEdgeId(item) === edge.id);
              if (previous === undefined) return;
              void composition.apply({ type: "reconnectGraphEdge", previous, connection: graphEdge(connection) }, connectionOrigin.current)
                .catch(reportSequenceEditError);
            }}
            onReconnectEnd={(_, edge, __, state) => {
              if (!editable || state.toHandle !== null) return;
              const previous = graph.edges.find((item) => graphEdgeId(item) === edge.id);
              if (previous === undefined) return;
              void composition.apply({ type: "deleteGraphItems", nodeIds: [], layerIds: [], edges: [previous], migrateToLayerId: null }, connectionOrigin.current)
                .catch(reportSequenceEditError);
            }}
            onConnect={(connection) => {
              if (!editable) return;
              void composition.apply({ type: "connectGraphNodes", connection: graphEdge(connection) }, connectionOrigin.current)
                .catch(reportSequenceEditError);
            }}>
            <Background color={THEME_COLORS.graphGrid} gap={THEME_METRICS.graphGridGap} />
            <MiniMap className="graph-flow-minimap" nodeStrokeWidth={THEME_METRICS.graphNodeStrokeWidth} pannable zoomable />
            <Controls className="graph-flow-controls" showInteractive={false} />
          </ReactFlow>
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal container={overlayContainer}><ContextMenu.Content className="menu-content graph-menu">
        {context?.type === "selection" ? <ContextMenuEntries entries={[
          { type: "action", label: "Delete", icon: Trash2, danger: true, shortcut: "edit.delete", disabled: !editable || !removable(context.selection), onSelect: () => { deleteSelection(context.selection); } }
        ]} /> :
          context?.type === "pane" && <ContextMenuEntries entries={[
            { type: "action", label: "Add Layer", disabled: !editable, onSelect: () => { add(null, context.position); } },
            { type: "separator" },
            { type: "submenu", label: "Add Operator", disabled: !editable, entries: definitionEntries(operatorTree, (definition) => definition.displayName, (definition) => { add(definition.operator, context.position); }, "No operators") }
          ]} />}
      </ContextMenu.Content></ContextMenu.Portal>
    </ContextMenu.Root>
  </div>;
}

function GraphNodeView({ id, data, isConnectable }: NodeProps<GraphFlowNode>) {
  const updateNodeInternals = useUpdateNodeInternals();
  useEffect(() => { updateNodeInternals(id); }, [id, data.inputs, data.output, updateNodeInternals]);
  return <div className={`graph-flow-node-card ${data.kind}${data.enabled ? "" : " disabled"}${data.controls === null ? "" : " has-controls"}`} style={data.color === null ? undefined : { borderLeftColor: data.color }}>
    <NodeResizeControl position="bottom-right" minWidth={data.minWidth} minHeight={data.minHeight} className="graph-flow-resize-corner"
      onResizeEnd={(_, size) => { data.saveSize(id, size.width, size.height); }} />
    <div className="graph-flow-node-title">
      {data.kind === "layer" ? <Layers /> : data.kind === "root" ? <Monitor /> : <SlidersHorizontal />}
      {data.color !== null && <span className="graph-flow-node-swatch" style={{ background: data.color }} />}
      <Tip label={data.label}><span>{data.label}{data.enabled ? "" : " (disabled)"}</span></Tip>
    </div>
    <div className="graph-flow-node-body">
      <div className="graph-flow-port-column inputs">{data.inputs.map((port) => <div className="graph-flow-port-row" key={port.input ?? ""}>
        <Handle {...(port.input === null ? {} : { id: port.input })} isConnectable={isConnectable} type="target" position={Position.Left} className="graph-flow-handle input" aria-label={`Input ${port.label}`} />
        <Tip label={port.label}><span>{port.label}</span></Tip>
      </div>)}</div>
      <div className="graph-flow-port-column outputs">{data.output && <div className="graph-flow-port-row">
        <Tip label="Output"><span>Output</span></Tip>
        <Handle isConnectable={isConnectable} type="source" position={Position.Right} className="graph-flow-handle output" aria-label="Output" />
      </div>}</div>
    </div>
    {data.controls !== null && <div className="graph-flow-node-controls nodrag nopan nowheel nokey"
      onKeyDown={(event) => { if (GRAPH_COMMAND_KEYS.some((id) => matchesCommand(event.nativeEvent, id))) event.stopPropagation(); }}
      onContextMenu={(event) => {
        if (event.target instanceof Element && event.target.closest("input, textarea, select, button, [contenteditable=true]") !== null) event.stopPropagation();
      }}>{data.controls}</div>}
  </div>;
}

/** The inputs a node shows: an operator's declared inputs, or the root's one input. */
function graphNodeInputs(node: SequenceGraphNode, catalog: SequenceGraphOperatorDefinition[]): GraphPort[] {
  switch (node.kind.type) {
    case "layer": return [];
    case "root": return [{ input: null, label: "Input" }];
    case "operator": return graphOperatorDefinition(catalog, node.kind.operator).inputs.map((input) => ({ input, label: input }));
  }
}

function graphEdge(connection: { source: string; target: string; targetHandle: string | null }): SequenceGraphEdge {
  return { fromNode: Number(connection.source), toNode: Number(connection.target), input: connection.targetHandle };
}

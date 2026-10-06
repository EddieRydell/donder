import { useSequenceEditorHost } from "../../../editor/host";
import { isMac } from "../../../platform";
import { OverlayPortal } from "../../OverlayPortal";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { Layers, SlidersHorizontal, Monitor } from "lucide-react";
import {
  Background, Controls, Handle, MarkerType, MiniMap, NodeResizeControl, Position, ReactFlow,
  useEdgesState, useNodesState, useUpdateNodeInternals, type Edge, type Node, type NodeProps, type ReactFlowInstance
} from "@xyflow/react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode, useContext } from "react";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import type { GuiDocumentRequest, SequenceEditorDocument, SequenceGraphNode, SequenceGraphOperator } from "../../../editor/types";
import { GRAPH_NEUTRAL_EDGE_COLOR, graphEdgeId, graphEdgeLineages } from "./graphEdge";
import { graphOperatorDefinition, graphOperatorKey } from "./graphOperator";
import { useGraphViewState } from "./graphViewState";
import { GraphFlowEdge } from "./GraphFlowEdge";
import { GraphNodeControls } from "./GraphNodeControls";
import type { AutomationClipChooser } from "../shared";
import { defaultLayerColor, deletableGraphNodes, nextLayerName, useSequenceEditErrorReporter, useSequenceEditable } from "./sequenceLayers";

export type SelectedGraphItem = { type: "node"; id: string } | { type: "edge"; id: string } | null;
type GraphNodeData = {
  label: string;
  kind: SequenceGraphNode["kind"]["type"];
  color: string | null;
  enabled: boolean;
  controls: ReactNode;
  minHeight: number;
  minWidth: number;
  inputs: SequenceGraphNode["inputs"];
  outputs: SequenceGraphNode["outputs"];
  saveSize: (id: string, width: number, height: number) => void;
};
type GraphFlowNode = Node<GraphNodeData, "donder">;
type Selection = { nodes: string[]; edges: string[] };
type ContextTarget = { type: "pane"; position: { x: number; y: number } } | { type: "selection"; selection: Selection } | null;
const NODE_TYPES = { donder: GraphNodeView };
const EDGE_TYPES = { donder: GraphFlowEdge };

export function GraphCanvas({ document, setSelectedItem, requestDelete, automationClipChooser, setAutomationClipChooser }: {
  document: SequenceEditorDocument;
  setSelectedItem: (item: SelectedGraphItem) => void;
  requestDelete: (nodeIds: string[], edgeIds?: string[]) => void;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
}) {
  const host = useSequenceEditorHost();
  const overlayContainer = useContext(OverlayPortal);
  const { commands, store: useAppStore, runGuiEditCommand } = host;
  const reportSequenceEditError = useSequenceEditErrorReporter();

  const editable = useSequenceEditable();
  const request = useAppStore((state) => state.guiRequest);
  const graph = document.compositionGraph;
  const { initial, view, save, saveSize } = useGraphViewState(document.sourceRef);
  const flow = useRef<ReactFlowInstance<GraphFlowNode> | null>(null);
  const connectionOrigin = useRef<GuiDocumentRequest | null>(null);
  const gesture = useRef<{ origin: GuiDocumentRequest | null; ids: Set<string>; committing: boolean } | null>(null);
  const [selection, setSelection] = useState<Selection>({ nodes: [], edges: [] });
  const [context, setContext] = useState<ContextTarget>(null);
  const nodes = useMemo<GraphFlowNode[]>(() => graph.nodes.map((node) => {
    const size = view.nodeSizes[node.id];
    const portHeight = THEME_METRICS.graphNodeHeaderHeight + THEME_METRICS.graphNodeBodyPadding * 2
      + Math.max(node.inputs.length, node.outputs.length) * THEME_METRICS.graphPortRowHeight;
    const hasControls = node.kind.type === "layer" || (node.kind.type === "operator" && node.kind.params.length > 0);
    const controlsMinHeight = !hasControls ? 0 : node.kind.type === "layer"
      ? THEME_METRICS.graphLayerControlsHeight : THEME_METRICS.graphNodeControlsMinHeight;
    const controlsHeight = node.kind.type === "operator" && hasControls
      ? Math.max(controlsMinHeight, Math.min(node.kind.params.length * THEME_METRICS.graphNodeParamRowHeight, THEME_METRICS.graphNodeControlsMaxHeight))
      : controlsMinHeight;
    const minHeight = Math.max(THEME_METRICS.graphNodeMinHeight, portHeight + controlsMinHeight);
    const minWidth = hasControls ? THEME_METRICS.graphNodeEditorMinWidth : THEME_METRICS.graphNodeMinWidth;
    return {
      id: node.id, type: "donder", position: { x: node.x, y: node.y },
      width: Math.max(size?.width ?? (hasControls ? THEME_METRICS.graphNodeEditorWidth : THEME_METRICS.graphNodeInitialWidth), minWidth),
      height: Math.max(size?.height ?? Math.max(THEME_METRICS.graphNodeInitialHeight, portHeight + controlsHeight), minHeight),
      draggable: editable, connectable: editable,
      data: {
        kind: node.kind.type,
        label: node.kind.type === "layer" ? node.kind.layerName : node.kind.type === "output" ? "Output" : graphOperatorDefinition(graph.operatorCatalog, node.kind.operator).displayName,
        color: node.kind.type === "layer" ? node.kind.layerColor : null,
        enabled: node.kind.type !== "layer" || node.kind.enabled,
        minHeight, minWidth,
        controls: hasControls ? <GraphNodeControls node={node} document={document}
          automationClipChooser={automationClipChooser} setAutomationClipChooser={setAutomationClipChooser} /> : null,
        inputs: node.inputs, outputs: node.outputs, saveSize
      }
    };
  }), [editable, graph, saveSize, view.nodeSizes, document, automationClipChooser, setAutomationClipChooser]);
  const edges = useMemo<Edge[]>(() => {
    const lineages = graphEdgeLineages(graph);
    return graph.edges.map((edge) => {
      const id = graphEdgeId(edge);
      const color = lineages.get(id)?.color ?? GRAPH_NEUTRAL_EDGE_COLOR;
      return {
        id, source: edge.fromNode, target: edge.toNode, sourceHandle: edge.fromPort, targetHandle: edge.toPort,
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
        const keepDraft = active !== null && active.origin === request && active.ids.has(node.id);
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
    }).map((node) => ({ nodeId: node.id, ...node.position }));
    try {
      if (positions.length > 0) await runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, { type: "moveGraphNodes", positions }), active.origin);
    } catch (error: unknown) {
      reportSequenceEditError(error);
    } finally {
      gesture.current = null;
      // A failed/stale gesture must restore the document, even when no new projection arrived.
      const latest = useAppStore.getState().guiDocument;
      const latestNodes = new Map((latest?.type === "sequence" ? latest.document.compositionGraph.nodes : []).map((node) => [node.id, node]));
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
    void runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, operator === null ? {
      type: "createLayerAt", name: nextLayerName(document.layers), color: defaultLayerColor(document.layers.length), ...position
    } : { type: "addGraphOperatorNode", operator, initialColor: THEME_COLORS.white, ...position })).catch(reportSequenceEditError);
  };
  const removable = (items: Selection) => items.edges.length > 0 || deletableGraphNodes(document, items.nodes).length > 0;
  const deleteSelection = (items: Selection) => { requestDelete(items.nodes, items.edges); };
  const selectionChanged = useCallback(({ nodes, edges }: { nodes: GraphFlowNode[]; edges: Edge[] }) => {
    setSelection({ nodes: nodes.map((node) => node.id), edges: edges.map((edge) => edge.id) });
    setSelectedItem(nodes.length + edges.length !== 1 ? null : nodes[0] !== undefined ? { type: "node", id: nodes[0].id } : edges[0] !== undefined ? { type: "edge", id: edges[0].id } : null);
  }, [setSelectedItem]);
  return <div className="graph-canvas-workspace">
    <ContextMenu.Root onOpenChange={(open) => { if (!open) setContext(null); }}>
      <ContextMenu.Trigger asChild>
        <div className="graph-flow-pane" tabIndex={0} aria-label="Composition graph canvas"
          onKeyDown={(event) => {
            if (event.target instanceof Element && event.target.closest(".graph-flow-node-controls, input, textarea, select, button, [contenteditable=true]") !== null) return;
            if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a") {
              event.preventDefault(); event.stopPropagation();
              setFlowNodes((nodes) => nodes.map((node) => ({ ...node, selected: true })));
              setFlowEdges((edges) => edges.map((edge) => ({ ...edge, selected: true })));
            }
            if (event.key === "Delete" || event.key === "Backspace") {
              event.preventDefault(); event.stopPropagation();
              deleteSelection(selection);
            }
          }}
          onKeyDownCapture={(event) => {
            if (!(event.target instanceof Element) || event.target.closest(".graph-flow-node-controls, input, textarea, select, button, [contenteditable=true]")) return;
            const delta = event.key === "ArrowLeft" ? { x: -1, y: 0 } : event.key === "ArrowRight" ? { x: 1, y: 0 }
              : event.key === "ArrowUp" ? { x: 0, y: -1 } : event.key === "ArrowDown" ? { x: 0, y: 1 } : null;
            if (delta === null || flow.current === null) return;
            const focusedId = event.target.closest(".react-flow__node")?.getAttribute("data-id");
            const moving = flow.current.getNodes().filter((node) => selection.nodes.includes(node.id) || (selection.nodes.length === 0 && node.id === focusedId));
            if (moving.length === 0) return;
            event.preventDefault(); event.stopPropagation();
            if (!editable) return;
            const distance = THEME_METRICS.graphGridGap * (event.shiftKey ? THEME_METRICS.graphKeyboardLargeStep : 1);
            beginMove(moving);
            void finishMove(moving.map((node) => ({ ...node, position: { x: node.position.x + delta.x * distance, y: node.position.y + delta.y * distance } })));
          }}
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
            deleteKeyCode={null} nodesDraggable={editable} nodesConnectable={editable} edgesReconnectable={editable}
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
              const fromPort = connection.sourceHandle;
              const toPort = connection.targetHandle;
              if (previous === undefined || fromPort === null || toPort === null) return;
              void runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, {
                type: "reconnectGraphEdge", previous, connection: {
                  fromNode: connection.source, fromPort,
                  toNode: connection.target, toPort
                }
              }), connectionOrigin.current).catch(reportSequenceEditError);
            }}
            onConnect={(connection) => {
              if (!editable) return;
              const fromPort = connection.sourceHandle;
              const toPort = connection.targetHandle;
              if (fromPort === null || toPort === null) return;
              void runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, {
                type: "connectGraphNodes", fromNode: connection.source, toNode: connection.target,
                fromPort, toPort
              }), connectionOrigin.current).catch(reportSequenceEditError);
            }}>
            <Background color={THEME_COLORS.graphGrid} gap={THEME_METRICS.graphGridGap} />
            <MiniMap className="graph-flow-minimap" nodeStrokeWidth={THEME_METRICS.graphNodeStrokeWidth} pannable zoomable />
            <Controls className="graph-flow-controls" showInteractive={false} />
          </ReactFlow>
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal container={overlayContainer}><ContextMenu.Content className="menu-content graph-menu">
        {context?.type === "selection" ? <ContextMenu.Item className="menu-item danger" disabled={!editable || !removable(context.selection)} onSelect={() => { deleteSelection(context.selection); }}>Delete selected</ContextMenu.Item> :
          context?.type === "pane" && <>
            <ContextMenu.Item className="menu-item" disabled={!editable} onSelect={() => { add(null, context.position); }}>Add layer</ContextMenu.Item>
            <ContextMenu.Separator className="menu-separator" />
            <ContextMenu.Label className="menu-label">Add operator</ContextMenu.Label>
            {graph.operatorCatalog.map((definition) => <ContextMenu.Item className="menu-item" disabled={!editable} key={graphOperatorKey(definition.operator)} onSelect={() => { add(definition.operator, context.position); }}>{definition.displayName}</ContextMenu.Item>)}
          </>}
      </ContextMenu.Content></ContextMenu.Portal>
    </ContextMenu.Root>
  </div>;
}

function GraphNodeView({ id, data, isConnectable }: NodeProps<GraphFlowNode>) {
  const updateNodeInternals = useUpdateNodeInternals();
  useEffect(() => { updateNodeInternals(id); }, [id, data.inputs, data.outputs, updateNodeInternals]);
  return <div className={`graph-flow-node-card ${data.kind}${data.enabled ? "" : " disabled"}${data.controls === null ? "" : " has-controls"}`} style={data.color === null ? undefined : { borderLeftColor: data.color }}>
    <NodeResizeControl position="bottom-right" minWidth={data.minWidth} minHeight={data.minHeight} className="graph-flow-resize-corner"
      onResizeEnd={(_, size) => { data.saveSize(id, size.width, size.height); }} />
    <div className="graph-flow-node-title">
      {data.kind === "layer" ? <Layers /> : data.kind === "output" ? <Monitor /> : <SlidersHorizontal />}
      {data.color !== null && <span className="graph-flow-node-swatch" style={{ background: data.color }} />}
      <span title={data.label}>{data.label}{data.enabled ? "" : " (disabled)"}</span>
    </div>
    <div className="graph-flow-node-body">
      <div className="graph-flow-port-column inputs">{data.inputs.map((port) => <div className="graph-flow-port-row" key={port.sourceName}>
        <Handle id={port.sourceName} isConnectable={isConnectable} type="target" position={Position.Left} className="graph-flow-handle input" aria-label={`Input ${port.displayName}`} title={port.displayName} />
        <span title={port.displayName}>{port.displayName}</span>
      </div>)}</div>
      <div className="graph-flow-port-column outputs">{data.outputs.map((port) => <div className="graph-flow-port-row" key={port.sourceName}>
        <span title={port.displayName}>{port.displayName}</span>
        <Handle id={port.sourceName} isConnectable={isConnectable} type="source" position={Position.Right} className="graph-flow-handle output" aria-label={`Output ${port.displayName}`} title={port.displayName} />
      </div>)}</div>
    </div>
    {data.controls !== null && <div className="graph-flow-node-controls nodrag nopan nowheel"
      onKeyDown={(event) => { event.stopPropagation(); }}
      onContextMenu={(event) => {
        if (event.target instanceof Element && event.target.closest("input, textarea, select, button, [contenteditable=true]") !== null) event.stopPropagation();
      }}>{data.controls}</div>}
  </div>;
}

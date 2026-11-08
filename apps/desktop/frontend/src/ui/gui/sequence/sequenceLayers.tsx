import { useAppStore } from "../../../store";
import { Plus, Trash2, Workflow } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { THEME_COLORS } from "../../../theme";
import type { GuiDocumentRequest, SequenceGraphEdge, SequenceLayer } from "../../../types";
import type { CompositionDocument, CompositionEditing } from "./composition";
import { ColorPicker } from "../../ColorPicker";
import { Button, Dialog, Field, IconButton, Select, TextField, Toggle } from "../../kit";
import { requestOpenLayerGraph } from "../../uiEvents";
import { graphEdgeId } from "./graphEdge";

export function useSequenceEditErrorReporter() {
 return useCallback((error: unknown) => { useAppStore.getState().setError(String(error)); }, []);
}

export function useSequenceEditable() {

  return useAppStore((state) => state.guiRequest !== null
    && state.guiRequest.projectRevision === state.guiDocumentRevision
    && !state.guiEditPending);
}

export function defaultLayerColor(index: number) {
  const colors = [THEME_COLORS.graphBlue, THEME_COLORS.graphRed, THEME_COLORS.graphGreen, THEME_COLORS.graphYellow, THEME_COLORS.graphPurple, THEME_COLORS.graphPink];
  const color = colors[index % colors.length];
  if (color === undefined) throw new Error("Invalid layer palette index.");
  return color;
}

export function deletableGraphNodes(document: CompositionDocument, nodeIds: number[]) {
  return document.compositionGraph.nodes.filter((node) => {
    if (!nodeIds.includes(node.id)) return false;
    if (node.kind.type === "operator") return true;
    return node.kind.type === "layer";
  });
}

export function LayerProperties({ layer, composition, onDelete }: { layer: SequenceLayer; composition: CompositionEditing; onDelete?: () => void }) {
  const reportSequenceEditError = useSequenceEditErrorReporter();

  const editable = useSequenceEditable();
  return <fieldset className="sequence-layer-row" disabled={!editable}>
    <Toggle name={`${layer.name} enabled`} checked={layer.enabled} disabled={!editable} onChange={(enabled) => {
      void composition.apply({ type: "setLayerEnabled", id: layer.id, enabled }).catch(reportSequenceEditError);
    }} />
    <ColorPicker value={layer.color} label={`${layer.name} color`} commit={(color) =>
      composition.applyItem({ type: "setColor", item: { type: "layer", id: layer.id }, color }).then(() => undefined).catch(reportSequenceEditError)
    } />
    <TextField name="Layer name" value={layer.name} disabled={!editable} onCommit={(value) => {
      const name = value.trim();
      if (name === "" || name === layer.name) return;
      void composition.applyItem({ type: "rename", item: { type: "layer", id: layer.id }, name }).catch(reportSequenceEditError);
    }} />
    {onDelete !== undefined && <IconButton icon={Trash2} label={`Delete ${layer.name}`} danger disabled={!editable} onClick={onDelete} />}
  </fieldset>;
}

type Deletion = {
  nodeIds: number[];
  edges: SequenceGraphEdge[];
  layerIds: number[];
  clipCount: number;
  origin: GuiDocumentRequest;
};

/**
 * Deletes graph items at once, except layers that clips use: those open a dialog that picks the
 * layer their clips move to.
 */
export function useGraphDeletion(composition: CompositionEditing, onDeleted?: () => void) {
  const { document } = composition;
  const reportSequenceEditError = useSequenceEditErrorReporter();

  const returnFocus = useRef<HTMLElement | null>(null);
  const [pending, setPending] = useState<Deletion | null>(null);
  const [destination, setDestination] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const editable = useSequenceEditable();
  const busy = useAppStore((state) => state.guiEditPending);
  const commit = useCallback(async (deletion: Deletion, migrateToLayerId: number | null) => {
    try {
      await composition.apply({
        type: "deleteGraphItems", nodeIds: deletion.nodeIds, layerIds: deletion.layerIds, edges: deletion.edges, migrateToLayerId
      }, deletion.origin);
      setPending(null);
      setError(null);
      onDeleted?.();
    } catch (error: unknown) {
      setError(String(error));
      reportSequenceEditError(error);
    }
  }, [composition, reportSequenceEditError, onDeleted]);
  const requestDelete = useCallback((nodeIds: number[], edgeIds: string[] = [], explicitLayerIds: number[] = []) => {
    if (!editable) return;
    const origin = useAppStore.getState().guiRequest;
    if (origin === null) return;
    const nodes = deletableGraphNodes(document, nodeIds);
    const edges = document.compositionGraph.edges.filter((edge) => edgeIds.includes(graphEdgeId(edge)));
    const layerIds = [...new Set([
      ...explicitLayerIds,
      ...nodes.flatMap((node) => node.kind.type === "layer" ? [node.kind.layerId] : [])
    ])];
    if (nodes.length === 0 && edges.length === 0 && layerIds.length === 0) return;
    const survivor = document.layers.find((layer) => !layerIds.includes(layer.id));
    const deletion: Deletion = {
      nodeIds: nodes.map((node) => node.id), edges, layerIds, origin,
      clipCount: layerIds.reduce((count, layerId) => count + composition.layerUses(layerId), 0)
    };
    setError(null);
    if (deletion.clipCount === 0) {
      void commit(deletion, layerIds.length === 0 ? null : survivor?.id ?? null);
    } else {
      const active = window.document.activeElement;
      returnFocus.current = active instanceof HTMLElement ? active : null;
      setDestination(survivor?.id ?? null);
      setPending(deletion);
    }
  }, [commit, composition, document, editable]);
  const destinations = document.layers
    .filter((layer) => pending?.layerIds.includes(layer.id) !== true)
    .map((layer) => ({ value: String(layer.id), label: layer.name }));
  const dialog = <Dialog
    open={pending !== null}
    onOpenChange={(open) => { if (!open && !busy) setPending(null); }}
    title="Delete layers"
    focus={{
      onCloseAutoFocus: (event) => {
        if (returnFocus.current?.isConnected === true) {
          event.preventDefault();
          returnFocus.current.focus();
        }
      }
    }}
    actions={<>
      <Button disabled={busy} onClick={() => { setPending(null); }}>Cancel</Button>
      <Button variant="danger" disabled={!editable || destination === null} onClick={() => {
        if (pending !== null) void commit(pending, destination);
      }}>Delete</Button>
    </>}
  >
    <Field label="Move clips" tip={`${pending?.clipCount ?? 0} clips use these layers; selected operators and connections are deleted too`}>
      <Select name="Move clips to" disabled={!editable} value={destination === null ? "" : String(destination)} options={destinations}
        onChange={(value) => { setDestination(Number(value)); }} />
    </Field>
    {error !== null && <p role="alert">{error}</p>}
  </Dialog>;
  return { requestDelete, dialog };
}

/** A sequence's or console's layers, with adding, editing and deleting, and its layer graph. */
export function LayerList({ composition }: { composition: CompositionEditing }) {
  const { document } = composition;
  const reportSequenceEditError = useSequenceEditErrorReporter();
  const editable = useSequenceEditable();
  const { requestDelete, dialog } = useGraphDeletion(composition);
  return <>
    <h2>Layers</h2>
    <Button icon={Workflow} tip="Open layer graph" command="view.openLayerGraph" onClick={requestOpenLayerGraph}>Layer graph</Button>
    <Button icon={Plus} disabled={!editable} onClick={() => {
      void composition.apply({
        type: "createLayer", name: "Layer", color: defaultLayerColor(document.layers.length)
      }).catch(reportSequenceEditError);
    }}>Add layer</Button>
    <div className="sequence-layer-list">
      {document.layers.map((layer) => <LayerProperties key={layer.id} layer={layer} composition={composition} onDelete={() => {
        requestDelete([], [], [layer.id]);
      }} />)}
    </div>
    {dialog}
  </>;
}

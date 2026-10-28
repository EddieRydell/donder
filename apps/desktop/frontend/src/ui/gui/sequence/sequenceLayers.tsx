import { useSequenceEditorHost } from "../../../editor/host";
import { OverlayPortal } from "../../OverlayPortal";
import * as Dialog from "@radix-ui/react-dialog";
import { useCallback, useRef, useState, useContext } from "react";
import { THEME_COLORS } from "../../../theme";
import type { GuiDocumentRequest, SequenceGraphEdge, SequenceLayer } from "../../../editor/types";
import type { CompositionDocument, CompositionEditing } from "./composition";
import { ColorPicker } from "../../ColorPicker";
import { graphEdgeId } from "./graphEdge";

export function useSequenceEditErrorReporter() {
 const host = useSequenceEditorHost();
 return useCallback((error: unknown) => { host.store.getState().setError(String(error)); }, [host]);
}

export function useSequenceEditable() {
  const host = useSequenceEditorHost();
  const { store: useAppStore } = host;

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

export function deletableGraphNodes(document: CompositionDocument, nodeIds: string[]) {
  return document.compositionGraph.nodes.filter((node) => {
    if (!nodeIds.includes(node.id)) return false;
    if (node.kind.type === "operator") return true;
    if (node.kind.type === "output") return false;
    const layerId = node.kind.layerId;
    return document.layers.some((layer) => layer.id === layerId && !layer.isDefault);
  });
}

export function LayerProperties({ layer, composition, onDelete }: { layer: SequenceLayer; composition: CompositionEditing; onDelete?: () => void }) {
  const reportSequenceEditError = useSequenceEditErrorReporter();

  const editable = useSequenceEditable();
  return <fieldset className="sequence-layer-row" disabled={!editable}>
    <input type="checkbox" checked={layer.enabled} aria-label={`${layer.name} enabled`} onChange={(event) => {
      void composition.apply({ type: "setLayerEnabled", id: layer.id, enabled: event.currentTarget.checked }).catch(reportSequenceEditError);
    }} />
    <ColorPicker value={layer.color} label={`${layer.name} color`} commit={(color) =>
      composition.applyItem({ type: "setColor", item: { type: "layer", id: layer.id }, color }).then(() => undefined).catch(reportSequenceEditError)
    } />
    <input key={`${layer.id}:${layer.name}`} defaultValue={layer.name} aria-label="Layer name" required
      onKeyDown={(event) => {
        if (event.key === "Enter") event.currentTarget.blur();
        if (event.key === "Escape") {
          event.stopPropagation();
          event.currentTarget.value = layer.name;
          event.currentTarget.blur();
        }
      }} onBlur={(event) => {
        const input = event.currentTarget;
        const name = input.value.trim();
        if (name === "") { input.value = layer.name; return; }
        if (name === layer.name) return;
        void composition.applyItem({ type: "rename", item: { type: "layer", id: layer.id }, name })
          .catch((error: unknown) => { input.value = layer.name; reportSequenceEditError(error); });
      }} />
    {!layer.isDefault && onDelete !== undefined && <button type="button" onClick={onDelete}>Delete</button>}
  </fieldset>;
}

type Deletion = {
  nodeIds: string[];
  edges: SequenceGraphEdge[];
  layerIds: number[];
  effectCount: number;
  origin: GuiDocumentRequest;
};

export function useGraphDeletion(composition: CompositionEditing, onDeleted?: () => void) {
  const host = useSequenceEditorHost();
  const overlayContainer = useContext(OverlayPortal);
  const { store: useAppStore } = host;
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
  const requestDelete = useCallback((nodeIds: string[], edgeIds: string[] = [], explicitLayerIds: number[] = []) => {
    if (!editable) return;
    const origin = useAppStore.getState().guiRequest;
    if (origin === null) return;
    const nodes = deletableGraphNodes(document, nodeIds);
    const edges = document.compositionGraph.edges.filter((edge) => edgeIds.includes(graphEdgeId(edge)));
    const layerIds = [...new Set([
      ...document.layers.filter((layer) => !layer.isDefault && explicitLayerIds.includes(layer.id)).map((layer) => layer.id),
      ...nodes.flatMap((node) => node.kind.type === "layer" ? [node.kind.layerId] : [])
    ])];
    if (nodes.length === 0 && edges.length === 0 && layerIds.length === 0) return;
    const defaultLayer = document.layers.find((layer) => layer.isDefault);
    const deletion: Deletion = {
      nodeIds: nodes.map((node) => node.id), edges, layerIds, origin,
      effectCount: layerIds.reduce((count, layerId) => count + composition.layerUses(layerId), 0)
    };
    setError(null);
    if (deletion.effectCount === 0) {
      void commit(deletion, layerIds.length === 0 ? null : defaultLayer?.id ?? null);
    } else {
      const active = window.document.activeElement;
      returnFocus.current = active instanceof HTMLElement ? active : null;
      setDestination(defaultLayer?.id ?? null);
      setPending(deletion);
    }
  }, [commit, composition, document, editable, useAppStore]);
  const dialog = <Dialog.Root open={pending !== null} onOpenChange={(open) => { if (!open && !busy) setPending(null); }}>
    <Dialog.Portal container={overlayContainer}>
      <Dialog.Overlay className="dialog-overlay graph-delete-overlay" />
      <Dialog.Content className="dialog-content graph-delete-dialog" onCloseAutoFocus={(event) => {
        if (returnFocus.current?.isConnected === true) {
          event.preventDefault();
          returnFocus.current.focus();
        }
      }}>
        <Dialog.Title>Delete selected layers?</Dialog.Title>
        <Dialog.Description>{pending?.effectCount} effects use these layers. Choose where to move them. Selected operators and connections will also be deleted.</Dialog.Description>
        <fieldset className="graph-dialog-fields" disabled={!editable}>
          <label>Move effects to<select value={destination ?? ""} onChange={(event) => { setDestination(Number(event.currentTarget.value)); }}>
            {document.layers.filter((layer) => pending?.layerIds.includes(layer.id) !== true).map((layer) => <option key={layer.id} value={layer.id}>{layer.name}</option>)}
          </select></label>
          {error !== null && <p role="alert">{error}</p>}
          <div className="dialog-actions">
            <Dialog.Close asChild><button type="button">Cancel</button></Dialog.Close>
            <button type="button" className="danger-button" disabled={destination === null} onClick={() => {
              if (pending !== null) void commit(pending, destination);
            }}>Delete</button>
          </div>
        </fieldset>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>;
  return { requestDelete, dialog };
}

/** A sequence's or rig's layers, with adding, editing and deleting. */
export function LayerList({ composition }: { composition: CompositionEditing }) {
  const { document } = composition;
  const reportSequenceEditError = useSequenceEditErrorReporter();
  const editable = useSequenceEditable();
  const { requestDelete, dialog } = useGraphDeletion(composition);
  return <>
    <button type="button" className="neutral-button" disabled={!editable} onClick={() => {
      void composition.apply({
        type: "createLayer", name: "Layer", color: defaultLayerColor(document.layers.length)
      }).catch(reportSequenceEditError);
    }}>Add layer</button>
    <div className="sequence-layer-list">
      {document.layers.map((layer) => <LayerProperties key={layer.id} layer={layer} composition={composition} onDelete={() => {
        requestDelete([], [], [layer.id]);
      }} />)}
    </div>
    {dialog}
  </>;
}

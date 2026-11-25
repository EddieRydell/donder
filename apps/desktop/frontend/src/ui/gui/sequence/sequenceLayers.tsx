import { useAppStore } from "../../../store";
import { Plus, Trash2, Workflow } from "lucide-react";
import { useCallback } from "react";
import { recolor, rename } from "../edits";
import { useFieldProblem } from "../problems";
import { THEME_COLORS } from "../../../theme";
import type { SequenceLayer } from "../../../types";
import type { CompositionDocument, CompositionEditing } from "./composition";
import { ColorPicker } from "../../ColorPicker";
import { Button, IconButton, Problem, TextField, Toggle } from "../../kit";
import { openLayerGraph } from "../documentUi";
import { graphEdgeId } from "./graphEdge";

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
  const editable = useSequenceEditable();
  const member = { type: "layer", id: layer.id } as const;
  const nameProblem = useFieldProblem(member, { type: "name" });
  return <fieldset className="sequence-layer-row" disabled={!editable}>
    <Toggle name={`${layer.name} enabled`} checked={layer.enabled} disabled={!editable} onChange={(enabled) => {
      void composition.apply({ type: "setLayerEnabled", id: layer.id, enabled });
    }} />
    <ColorPicker value={layer.color} label={`${layer.name} color`} commit={(color) =>
      recolor({ type: "layer", id: layer.id }, color).then(() => undefined)
    } />
    <Problem error={nameProblem}>
      <TextField name="Layer name" value={layer.name} disabled={!editable} invalid={nameProblem !== null} onCommit={(value) => {
        const name = value.trim();
        if (name === "" || name === layer.name) return;
        void rename({ type: "layer", id: layer.id }, name);
      }} />
    </Problem>
    {onDelete !== undefined && <IconButton icon={Trash2} label={`Delete ${layer.name}`} danger disabled={!editable} onClick={onDelete} />}
  </fieldset>;
}

/**
 * Deletes graph items at once. Clips on deleted layers go with them, which the edit's confirmation
 * lists.
 */
export function useGraphDeletion(composition: CompositionEditing, onDeleted?: () => void) {
  const { document } = composition;
  const editable = useSequenceEditable();
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
    void composition.apply({ type: "deleteGraphItems", nodeIds: nodes.map((node) => node.id), layerIds, edges }, origin).then((result) => {
      if (result !== null) onDeleted?.();
    });
  }, [composition, document, editable, onDeleted]);
  return requestDelete;
}

/** A sequence's or console's layers, with adding, editing and deleting, and its layer graph. */
export function LayerList({ composition }: { composition: CompositionEditing }) {
  const { document } = composition;
  const editable = useSequenceEditable();
  const requestDelete = useGraphDeletion(composition);
  return <>
    <h2>Layers</h2>
    <Button icon={Workflow} tip="Open layer graph" command="view.openLayerGraph" onClick={openLayerGraph}>Layer graph</Button>
    <Button icon={Plus} disabled={!editable} onClick={() => {
      void composition.apply({
        type: "createLayer", name: "Layer", color: defaultLayerColor(document.layers.length)
      });
    }}>Add layer</Button>
    <div className="sequence-layer-list">
      {document.layers.map((layer) => <LayerProperties key={layer.id} layer={layer} composition={composition} onDelete={() => {
        requestDelete([], [], [layer.id]);
      }} />)}
    </div>
  </>;
}

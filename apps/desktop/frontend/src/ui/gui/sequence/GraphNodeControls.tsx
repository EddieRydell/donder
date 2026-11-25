import type { SequenceGraphNode } from "../../../types";
import { TypedParamInput } from "./params/TypedParamInput";
import { LayerProperties, useSequenceEditable } from "./sequenceLayers";
import { NameField } from "./NameField";
import { DescriptionField } from "../DescriptionField";
import type { CompositionEditing } from "./composition";
import { describe, put, rename, site } from "../edits";

export function GraphNodeControls({ node, composition }: { node: SequenceGraphNode; composition: CompositionEditing }) {
  const { document, automation } = composition;
  const editable = useSequenceEditable();
  if (node.kind.type === "layer") {
    const layerId = node.kind.layerId;
    const layer = document.layers.find((item) => item.id === layerId);
    if (layer === undefined) throw new Error(`Graph layer ${layerId} was not found.`);
    return <div className="graph-node-layer-controls">
      <LayerProperties layer={layer} composition={composition} />
      <DescriptionField description={layer.description} member={{ type: "layer", id: layer.id }} disabled={!editable} onCommit={(description) =>
        describe({ type: "layer", id: layer.id }, description)} />
      <p>{composition.layerUses(layer.id)} clips</p>
    </div>;
  }
  if (node.kind.type !== "operator") return null;
  return <fieldset className="graph-node-parameters" disabled={!editable} aria-label="Project operator parameters">
    <NameField name={node.kind.name} label="Name" member={{ type: "node", id: node.id }} commit={(name) => rename({ type: "node", id: node.id }, name)} />
    {node.kind.params.map((param, index) => {
      const live = composition.operatorParamControl === null || !param.supportsAutomation ? null : composition.operatorParamControl(node.id, param.name, param.range);
      return <div key={param.name} className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}>
      <TypedParamInput param={param} commitParam={(name, value) =>
        put(site({ type: "node", id: node.id }, { type: "param", name }), { type: "param", value }).then(() => undefined)
      } curveLibrary={document.curveLibrary} gradientLibrary={document.gradientLibrary} markCollections={document.markCollections}
        automationControl={live?.control ?? null} linked={live?.linked ?? false} member={{ type: "node", id: node.id }}
        automation={automation === null ? null : { target: { type: "compositionNodeParam", nodeId: node.id, param: param.name },
          envelopes: automation.envelopes, canCreate: document.layers.length > 0 }} />
    </div>;
    })}
  </fieldset>;
}

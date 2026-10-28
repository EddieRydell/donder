import type { SequenceGraphNode } from "../../../editor/types";
import { TypedParamInput } from "./params/TypedParamInput";
import { LayerProperties, useSequenceEditable } from "./sequenceLayers";
import { NameField } from "./NameField";
import { DescriptionField } from "../DescriptionField";
import type { CompositionEditing } from "./composition";

export function GraphNodeControls({ node, composition }: { node: SequenceGraphNode; composition: CompositionEditing }) {
  const { document, automation } = composition;
  const editable = useSequenceEditable();
  if (node.kind.type === "layer") {
    const layerId = node.kind.layerId;
    const layer = document.layers.find((item) => item.id === layerId);
    if (layer === undefined) throw new Error(`Graph layer ${layerId} was not found.`);
    return <div className="graph-node-layer-controls">
      <LayerProperties layer={layer} composition={composition} />
      <DescriptionField description={layer.description} disabled={!editable} onCommit={(description) =>
        composition.applyItem({ type: "setDescription", item: { type: "layer", id: layer.id }, description })} />
      <p>{composition.layerUses(layer.id)} clips{layer.isDefault ? " · Default layer" : ""}</p>
    </div>;
  }
  if (node.kind.type !== "operator") return null;
  return <fieldset className="graph-node-parameters" disabled={!editable} aria-label="Project operator parameters">
    <NameField name={node.kind.name} label="Name" commit={(name) =>
      composition.applyItem({ type: "rename", item: { type: "graphNode", nodeId: node.id }, name })} />
    {node.kind.params.map((param, index) => <div key={param.name} className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}>
      <TypedParamInput param={param} commitParam={(name, value) =>
        composition.apply({ type: "updateGraphOperatorParam", nodeId: node.id, name, value }).then(() => undefined)
      } curveLibrary={document.curveLibrary} gradientLibrary={document.gradientLibrary} markCollections={document.markCollections}
        automation={automation === null ? null : { target: { type: "compositionNodeParam", nodeId: node.id, param: param.name },
          automationClips: automation.automationClips, canCreateAutomationClip: document.layers.length > 0,
          automationClipChooser: automation.automationClipChooser, setAutomationClipChooser: automation.setAutomationClipChooser }} />
    </div>)}
  </fieldset>;
}

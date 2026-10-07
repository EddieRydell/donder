import { useSequenceEditorHost } from "../../../editor/host";
import type { SequenceEditorDocument, SequenceGraphNode } from "../../../editor/types";
import type { AutomationClipChooser } from "../shared";
import { TypedParamInput } from "./params/TypedParamInput";
import { LayerProperties, useSequenceEditable } from "./sequenceLayers";
import { NameField } from "./NameField";
import { DescriptionField } from "../DescriptionField";

export function GraphNodeControls({ node, document, automationClipChooser, setAutomationClipChooser }: {
  node: SequenceGraphNode;
  document: SequenceEditorDocument;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
}) {
  const host = useSequenceEditorHost();
  const { commands, runGuiEditCommand } = host;

  const editable = useSequenceEditable();
  if (node.kind.type === "layer") {
    const layerId = node.kind.layerId;
    const layer = document.layers.find((item) => item.id === layerId);
    if (layer === undefined) throw new Error(`Graph layer ${layerId} was not found.`);
    return <div className="graph-node-layer-controls">
      <LayerProperties layer={layer} />
      <DescriptionField description={layer.description} disabled={!editable} onCommit={(description) =>
        runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, {
          type: "setItemDescription", item: { type: "layer", id: layer.id }, description
        }))} />
      <p>{document.effects.filter((effect) => effect.layerId === layer.id).length} effects{layer.isDefault ? " · Default layer" : ""}</p>
    </div>;
  }
  if (node.kind.type !== "operator") return null;
  return <fieldset className="graph-node-parameters" disabled={!editable} aria-label="Project operator parameters">
    <NameField name={node.kind.name} label="Name" commit={(name) =>
      runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, { type: "renameGraphNode", nodeId: node.id, name }))} />
    {node.kind.params.map((param, index) => <div key={param.name} className={`effect-param-row ${index % 2 === 0 ? "effect-param-row-even" : "effect-param-row-odd"}`}>
      <TypedParamInput param={param} commitParam={(name, value) =>
        runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, {
          type: "updateGraphOperatorParam", nodeId: node.id, name, value
        })).then(() => undefined)
      } curveLibrary={document.curveLibrary} gradientLibrary={document.gradientLibrary} markCollections={document.markCollections}
        automation={{ target: { type: "compositionNodeParam", nodeId: node.id, param: param.name },
          automationClips: document.automationClips, canCreateAutomationClip: document.layers.length > 0,
          automationClipChooser, setAutomationClipChooser }} />
    </div>)}
  </fieldset>;
}

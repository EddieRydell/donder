import { useState } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { PreviewDocument } from "../../../types";
import { navigateToGuiObject } from "../../../workspace/navigation";
import { InspectorScrollArea } from "../InspectorScrollArea";
import type { GuiFocus } from "../shared";

type Placement = PreviewDocument["fixtures"][number];

export function LayoutInspector({ document, selected }: { document: PreviewDocument; selected: GuiFocus }) {
  const placement = document.fixtures.find((candidate) => candidate.id === (selected?.type === "placement" ? selected.id : null));
  return <InspectorScrollArea>
    <h2>Layout</h2>
    {placement === undefined ? <p>Select a placement.</p> : <>
      <h3>{placement.name}</h3>
      <a href="#" className="neutral-button" onClick={(event) => { event.preventDefault(); void navigateToGuiObject(placement.definitionRef); }}>Edit definition: {placement.definitionRef.objectKey}</a>
      <button type="button" onClick={() => void runGuiEditCommand((request) => commands.applyPreviewGuiEdit(request, { type: "copyPlacementDefinition", id: placement.id }))}>Make independent fixture copy</button>
      <p>Definition edits affect every placement using it. An independent copy changes only this placement's definition link.</p>
      <div className="setup-element-actions"><button type="button" disabled={document.hierarchy.readOnly} title="Create new elements using the same fixture definition. Outputs are assigned separately." onClick={() => void runGuiEditCommand((request) => commands.applyPreviewGuiEdit(request, { type: "duplicatePlacement", id: placement.id }))}>Duplicate light</button><button type="button" title="Remove only this placement. Keep its elements and output assignments." onClick={() => void runGuiEditCommand((request) => commands.applyPreviewGuiEdit(request, { type: "removePlacement", id: placement.id }))}>Remove placement</button></div>
      <PlacementTransform key={JSON.stringify([placement.id, placement.transform])} placement={placement} />
      <PlacementBindings key={JSON.stringify([placement.id, placement.bindings])} placement={placement} elements={document.hierarchy.elements.filter((element) => element.cellCount !== null)} />
    </>}
  </InspectorScrollArea>;
}

function PlacementTransform({ placement }: { placement: Placement }) {
  const [transform, setTransform] = useState(placement.transform);
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    void runGuiEditCommand((request) => commands.applyPreviewGuiEdit(request, { type: "updatePlacementTransform", id: placement.id, transform }));
  }}>
    <h4>Placement</h4>
    <VectorFields label="Position (meters)" value={transform.position} axes={["xMeters", "yMeters", "zMeters"]} onChange={(position) => { setTransform({ ...transform, position }); }} />
    <VectorFields label="Rotation (degrees)" value={transform.rotation} axes={["xDegrees", "yDegrees", "zDegrees"]} onChange={(rotation) => { setTransform({ ...transform, rotation }); }} />
    <VectorFields label="Scale" value={transform.scale} axes={["x", "y", "z"]} onChange={(scale) => { setTransform({ ...transform, scale }); }} />
    <button type="submit">Apply placement</button>
  </form>;
}

function VectorFields<T extends Record<string, number>>({ label, value, axes, onChange }: { label: string; value: T; axes: (keyof T)[]; onChange: (value: T) => void }) {
  return <fieldset><legend>{label}</legend>{axes.map((axis, index) => <label key={String(axis)}>{["X", "Y", "Z"][index]}<input type="number" step="any" required value={value[axis]} onChange={(event) => { onChange({ ...value, [axis]: Number(event.target.value) }); }} /></label>)}</fieldset>;
}

function PlacementBindings({ placement, elements }: { placement: Placement; elements: PreviewDocument["hierarchy"]["elements"] }) {
  const [bindings, setBindings] = useState(placement.bindings);
  const [node, setNode] = useState(placement.bindings[0]?.node ?? null);
  const [start, setStart] = useState(placement.bindings[0]?.cell ?? 0);
  const element = elements.find((element) => element.id === node);
  return <details className="setup-patch-editor"><summary>Element bindings ({bindings.length})</summary>
    <form onSubmit={(event) => {
      event.preventDefault();
      void runGuiEditCommand((request) => commands.applyPreviewGuiEdit(request, { type: "setPlacementBindings", id: placement.id, bindings }));
    }}>
      <label>Element<select value={node ?? ""} onChange={(event) => { setNode(Number(event.target.value)); }}>{elements.map((element) => <option key={element.id} value={element.id}>{element.name}</option>)}</select></label>
      <label>First cell (zero-based)<input type="number" min={0} step={1} value={start} onChange={(event) => { setStart(Number(event.target.value)); }} /></label>
      <button type="button" disabled={node === null || element === undefined || element.cellCount === null || start + bindings.length > element.cellCount} onClick={() => { if (node !== null) setBindings(bindings.map((_, index) => ({ node, cell: start + index }))); }}>Fill consecutive cells</button>
      {bindings.map((binding, index) => <div className="setup-patch-fields" key={index}>
        <span>Point {index + 1}</span>
        <select aria-label={`Point ${index + 1} element`} value={binding.node} onChange={(event) => { setBindings(bindings.map((item, i) => i === index ? { ...item, node: Number(event.target.value) } : item)); }}>{elements.map((element) => <option key={element.id} value={element.id}>{element.name}</option>)}</select>
        <input aria-label={`Point ${index + 1} cell`} type="number" min={0} step={1} required value={binding.cell} onChange={(event) => { setBindings(bindings.map((item, i) => i === index ? { ...item, cell: Number(event.target.value) } : item)); }} />
      </div>)}
      <button type="submit">Apply bindings</button>
    </form>
  </details>;
}

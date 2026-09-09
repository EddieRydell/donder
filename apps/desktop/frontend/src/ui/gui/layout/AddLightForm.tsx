import { FixtureDefinitionFields } from "../fixture/FixtureDefinitionFields";
import { useState } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import type { Geometry, GuiColorCapability, PreviewDocument, Point3Meters, GuiObjectRef } from "../../../types";
import { ColorCapabilityInput, ReferenceInput } from "../setup/PatchInputs";
const origin: Point3Meters = { xMeters: 0, yMeters: 0, zMeters: 0 };
export function AddLightForm({ document }: { document: PreviewDocument }) {
  const readOnly = useAppStore((state) => state.snapshot?.activeBuffer?.readOnly ?? false);
  const [existing, setExisting] = useState<GuiObjectRef | null>(null);
  const [name, setName] = useState("New light");
  const [capability, setCapability] = useState<GuiColorCapability>({ type: "rgb" });
  const [geometry, setGeometry] = useState<Geometry>({ type: "lines", points: [origin, { ...origin, xMeters: 2 }], pixels: 100 });
  const [x, setX] = useState(0);
  const [y, setY] = useState(0);
  const [parent, setParent] = useState<number | null>(null);
  const [bulbDiameter, setBulbDiameter] = useState(0.04);
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    void runGuiEditCommand((request) => commands.applyPreviewGuiEdit(request, existing === null ? {
      type: "addPixelLight", light: { name, parent, capability, geometry, bulbDiameterMeters: bulbDiameter,
      position: { xMeters: x, yMeters: y, zMeters: 0 } }
    } : { type: "placeFixture", name, parent, capability, definition: existing, position: { xMeters: x, yMeters: y, zMeters: 0 } }));
  }}>
    <h4>Add a pixel light</h4>
    <ColorCapabilityInput value={capability} onChange={setCapability} />
    <label>Name<input value={name} required onChange={(event) => { setName(event.target.value); }} /></label>
    <label>Definition<select value={existing === null ? "new" : "existing"} onChange={(event) => { setExisting(event.target.value === "new" ? null : document.availableFixtures[0] ?? null); }}><option value="new">Create new fixture file</option><option value="existing" disabled={document.availableFixtures.length === 0}>Use existing fixture</option></select></label>
    {existing === null ? <FixtureDefinitionFields geometry={geometry} diameter={bulbDiameter} onGeometryChange={setGeometry} onDiameterChange={setBulbDiameter} /> : <ReferenceInput label="Fixture" value={existing} choices={document.availableFixtures} onChange={setExisting} />}
    <label>X (meters)<input type="number" step="any" value={x} onChange={(event) => { setX(Number(event.target.value)); }} /></label>
    <label>Y (meters)<input type="number" step="any" value={y} onChange={(event) => { setY(Number(event.target.value)); }} /></label>
    <label>Group<select value={parent ?? ""} onChange={(event) => { setParent(event.target.value === "" ? null : Number(event.target.value)); }}>
      <option value="">Top level</option>{document.hierarchy.elements.filter((element) => element.kind === "group").map((element) => <option key={element.id} value={element.id}>{element.name}</option>)}
    </select></label>
    <button type="submit" disabled={document.hierarchy.readOnly || readOnly}>Add light</button>
    {(document.hierarchy.readOnly || readOnly) && <p>This layout or its elements belong to a dependency. Make an independent layout copy from Setup first.</p>}
  </form>;
}


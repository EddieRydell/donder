import type { EditElements } from "./ElementTreeEditor";
import { useState } from "react";
import type { SetupControlElement, ElementTreeGuiDocument } from "../../../types";
import { NumberField, ReferenceInput } from "../setup/PatchInputs";

export function ControlElementEditor({ document, onEdit, id = null, initial = { type: "scalar", cells: 1 }, initialName = "New dimmer" }: { document: ElementTreeGuiDocument; onEdit: EditElements; id?: number | null; initial?: SetupControlElement; initialName?: string }) {
  const [definition, setDefinition] = useState(initial);
  const [name, setName] = useState(initialName);
  const [parent, setParent] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  return <details className="setup-light-editor">
    <summary>{id === null ? "Add dimmer, indexed control, or fixture" : "Edit control element"}</summary>
    <form className="setup-authoring-form" onSubmit={(event) => {
      event.preventDefault();
      void onEdit( id === null
        ? { type: "addControlElement", name, parent, definition }
        : { type: "updateControlElement", id, name, definition })
        .then(() => { setError(null); }).catch((error: unknown) => { setError(String(error)); });
    }}>
      {error !== null && <p role="alert">{error}</p>}
      <fieldset disabled={document.readOnly}>
        <label>Name<input required value={name} onChange={(event) => { setName(event.target.value); }} /></label>
        <label>Element type<select value={definition.type} onChange={(event) => {
          switch (event.target.value) {
            case "scalar": setDefinition({ type: "scalar", cells: 1 }); break;
            case "indexed": setDefinition({ type: "indexed", cells: 1, options: [{ id: 0, name: "Off" }, { id: 1, name: "On" }] }); break;
            case "fixture": { const profile = document.profiles[0]; if (profile !== undefined) setDefinition({ type: "fixture", profile }); break; }
          }
        }}><option value="scalar">Scalar / dimmer</option><option value="indexed">Indexed options</option><option value="fixture" disabled={document.profiles.length === 0}>Fixture profile</option></select></label>
        {definition.type !== "fixture" && <NumberField label="Cells" min={1} value={definition.cells} onChange={(cells) => { setDefinition({ ...definition, cells }); }} />}
        {definition.type === "indexed" && <div className="setup-patch-list">
          {definition.options.map((option, index) => <div className="setup-patch-fields" key={index}>
            <NumberField label="Option ID" value={option.id} onChange={(id) => { setDefinition({ ...definition, options: definition.options.map((option, i) => i === index ? { ...option, id } : option) }); }} />
            <label>Option name<input required value={option.name} onChange={(event) => { setDefinition({ ...definition, options: definition.options.map((option, i) => i === index ? { ...option, name: event.target.value } : option) }); }} /></label>
            <button type="button" onClick={() => { setDefinition({ ...definition, options: definition.options.filter((_, i) => i !== index) }); }}>Remove option</button>
          </div>)}
          <button type="button" onClick={() => { setDefinition({ ...definition, options: [...definition.options, { id: Math.max(-1, ...definition.options.map((option) => option.id)) + 1, name: "New option" }] }); }}>Add option</button>
        </div>}
        {definition.type === "fixture" && <ReferenceInput label="Fixture profile" value={definition.profile} choices={document.profiles} onChange={(profile) => { setDefinition({ ...definition, profile }); }} />}
        {id === null && <label>Group<select value={parent ?? ""} onChange={(event) => { setParent(event.target.value === "" ? null : Number(event.target.value)); }}>
          <option value="">Top level</option>{document.elements.filter((element) => element.kind === "group").map((element) => <option key={element.id} value={element.id}>{element.name}</option>)}
        </select></label>}
        <p>Use the assignment forms under Patching to connect this element to a controller. The advanced patch editor supports custom processing. Changing the cell count resizes guided dimmer and indexed output assignments together. Custom routes, overlapping channels, and changes that invalidate clips are rejected.</p>
        <button type="submit">{id === null ? "Add element" : "Apply element changes"}</button>
      </fieldset>
    </form>
  </details>;
}

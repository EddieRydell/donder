import { useState, type ReactNode } from "react";
import type { ElementTreeGuiDocument, ElementTreeGuiEdit } from "../../../types";
import { ColorLightEditor } from "./ColorLightEditor";
import { ControlElementEditor } from "./ControlElementEditor";
export type EditElements = (edit: ElementTreeGuiEdit) => Promise<unknown>;

export function ElementTreeEditor({ document, onEdit }: { document: ElementTreeGuiDocument; onEdit: EditElements }) {
  const elements = orderElements(document);
  return (
    <main className="element-tree-editor">
      <SetupSection title="Fixture instances & controls">
        <ControlElementEditor document={document} onEdit={onEdit} />
        <GroupForm document={document} onEdit={onEdit} />
        {elements.length === 0 ? <p className="object-overview-empty">No fixture instances or controls yet. Add a fixture instance from Layout or create a control here.</p> : <div className="setup-table">{elements.map((element) => (
          <div className="setup-row" key={`${element.id}:${element.name}:${element.cellCount ?? "group"}`}>
            <span className="setup-id">{element.id}</span>
            <input defaultValue={element.name} disabled={document.readOnly} aria-label={`Element ${element.id} name`} onBlur={(event) => { if (event.currentTarget.value !== element.name) void onEdit({ type: "renameElement", id: element.id, name: event.currentTarget.value }); }} />
            <span>{element.kind}</span>
            {element.cellCount !== null ? <span>{element.cellCount} {element.kind === "color" ? "pixels" : "cells"}</span> : <span>{element.children.length} children</span>}
            <span>{element.capability?.type.toUpperCase() ?? element.profile ?? ""}</span>
            <ElementActions document={document} onEdit={onEdit} element={element} />
            {element.capability !== null && <ColorLightEditor key={JSON.stringify(element.capability)} document={document} onEdit={onEdit} node={element.id} initial={element.capability} />}
            {element.controlDefinition !== null && <ControlElementEditor key={JSON.stringify([element.name, element.controlDefinition])} document={document} onEdit={onEdit} id={element.id} initial={element.controlDefinition} initialName={element.name} />}
          </div>
        ))}</div>}
      </SetupSection>
    </main>
  );
}

function GroupForm({ document, onEdit }: { document: ElementTreeGuiDocument; onEdit: EditElements }) {
  const [name, setName] = useState("");
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    void onEdit({ type: "addGroup", name, parent: null });
  }}>
    <label>New group<input required value={name} onChange={(event) => { setName(event.target.value); }} /></label>
    <button type="submit" disabled={document.readOnly}>Add group</button>
  </form>;
}

function ElementActions({ document, element, onEdit }: { document: ElementTreeGuiDocument; element: ElementTreeGuiDocument["elements"][number]; onEdit: EditElements }) {
  const parent = document.elements.find((candidate) => candidate.id === element.parent);
  const ordered = parent === undefined ? document.rootIds : parent.children;
  const index = ordered.indexOf(element.id);
  const reorder = (direction: -1 | 1) => {
    const next = ordered.slice();
    const other = next[index + direction];
    if (other === undefined) return;
    next[index] = other;
    next[index + direction] = element.id;
    void onEdit({ type: "reorderElements", parent: element.parent, orderedIds: next });
  };
  return <div className="setup-element-actions">
    <label>Group<select disabled={document.readOnly} value={element.parent ?? ""} onChange={(event) => {
      const parent = event.target.value === "" ? null : Number(event.target.value);
      void onEdit({ type: "moveElement", id: element.id, parent });
    }}><option value="">Top level</option>{document.elements.filter((candidate) => candidate.kind === "group" && candidate.id !== element.id).map((candidate) => <option key={candidate.id} value={candidate.id}>{candidate.name}</option>)}</select></label>
    <button type="button" disabled={document.readOnly || index <= 0} onClick={() => { reorder(-1); }}>Move up</button>
    <button type="button" disabled={document.readOnly || index < 0 || index >= ordered.length - 1} onClick={() => { reorder(1); }}>Move down</button>
    {element.controlDefinition !== null && <button type="button" disabled={document.readOnly} title="Duplicate the control definition without preview bindings or output assignments." onClick={() => {
      const definition = element.controlDefinition;
      if (definition !== null) void onEdit({ type: "addControlElement", name: `${element.name} copy`, parent: element.parent, definition });
    }}>Duplicate control</button>}
    <button type="button" disabled={document.readOnly} onClick={() => {
      void onEdit({ type: "deleteElement", id: element.id });
    }}>Delete</button>
  </div>;
}

function orderElements(document: ElementTreeGuiDocument): ElementTreeGuiDocument["elements"] {
  const nodes = new Map(document.elements.map((element) => [element.id, element]));
  const ordered: ElementTreeGuiDocument["elements"] = [];
  const pending = document.rootIds.slice().reverse();
  while (pending.length > 0) {
    const id = pending.pop();
    const element = id === undefined ? undefined : nodes.get(id);
    if (element === undefined) throw new Error("Element hierarchy references a missing element.");
    ordered.push(element);
    pending.push(...element.children.slice().reverse());
  }
  return ordered;
}


function SetupSection({ title, children }: { title: string; children: ReactNode }) { return <section className="setup-section"><h3>{title}</h3>{children}</section>; }

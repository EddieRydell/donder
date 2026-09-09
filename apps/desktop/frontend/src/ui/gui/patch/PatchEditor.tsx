import { useState } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import type { GuiDocumentRequest, PatchGuiNodeDefinition, PatchGuiDocument } from "../../../types";
import { PatchFilterInput } from "../setup/PatchFilterInput";
import { NumberField, ReferenceInput } from "../setup/PatchInputs";

type Edge = PatchGuiDocument["edges"][number];
type Draft = { origin: GuiDocumentRequest; nodes: PatchGuiDocument["nodes"]; edges: Edge[] };

export function PatchEditor({ document }: { document: PatchGuiDocument }) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const request = useAppStore((state) => state.guiRequest);
  const documentRevision = useAppStore((state) => state.guiDocumentRevision);
  const pending = useAppStore((state) => state.guiEditPending);
  const stale = draft !== null && draft.origin !== request;
  const begin = () => {
    if (request === null || documentRevision !== request.projectRevision) return;
    setDraft({ origin: request, nodes: structuredClone(document.nodes), edges: structuredClone(document.edges) });
    setError(null);
  };
  const addNode = (definition: PatchGuiNodeDefinition) => {
    if (draft === null) return;
    const id = Math.max(0, ...draft.nodes.map((node) => node.id)) + 1;
    setDraft({ ...draft, nodes: [...draft.nodes, { id, definition }] });
  };
  return <main className="setup-editor"><header className="object-overview-header"><div><span className="object-overview-eyebrow">{document.path}</span><h2>{document.objectKey}</h2></div></header><section className="setup-section">
    {draft === null ? <>
      <p>{document.nodes.length} nodes, {document.edges.length} connections. Edit a draft, then apply all changes together. Changes affect every setup using this patch.</p>
      <button type="button" disabled={pending || request === null || documentRevision !== request.projectRevision} onClick={begin}>Edit patch</button>
    </> : <form onSubmit={(event) => {
      event.preventDefault();
      void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "patch", nodes: draft.nodes, edges: draft.edges }), draft.origin)
        .then(() => { setDraft(null); setError(null); })
        .catch((error: unknown) => { setError(String(error)); });
    }}>
      <p>The draft is not saved or sent to outputs until Apply succeeds. Node ports and cell offsets start at zero; controller channels start at one.</p>
      {stale && <p role="alert">The project changed while this draft was open. Discard the draft and start again from the current patch.</p>}
      {error !== null && <p role="alert">{error}</p>}
      <fieldset disabled={stale || pending}>
        <div className="setup-patch-actions">
          <button type="button" disabled={!document.elementTrees.some((tree) => tree.elements.length > 0)} onClick={() => {
            const tree = document.elementTrees.find((tree) => tree.elements.length > 0);
            const element = tree?.elements[0];
            if (tree !== undefined && element !== undefined) addNode({ type: "source", tree: tree.sourceRef, node: element.id, cells: null, output: { type: "color", width: element.cellCount } });
          }}>Add element source</button>
          <button type="button" onClick={() => { addNode({ type: "filter", filter: { type: "quantize8", width: 1 } }); }}>Add filter</button>
          <button type="button" disabled={document.controllers.length === 0} onClick={() => {
            const controller = document.controllers[0];
            const port = controller?.ports[0];
            if (controller !== undefined && port !== undefined) addNode({ type: "sink", controller: controller.sourceRef, port: port.id, startSlot: 0, slotCount: 3 });
          }}>Add controller output</button>
        </div>
        <div className="setup-patch-list">
          {draft.nodes.map((node) => <section className="setup-patch-node" key={node.id}>
            <header><strong>Node {node.id}: {node.definition.type}</strong><button type="button" onClick={() => {
              setDraft({ ...draft, nodes: draft.nodes.filter((candidate) => candidate.id !== node.id), edges: draft.edges.filter((edge) => edge.fromNode !== node.id && edge.toNode !== node.id) });
            }}>Remove node and connections</button></header>
            <PatchNodeInput document={document} value={node.definition} onChange={(definition) => { setDraft({ ...draft, nodes: draft.nodes.map((candidate) => candidate.id === node.id ? { ...candidate, definition } : candidate) }); }} />
          </section>)}
        </div>
        <h4>Connections</h4>
        {draft.edges.map((edge, index) => <div className="setup-patch-fields" key={index}>
          <NodeChoice label="From node" nodes={draft.nodes} value={edge.fromNode} onChange={(fromNode) => { setDraft({ ...draft, edges: draft.edges.map((edge, i) => i === index ? { ...edge, fromNode } : edge) }); }} />
          <NumberField label="Output port" value={edge.fromPort} onChange={(fromPort) => { setDraft({ ...draft, edges: draft.edges.map((edge, i) => i === index ? { ...edge, fromPort } : edge) }); }} />
          <NodeChoice label="To node" nodes={draft.nodes} value={edge.toNode} onChange={(toNode) => { setDraft({ ...draft, edges: draft.edges.map((edge, i) => i === index ? { ...edge, toNode } : edge) }); }} />
          <NumberField label="Input port" value={edge.toPort} onChange={(toPort) => { setDraft({ ...draft, edges: draft.edges.map((edge, i) => i === index ? { ...edge, toPort } : edge) }); }} />
          <button type="button" onClick={() => { setDraft({ ...draft, edges: draft.edges.filter((_, i) => i !== index) }); }}>Remove connection</button>
        </div>)}
        <div className="setup-patch-actions">
          <button type="button" disabled={draft.nodes.length < 2} onClick={() => {
            const from = draft.nodes[0]; const to = draft.nodes[1];
            if (from !== undefined && to !== undefined) setDraft({ ...draft, edges: [...draft.edges, { fromNode: from.id, fromPort: 0, toNode: to.id, toPort: 0 }] });
          }}>Add connection</button>
          <button type="submit">Apply patch</button>
        </div>
      </fieldset>
      <button type="button" disabled={pending} onClick={() => { setDraft(null); setError(null); }}>Discard draft</button>
    </form>}
  </section></main>;
}

function NodeChoice({ label, nodes, value, onChange }: { label: string; nodes: Draft["nodes"]; value: number; onChange: (value: number) => void }) {
  return <label>{label}<select value={value} onChange={(event) => { onChange(Number(event.target.value)); }}>
    {nodes.map((node) => <option value={node.id} key={node.id}>{node.id}: {node.definition.type}</option>)}
  </select></label>;
}

function PatchNodeInput({ document, value, onChange }: { document: PatchGuiDocument; value: PatchGuiNodeDefinition; onChange: (value: PatchGuiNodeDefinition) => void }) {
  if (value.type === "filter") return <PatchFilterInput value={value.filter} profiles={document.profiles} onChange={(filter) => { onChange({ ...value, filter }); }} />;
  if (value.type === "sink") return <div className="setup-patch-fields">
    <ReferenceInput label="Controller" value={value.controller} choices={document.controllers.map((controller) => controller.sourceRef)} onChange={(controller) => { onChange({ ...value, controller }); }} />
    <NumberField label="Controller port ID" value={value.port} onChange={(port) => { onChange({ ...value, port }); }} />
    <NumberField label="First channel" value={value.startSlot + 1} min={1} onChange={(channel) => { onChange({ ...value, startSlot: channel - 1 }); }} />
    <NumberField label="Channel count" value={value.slotCount} min={1} onChange={(slotCount) => { onChange({ ...value, slotCount }); }} />
  </div>;
  const range = value.cells;
  const tree = document.elementTrees.find((tree) => tree.sourceRef.moduleId === value.tree.moduleId && tree.sourceRef.path === value.tree.path && tree.sourceRef.objectKey === value.tree.objectKey);
  return <div className="setup-patch-fields">
    <ReferenceInput label="Element tree" value={value.tree} choices={document.elementTrees.filter((tree) => tree.elements.length > 0).map((tree) => tree.sourceRef)} onChange={(reference) => {
      const next = document.elementTrees.find((tree) => tree.sourceRef === reference)?.elements[0];
      if (next !== undefined) onChange({ ...value, tree: reference, node: next.id, cells: null });
    }} />
    <label>Element<select value={value.node} onChange={(event) => { onChange({ ...value, node: Number(event.target.value) }); }}>
      {(tree?.elements ?? []).map((element) => <option key={element.id} value={element.id}>{element.name}</option>)}
    </select></label>
    <label><input type="checkbox" checked={range !== null} onChange={(event) => { onChange({ ...value, cells: event.target.checked ? { start: 0, count: value.output.width } : null }); }} />Select a cell range</label>
    {range !== null && <>
      <NumberField label="First cell (zero-based)" value={range.start} onChange={(start) => { onChange({ ...value, cells: { ...range, start } }); }} />
      <NumberField label="Cell count" value={range.count} min={1} onChange={(count) => { onChange({ ...value, cells: { ...range, count } }); }} />
    </>}
    <label>Element value type<select value={value.output.type} onChange={(event) => {
      const width = value.output.width;
      switch (event.target.value) {
        case "color": onChange({ ...value, output: { type: "color", width } }); break;
        case "scalar": onChange({ ...value, output: { type: "scalar", width } }); break;
        case "indexed": onChange({ ...value, output: { type: "indexed", width } }); break;
        case "fixtureState": { const profile = document.profiles[0]; if (profile !== undefined) onChange({ ...value, output: { type: "fixtureState", width, profile } }); break; }
      }
    }}><option value="color">Color</option><option value="scalar">Scalar</option><option value="indexed">Indexed</option><option value="fixtureState" disabled={document.profiles.length === 0}>Fixture state</option>
      {(value.output.type === "components" || value.output.type === "slots") && <option value={value.output.type} disabled>{value.output.type} (invalid source type)</option>}
    </select></label>
    <NumberField label="Output values" value={value.output.width} min={1} onChange={(width) => { onChange({ ...value, output: { ...value.output, width } }); }} />
    {value.output.type === "fixtureState" && <ReferenceInput label="Fixture profile" value={value.output.profile} choices={document.profiles} onChange={(profile) => { onChange({ ...value, output: { type: "fixtureState", width: value.output.width, profile } }); }} />}
  </div>;
}

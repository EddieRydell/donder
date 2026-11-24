import { SpatialSelectionControls } from "./SpatialSelectionControls";
import { isSecondaryClick } from "../../../platform";
import { fixtureItems, selectedItems, selectionClick, plus, type SpatialMove } from "./spatialSelection";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { useState, type ReactNode } from "react";
import { ArrowDown, ArrowUp, Grip, X } from "lucide-react";
import { commands } from "../../../api";
import { runGuiEditCommand, showRefusedEdit, useAppStore } from "../../../store";
import { useFieldProblem } from "../problems";
import type { FixtureGuiDocument, FixtureGuiEdit, GuiFixtureElement, GuiFixtureShape, Point3Meters } from "../../../types";
import { fixtureMenuEntries, fixtureTools, type FixtureTool } from "./FixtureContextMenu";
import { Placement } from "./FixtureFields";
import { AddRow, Button, ContextMenuArea, Field, FieldGroup, IconButton, NumberField, TextField, Toggle } from "../../kit";
import { FixtureCapture } from "./FixtureCapture";
import { FixtureShapeFields } from "./FixtureShapeFields";
import { SpatialCanvas } from "./SpatialCanvas";
import { spatialUnits } from "./spatialSnapping";

const millimeters = spatialUnits.millimeters;

export function FixtureEditor({ document }: { document: FixtureGuiDocument }) {
  const [selection, setSelection] = useState<number[]>([]);
  const selected = selection.length === 1 ? selection[0] ?? null : null;
  const setSelected = (id: number | null) => { setSelection(id === null ? [] : [id]); };
  const [tool, setTool] = useState<FixtureTool | null>(null);
  const [toolSession, setToolSession] = useState(0);
  const beginTool = (tool: FixtureTool) => { setToolSession((session) => session + 1); setTool(tool); };
  const [count, setCount] = useState(30);
  const [rows, setRows] = useState(8);
  const [columns, setColumns] = useState(8);
  const [showOrder, setShowOrder] = useState(true);
  const [dragged, setDragged] = useState<number | null>(null);
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const pending = useAppStore((state) => state.guiEditPending);
  const editable = request !== null && request.projectRevision === revision && !pending;
  const elements = document.elements;
  const items = fixtureItems(elements, document.renderPlan);
  const chosen = selectedItems(items, selection);
  const index = elements.findIndex((element) => element.id === selected);
  const element = elements[index];
  /** Whether the edit was applied. */
  const edit = async (edit: FixtureGuiEdit) => await runGuiEditCommand((current) => commands.applyGuiEdit(current, { type: "fixture", edit: edit }), request) !== null;
  const change = (elements: GuiFixtureElement[]) => { void edit({ type: "setElements", elements }); };
  const update = (element: GuiFixtureElement) => { change(elements.map((item) => item.id === element.id ? element : item)); };
  const move = (from: number, to: number) => {
    if (from < 0 || to < 0 || to >= elements.length || from === to) return;
    const next = [...elements];
    const item = next.splice(from, 1)[0];
    if (item !== undefined) { next.splice(to, 0, item); change(next); }
  };
  const nextId = () => elements.reduce((highest, element) => Math.max(highest, element.id), 0) + 1;
  const remove = () => { change(elements.filter((item) => !selection.includes(item.id))); setSelection([]); };
  const moveSelection = (moves: SpatialMove[]) => edit({ type: "setElements", elements: elements.map((element) => {
    const delta = moves.find((move) => move.id === element.id)?.delta;
    return delta === undefined ? element : { ...element, transform: { ...element.transform, position: plus(element.transform.position, delta) } };
  }) });
  const repeat = (offsets: Point3Meters[]) => {
    let id = nextId();
    const copies = offsets.flatMap((offset) => elements.filter((element) => selection.includes(element.id)).map((element) => ({ ...element, id: id++, name: `${element.name} copy`, transform: { ...element.transform, position: plus(element.transform.position, offset) } })));
    void edit({ type: "setElements", elements: [...elements, ...copies] }).then((applied) => { if (applied) setSelection(copies.map((copy) => copy.id)); });
  };
  const duplicate = () => { repeat([{ xMeters: 0, yMeters: 0, zMeters: 0 }]); };
  const draw = async (tool: FixtureTool, points: Point3Meters[]) => {
    const start = points[0]; const end = points[points.length - 1];
    if (start === undefined || end === undefined) return;
    const dx = end.xMeters - start.xMeters; const dy = end.yMeters - start.yMeters;
    const length = Math.hypot(dx, dy);
    if (tool !== "pixel" && tool !== "polyline" && length === 0) { showRefusedEdit({ message: "Drag to give the shape a size.", site: null }); return; }
    let position = start;
    let rotation = 0;
    let shape: GuiFixtureShape;
    switch (tool) {
      case "pixel": shape = { type: "pixel" }; break;
      case "line": shape = { type: "line", length, count }; rotation = Math.atan2(dy, dx) * 180 / Math.PI; break;
      case "polyline": shape = { type: "polyline", points: points.map((point) => ({ xMeters: point.xMeters - start.xMeters, yMeters: point.yMeters - start.yMeters, zMeters: point.zMeters - start.zMeters })), count }; break;
      case "arc": case "circle": shape = { type: "arc", radius: length, startDegrees: Math.atan2(dy, dx) * 180 / Math.PI, sweepDegrees: tool === "circle" ? 360 : 180, closed: tool === "circle", count }; break;
      case "grid":
        position = { xMeters: Math.min(start.xMeters, end.xMeters), yMeters: Math.min(start.yMeters, end.yMeters), zMeters: start.zMeters };
        shape = { type: "grid", width: Math.abs(dx), height: Math.abs(dy), rows, columns, axis: "rows", corner: "bottomLeft", serpentine: true }; break;
    }
    const id = nextId();
    const added: GuiFixtureElement = { id, name: `${tool.charAt(0).toUpperCase() + tool.slice(1)} ${id}`, shape, diameterMeters: 0.01, reverse: false, transform: { position, rotation: { xDegrees: 0, yDegrees: 0, zDegrees: rotation }, scale: { x: 1, y: 1, z: 1 } } };
    if (!await edit({ type: "setElements", elements: [...elements, added] })) return;
    setSelected(id); setTool(null);
  };
  const ranges = new Map<number, { count: number; start: number; end: number }>();
  for (const pixel of document.renderPlan.pixels) {
    const range = ranges.get(pixel.owner);
    if (range === undefined) ranges.set(pixel.owner, { count: 1, start: pixel.index + 1, end: pixel.index + 1 });
    else { range.count++; range.end = pixel.index + 1; }
  }
  return <div className="layout-authoring">
    <ContextMenuArea entries={fixtureMenuEntries({ enabled: editable, onTool: beginTool, selected: selection.length > 0, onDuplicate: duplicate, onDelete: remove })}><aside className="layout-hierarchy layout-tree-sidebar fixture-editor-sidebar" onContextMenuCapture={(event) => {
      const row = event.target instanceof Element ? event.target.closest("[data-shape-id]") : null;
      const id = row === null ? null : Number(row.getAttribute("data-shape-id"));
      if (id !== null && !selection.includes(id)) setSelected(id);
    }}>
      <h2 className="composition-title">{document.name}</h2>
      <fieldset className="composition-controls" disabled={!editable}>
        <div className="fixture-shape-actions" aria-label="Draw a shape">{fixtureTools.map((item) => <IconButton key={item.type} icon={item.icon} label={`${item.label}: ${item.instruction}`} pressed={tool === item.type} onClick={() => { beginTool(item.type); }} />)}</div>
        {tool !== null && <div className="fixture-tool-options">
          {tool === "grid"
            ? <Field label="Size"><FieldGroup>
              <NumberField label="C" name="Columns" integer step={1} min={1} max={1000000} disabled={!editable} value={columns} onCommit={setColumns} />
              <NumberField label="R" name="Rows" integer step={1} min={1} max={1000000} disabled={!editable} value={rows} onCommit={setRows} />
            </FieldGroup></Field>
            : tool !== "pixel" && <Field label="Pixels"><NumberField name="Pixels" integer step={1} min={1} max={1000000} disabled={!editable} value={count} onCommit={setCount} /></Field>}
          <Button icon={X} command="edit.cancel" tip="Cancel drawing" onClick={() => { setTool(null); }}>Cancel</Button>
        </div>}
        <div className="composition-tree" aria-label="Shapes in output order">
          {elements.length === 0 && <AddRow label="Draw a line" disabled={!editable} onAdd={() => { beginTool("line"); }} />}
          {elements.map((item, index) => <button type="button" className="fixture-shape-row" data-shape-id={item.id} key={item.id} aria-pressed={selection.includes(item.id)} draggable={editable} onDragStart={() => { setDragged(index); }} onDragEnd={() => { setDragged(null); }} onDragOver={(event) => { if (dragged !== null) event.preventDefault(); }} onDrop={(event) => { event.preventDefault(); if (dragged !== null) move(dragged, index); setDragged(null); }} onClick={(event) => { if (isSecondaryClick(event)) return; setSelection(selectionClick(selection, item.id, event.shiftKey || event.ctrlKey || event.metaKey)); }}>
            <span>{item.name}</span><span className="composition-tree-meta">{ranges.get(item.id)?.count} px · {ranges.get(item.id)?.start}–{ranges.get(item.id)?.end}</span>
          </button>)}
        </div>
        <SpatialSelectionControls items={chosen} disabled={!editable} onMove={(moves) => { void moveSelection(moves); }} onRepeat={repeat} onDuplicate={duplicate} onDelete={remove} />
        {element !== undefined && <div className="fixture-shape-fields" key={element.id}>
          <ShapeName name={element.name} id={element.id} onCommit={(name) => { update({ ...element, name }); }}>
            <IconButton icon={ArrowUp} label="Move shape earlier" disabled={index <= 0} onClick={() => { move(index, index - 1); }} />
            <IconButton icon={ArrowDown} label="Move shape later" disabled={index === elements.length - 1} onClick={() => { move(index, index + 1); }} />
          </ShapeName>
          <FixtureShapeFields shape={element.shape} disabled={!editable} onChange={(shape) => { update({ ...element, shape }); }} />
          <Field label="Pixel size" tip="Diameter of each pixel, as the layout and preview draw it">
            <NumberField name="Pixel size" unit={millimeters.label} step={1} min={0.001} max={100000} disabled={!editable} value={element.diameterMeters / millimeters.meters} onCommit={(size) => { update({ ...element, diameterMeters: size * millimeters.meters }); }} />
          </Field>
          {element.shape.type !== "pixel" && <Field label="Reverse" tip="Reverse pixel order">
            <Toggle name="Reverse pixel order" checked={element.reverse} onChange={(reverse) => { update({ ...element, reverse }); }} />
          </Field>}
          <Placement value={element.transform} disabled={!editable} problem={null} onChange={(transform) => { update({ ...element, transform }); }} />
          {element.shape.type !== "pixel" && <Button icon={Grip} tip="Replace the shape with its individual pixels" onClick={() => { void edit({ type: "convertToPixels", id: element.id }).then((applied) => { if (applied) setSelected(null); }); }}>To pixels</Button>}
        </div>}
      </fieldset>
      <FixtureCapture document={document} disabled={!editable} />
      <Toggle name="Show pixel order" label="Pixel order" checked={showOrder} onChange={setShowOrder} />
    </aside></ContextMenuArea>
    <SpatialCanvas reference={document.sourceRef} plan={document.renderPlan} documentKey={guiObjectKey(document.sourceRef)} selection={selection} items={items} onSelect={setSelection} onDelete={remove} onDuplicate={duplicate}
      onMoveStart={(ids, anchor) => {
        const chosen = selectedItems(items, ids);
        const origin = items.find((item) => item.id === anchor)?.origin;
        return editable && origin !== undefined ? { origin, owners: chosen.flatMap((item) => item.owners), commit: (delta) => moveSelection(chosen.map((item) => ({ id: item.id, delta }))) } : null;
      }}
      fixtureTools={{ enabled: editable, tool, session: toolSession, handles: document.handles, elements, showOrder, onTool: beginTool, onDuplicate: duplicate, onDelete: remove, onCancel: () => { setTool(null); }, onDraw: (tool, points) => { void draw(tool, points); }, onHandleStart: (id, index) => editable ? (position) => edit({ type: "moveHandle", id, index, position }) : null }}
    />
  </div>;
}

/** A shape's name with its order buttons; a refused edit of the shape marks it. */
function ShapeName({ name, id, onCommit, children }: { name: string; id: number; onCommit: (name: string) => void; children: ReactNode }) {
  const problem = useFieldProblem({ type: "shape", id }, null);
  return <Field label="Name" error={problem}>
    <TextField name="Shape name" value={name} invalid={problem !== null} onCommit={(value) => { const next = value.trim(); if (next !== "" && next !== name) onCommit(next); }} />
    {children}
  </Field>;
}

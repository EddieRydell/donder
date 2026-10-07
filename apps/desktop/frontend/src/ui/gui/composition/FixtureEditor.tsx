import { SpatialSelectionControls } from "./SpatialSelectionControls";
import { isSecondaryClick } from "../../../platform";
import { fixtureItems, selectedItems, selectionClick, plus, type SpatialMove } from "./spatialSelection";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { useState } from "react";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { ArrowDown, ArrowUp } from "lucide-react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import { THEME_METRICS } from "../../../theme";
import type { FixtureGuiDocument, FixtureGuiEdit, GuiFixtureElement, GuiFixtureShape, Point3Meters } from "../../../types";
import { FixtureContextMenu, fixtureTools, type FixtureTool } from "./FixtureContextMenu";
import { CoordinateField, Placement } from "./FixtureFields";
import { FixtureShapeFields } from "./FixtureShapeFields";
import { SpatialCanvas } from "./SpatialCanvas";

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
  const [error, setError] = useState<string | null>(null);
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const pending = useAppStore((state) => state.guiEditPending);
  const readOnly = useAppStore((state) => state.snapshot?.activeBuffer?.readOnly ?? false);
  const editable = request !== null && request.projectRevision === revision && !pending && !readOnly;
  const elements = document.elements;
  const items = fixtureItems(elements, document.renderPlan);
  const chosen = selectedItems(items, selection);
  const index = elements.findIndex((element) => element.id === selected);
  const element = elements[index];
  const report = (error: unknown) => { setError(String(error)); };
  const edit = async (edit: FixtureGuiEdit) => {
    await runGuiEditCommand((current) => commands.applyFixtureGuiEdit(current, edit), request);
    setError(null);
  };
  const change = (elements: GuiFixtureElement[]) => { void edit({ type: "setElements", elements }).catch(report); };
  const update = (element: GuiFixtureElement) => { change(elements.map((item) => item.id === element.id ? element : item)); };
  const move = (from: number, to: number) => {
    if (from < 0 || to < 0 || to >= elements.length || from === to) return;
    const next = [...elements];
    const item = next.splice(from, 1)[0];
    if (item !== undefined) { next.splice(to, 0, item); change(next); }
  };
  const nextId = () => elements.reduce((highest, element) => Math.max(highest, element.id), 0) + 1;
  const remove = () => { change(elements.filter((item) => !selection.includes(item.id))); setSelection([]); };
  const moveSelection = async (moves: SpatialMove[]) => {
    await edit({ type: "setElements", elements: elements.map((element) => {
      const delta = moves.find((move) => move.id === element.id)?.delta;
      return delta === undefined ? element : { ...element, transform: { ...element.transform, position: plus(element.transform.position, delta) } };
    }) });
  };
  const repeat = (offsets: Point3Meters[]) => {
    let id = nextId();
    const copies = offsets.flatMap((offset) => elements.filter((element) => selection.includes(element.id)).map((element) => ({ ...element, id: id++, name: `${element.name} copy`, transform: { ...element.transform, position: plus(element.transform.position, offset) } })));
    void edit({ type: "setElements", elements: [...elements, ...copies] }).then(() => { setSelection(copies.map((copy) => copy.id)); }).catch(report);
  };
  const duplicate = () => { repeat([{ xMeters: 0, yMeters: 0, zMeters: 0 }]); };
  const draw = async (tool: FixtureTool, points: Point3Meters[]) => {
    const start = points[0]; const end = points[points.length - 1];
    if (start === undefined || end === undefined) return;
    const dx = end.xMeters - start.xMeters; const dy = end.yMeters - start.yMeters;
    const length = Math.hypot(dx, dy);
    if (tool !== "pixel" && tool !== "polyline" && length === 0) { setError("Drag to give the shape a size."); return; }
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
    await edit({ type: "setElements", elements: [...elements, added] });
    setSelected(id); setTool(null);
  };
  const ranges = new Map<number, { count: number; start: number; end: number }>();
  for (const pixel of document.renderPlan.pixels) {
    const range = ranges.get(pixel.owner);
    if (range === undefined) ranges.set(pixel.owner, { count: 1, start: pixel.index + 1, end: pixel.index + 1 });
    else { range.count++; range.end = pixel.index + 1; }
  }
  return <div className="layout-authoring">
    <ContextMenu.Root><ContextMenu.Trigger asChild><aside className="layout-hierarchy layout-tree-sidebar fixture-editor-sidebar" onContextMenuCapture={(event) => {
      const row = event.target instanceof Element ? event.target.closest("[data-shape-id]") : null;
      const id = row === null ? null : Number(row.getAttribute("data-shape-id"));
      if (id !== null && !selection.includes(id)) setSelected(id);
    }}>
      <h2 className="composition-title">{document.name}</h2>
      <fieldset className="composition-controls" disabled={!editable}>
        <div className="fixture-drawing-tools" aria-label="Draw a shape">{fixtureTools.map((item) => <button type="button" key={item.type} aria-pressed={tool === item.type} onClick={() => { beginTool(item.type); }}>{item.label}</button>)}</div>
        {tool !== null && <div className="fixture-tool-options">
          <p>{fixtureTools.find((item) => item.type === tool)?.instruction}</p>
          {tool === "grid" ? <><CoordinateField label="Columns" value={columns} min={1} max={1000000} step={1} onChange={setColumns} /><CoordinateField label="Rows" value={rows} min={1} max={1000000} step={1} onChange={setRows} /></> : tool !== "pixel" && <CoordinateField label="Pixels" value={count} min={1} max={1000000} step={1} onChange={setCount} />}
          <button type="button" onClick={() => { setTool(null); }}>Cancel drawing</button>
        </div>}
        <div className="composition-tree" aria-label="Shapes in output order">
          {elements.length === 0 && <p className="composition-tree-empty">Choose a shape, then draw it on the canvas.</p>}
          {elements.map((item, index) => <button type="button" className="fixture-shape-row" data-shape-id={item.id} key={item.id} aria-pressed={selection.includes(item.id)} draggable={editable} onDragStart={() => { setDragged(index); }} onDragEnd={() => { setDragged(null); }} onDragOver={(event) => { if (dragged !== null) event.preventDefault(); }} onDrop={(event) => { event.preventDefault(); if (dragged !== null) move(dragged, index); setDragged(null); }} onClick={(event) => { if (isSecondaryClick(event)) return; setSelection(selectionClick(selection, item.id, event.shiftKey || event.ctrlKey || event.metaKey)); }}>
            <span>{item.name}</span><span className="composition-tree-meta">{ranges.get(item.id)?.count} pixels · {ranges.get(item.id)?.start}–{ranges.get(item.id)?.end}</span>
          </button>)}
        </div>
        <SpatialSelectionControls items={chosen} disabled={!editable} onMove={(moves) => { void moveSelection(moves).catch(report); }} onRepeat={repeat} onDuplicate={duplicate} onDelete={remove} />
        {element !== undefined && <>
          <div className="fixture-shape-actions"><button type="button" aria-label="Move shape earlier" disabled={index <= 0} onClick={() => { move(index, index - 1); }}><ArrowUp size={THEME_METRICS.iconSizeSmall} /></button><button type="button" aria-label="Move shape later" disabled={index === elements.length - 1} onClick={() => { move(index, index + 1); }}><ArrowDown size={THEME_METRICS.iconSizeSmall} /></button></div>
          <div className="fixture-shape-fields" key={element.id}>
            <label>Name<input key={element.name} required defaultValue={element.name} onBlur={(event) => { const name = event.currentTarget.value.trim(); if (name !== "" && name !== element.name) update({ ...element, name }); else event.currentTarget.value = element.name; }} /></label>
            <FixtureShapeFields shape={element.shape} onChange={(shape) => { update({ ...element, shape }); }} />
            <CoordinateField label="Pixel diameter (m)" value={element.diameterMeters} min={0.000001} max={100} onChange={(diameterMeters) => { update({ ...element, diameterMeters }); }} />
            {element.shape.type !== "pixel" && <label className="fixture-checkbox"><input type="checkbox" checked={element.reverse} onChange={(event) => { update({ ...element, reverse: event.target.checked }); }} />Reverse pixel order</label>}
            <Placement value={element.transform} onChange={(transform) => { update({ ...element, transform }); }} />
            {element.shape.type !== "pixel" && <button type="button" onClick={() => { void edit({ type: "convertToPixels", id: element.id }).then(() => { setSelected(null); }).catch(report); }}>Convert to individual pixels</button>}
          </div>
        </>}
      </fieldset>
      <label className="fixture-checkbox"><input type="checkbox" checked={showOrder} onChange={(event) => { setShowOrder(event.target.checked); }} />Show pixel order</label>
      {error !== null && <p role="alert">{error}</p>}
    </aside></ContextMenu.Trigger><ContextMenu.Portal><ContextMenu.Content className="menu-content">
      <FixtureContextMenu enabled={editable} onTool={beginTool} selected={selection.length > 0} onDuplicate={duplicate} onDelete={remove} />
    </ContextMenu.Content></ContextMenu.Portal></ContextMenu.Root>
    <SpatialCanvas reference={document.sourceRef} plan={document.renderPlan} documentKey={guiObjectKey(document.sourceRef)} selection={selection} items={items} onSelect={setSelection} onDelete={remove} onDuplicate={duplicate}
      onMoveStart={(ids, anchor) => {
        const chosen = selectedItems(items, ids);
        const origin = items.find((item) => item.id === anchor)?.origin;
        return editable && origin !== undefined ? { origin, owners: chosen.flatMap((item) => item.owners), commit: async (delta) => { try { await moveSelection(chosen.map((item) => ({ id: item.id, delta }))); return true; } catch (error) { report(error); return false; } } } : null;
      }}
      fixtureTools={{ enabled: editable, tool, session: toolSession, handles: document.handles, elements, showOrder, onTool: beginTool, onDuplicate: duplicate, onDelete: remove, onCancel: () => { setTool(null); }, onDraw: (tool, points) => { void draw(tool, points).catch(report); }, onHandleStart: (id, index) => editable ? async (position) => { try { await edit({ type: "moveHandle", id, index, position }); return true; } catch (error) { report(error); return false; } } : null }}
    />
  </div>;
}

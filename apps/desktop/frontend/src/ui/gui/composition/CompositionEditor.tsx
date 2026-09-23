import { useState } from "react";
import * as ContextMenu from "@radix-ui/react-context-menu";
import * as Dialog from "@radix-ui/react-dialog";
import { ArrowDown, ArrowUp, ChevronDown, ChevronRight, Circle } from "lucide-react";
import { navigateToGuiObject } from "../../../workspace/navigation";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import { THEME_METRICS } from "../../../theme";
import type { GuiDocument, GuiDocumentRequest, GuiPixel, GuiLayoutFixture, GuiObjectRef, Transform } from "../../../types";
import { SpatialCanvas } from "./SpatialCanvas";

type Document = Extract<GuiDocument, { type: "fixture" | "layout" }>;
const identityTransform = (): Transform => ({ position: { xMeters: 0, yMeters: 0, zMeters: 0 }, rotation: { xDegrees: 0, yDegrees: 0, zDegrees: 0 }, scale: { x: 1, y: 1, z: 1 } });

type TreeAction =
  | { type: "group" | "fixture"; parent: number | null; origin: GuiDocumentRequest }
  | { type: "rename"; id: number; origin: GuiDocumentRequest };

export function CompositionEditor({ gui }: { gui: Document }) {
  const [selected, setSelected] = useState<number | null>(null);
  const [menuTarget, setMenuTarget] = useState<number | null>(null);
  const [action, setAction] = useState<TreeAction | null>(null);
  const [name, setName] = useState("");
  const [definitionIndex, setDefinitionIndex] = useState("");
  const [error, setError] = useState<string | null>(null);
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const inline = useAppStore((state) => state.guiParents.length > 0);
  const pending = useAppStore((state) => state.guiEditPending);
  const readOnly = useAppStore((state) => state.snapshot?.activeBuffer?.readOnly ?? false);
  const ready = request !== null && revision === request.projectRevision;
  const editable = ready && !pending && !readOnly;
  const target = gui.type === "layout" && menuTarget !== null ? findLayoutItem(gui.document.fixtures, menuTarget) : null;
  const instance = gui.type === "layout" && selected !== null ? findLayoutItem(gui.document.fixtures, selected) : null;
  const reportError = (error: unknown) => { setError(String(error)); };
  const changePixels = async (pixels: GuiPixel[]) => {
    await runGuiEditCommand((request) => commands.applyFixtureGuiEdit(request, { type: "setPixels", pixels }), request);
    setError(null);
  };
  const changeTransform = (transform: Transform) => {
    if (gui.type !== "layout" || instance === null) return;
    const fixtures = updateLayoutItem(gui.document.fixtures, instance.id, (item) => item.kind.type === "fixture" ? { ...item, kind: { ...item.kind, transform } } : item);
    void runGuiEditCommand((request) => commands.applyLayoutGuiEdit(request, { type: "setFixtures", fixtures }), request)
      .then(() => { setError(null); }).catch(reportError);
  };
  const beginAdd = (type: "group" | "fixture") => {
    if (request === null || gui.type !== "layout") return;
    setAction({ type, parent: target?.id ?? null, origin: request });
    setName(type === "group" ? "Group" : "");
    setDefinitionIndex(gui.document.availableFixtures.length === 0 ? "" : "0");
    setError(null);
  };
  const submitAction = async () => {
    if (action === null || gui.type !== "layout" || name.trim() === "") return;
    let definitionToOpen: GuiObjectRef | null = null;
    const id = nextInstanceId(gui.document.fixtures);
    if (action.type === "fixture" && definitionIndex === "") {
      const result = await runGuiEditCommand((request) => commands.applyLayoutGuiEdit(request, {
        type: "addDefinition", name: name.trim(), parent: action.parent
      }), action.origin);
      const added = result.document.type === "layout" ? findLayoutItem(result.document.document.fixtures, id) : null;
      if (added?.kind.type !== "fixture") throw new Error("The created fixture was not returned.");
      definitionToOpen = added.kind.definition;
    } else {
      let fixtures: GuiLayoutFixture[];
      if (action.type === "rename") {
        fixtures = updateLayoutItem(gui.document.fixtures, action.id, (item) => ({ ...item, name: name.trim() }));
      } else if (action.type === "group") {
        fixtures = addChild(gui.document.fixtures, action.parent, { id, name: name.trim(), kind: { type: "group", children: [] } });
      } else {
        if (definitionIndex === "") return;
        const definition = gui.document.availableFixtures[Number(definitionIndex)];
        if (definition === undefined) return;
        fixtures = addChild(gui.document.fixtures, action.parent, { id, name: name.trim(), kind: { type: "fixture", definition, transform: identityTransform() } });
      }
      await runGuiEditCommand((request) => commands.applyLayoutGuiEdit(request, { type: "setFixtures", fixtures }), action.origin);
    }
    setSelected(action.type === "rename" ? action.id : id);
    setAction(null);
    setError(null);
    if (definitionToOpen !== null) await navigateToGuiObject(definitionToOpen);
  };
  return <div className="layout-authoring">
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild disabled={gui.type !== "layout" || !ready}>
        <aside className="layout-hierarchy layout-tree-sidebar" onContextMenuCapture={(event) => {
          const row = event.target instanceof Element ? event.target.closest("[data-layout-item]") : null;
          const id = row === null ? null : Number(row.getAttribute("data-layout-item"));
          setMenuTarget(id);
          if (id !== null) setSelected(id);
        }}>
          {!inline && <h2 className="composition-title">{documentLabel(gui.document.path, gui.document.objectKey)}</h2>}
          {error !== null && action === null && <p role="alert">{error}</p>}
          <fieldset className="composition-controls" disabled={!editable}>
            {gui.type === "fixture"
              ? <Pixels items={gui.document.pixels} selected={selected} onSelect={setSelected} onChange={changePixels} onError={reportError} />
              : <LayoutTree items={gui.document.fixtures} pixels={gui.document.renderPlan.pixels} selected={selected} onSelect={setSelected} />}
            {instance?.kind.type === "fixture" && <Placement value={instance.kind.transform} onChange={changeTransform} />}
          </fieldset>
        </aside>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="menu-content" onCloseAutoFocus={(event) => { if (action !== null) event.preventDefault(); }}>
          {(target === null || target.kind.type === "group") && <>
            <ContextMenu.Item className="menu-item" disabled={!editable} onSelect={() => { beginAdd("group"); }}>Add group</ContextMenu.Item>
            <ContextMenu.Item className="menu-item" disabled={!editable} onSelect={() => { beginAdd("fixture"); }}>Add fixture</ContextMenu.Item>
          </>}
          {target?.kind.type === "fixture" && <ContextMenu.Item className="menu-item" onSelect={() => {
            if (target.kind.type === "fixture") void navigateToGuiObject(target.kind.definition).catch(reportError);
          }}>Edit definition</ContextMenu.Item>}
          {target !== null && <>
            <ContextMenu.Item className="menu-item" disabled={!editable} onSelect={() => {
              if (request === null) return;
              setAction({ type: "rename", id: target.id, origin: request }); setName(target.name); setError(null);
            }}>Rename</ContextMenu.Item>
            <ContextMenu.Item className="menu-item danger" disabled={!editable} onSelect={() => {
              if (gui.type !== "layout") return;
              void runGuiEditCommand((request) => commands.applyLayoutGuiEdit(request, {
                type: "setFixtures", fixtures: updateLayoutItem(gui.document.fixtures, target.id, () => null)
              }), request).then(() => { setSelected(null); setError(null); }).catch(reportError);
            }}>Remove</ContextMenu.Item>
          </>}
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
    <Dialog.Root open={action !== null} onOpenChange={(open) => { if (!open && !pending) setAction(null); }}>
      <Dialog.Portal><Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content composition-add-dialog" aria-describedby={undefined}>
          <Dialog.Title>{action?.type === "rename" ? "Rename" : action?.type === "group" ? "Add group" : "Add fixture"}</Dialog.Title>
          <form className="setup-authoring-form" onSubmit={(event) => { event.preventDefault(); void submitAction().catch(reportError); }}>
            <fieldset disabled={!editable}>
              <label>Name<input required value={name} onFocus={(event) => { event.currentTarget.select(); }} onChange={(event) => { setName(event.target.value); }} /></label>
              {action?.type === "fixture" && gui.type === "layout" && <>
                <label>Definition<select value={definitionIndex} onChange={(event) => {
                  setDefinitionIndex(event.target.value);
                  const definition = event.target.value === "" ? undefined : gui.document.availableFixtures[Number(event.target.value)];
                  if (name === "" && definition !== undefined) setName(definition.objectKey);
                }}><option value="">New definition</option>
                  {gui.document.availableFixtures.map((definition, index) => <option key={definition.id} value={index}>{definition.path} · {definition.objectKey}</option>)}
                </select></label>
              </>}
              {error !== null && <p role="alert">{error}</p>}
              <div className="dialog-actions">
                <button type="submit" disabled={name.trim() === ""}>{action?.type === "rename" ? "Rename" : "Add"}</button>
                <Dialog.Close asChild><button type="button">Cancel</button></Dialog.Close>
              </div>
            </fieldset>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
    <SpatialCanvas plan={gui.document.renderPlan} documentKey={gui.document.path + ":" + gui.document.objectKey} selected={selected} onSelect={setSelected} onMoveStart={(id) => {
      if (!editable) return null;
      const origin = request;
      return async (delta) => {
        try {
          await runGuiEditCommand((request) => gui.type === "fixture"
            ? commands.applyFixtureGuiEdit(request, { type: "movePixel", id, delta })
            : commands.applyLayoutGuiEdit(request, { type: "moveFixture", id, delta }), origin);
          setError(null);
        } catch (error: unknown) { reportError(error); }
      };
    }} />
  </div>;
}

function Pixels({ items, selected, onSelect, onChange, onError }: {
  items: GuiPixel[]; selected: number | null; onSelect: (id: number | null) => void;
  onChange: (pixels: GuiPixel[]) => Promise<void>; onError: (error: unknown) => void;
}) {
  const index = items.findIndex((pixel) => pixel.id === selected);
  const pixel = items[index];
  const nextId = items.reduce((id, pixel) => Math.max(id, pixel.id), 0) + 1;
  const change = (pixels: GuiPixel[]) => { void onChange(pixels).catch(onError); };
  const add = (duplicate: boolean) => {
    const previous = duplicate ? pixel : items[items.length - 1];
    const added: GuiPixel = { id: nextId, position: previous === undefined ? { xMeters: 0, yMeters: 0, zMeters: 0 } : { ...previous.position, xMeters: previous.position.xMeters + previous.diameterMeters }, diameterMeters: previous?.diameterMeters ?? 0.01 };
    const pixels = [...items];
    pixels.splice(duplicate ? index + 1 : items.length, 0, added);
    void onChange(pixels).then(() => { onSelect(added.id); }).catch(onError);
  };
  return <>
    <div className="pixel-actions">
      <button type="button" onClick={() => { add(false); }}>Add pixel</button>
      <button type="button" disabled={pixel === undefined} onClick={() => { add(true); }}>Duplicate</button>
      <button type="button" disabled={pixel === undefined} onClick={() => {
        void onChange(items.filter((item) => item.id !== selected)).then(() => { onSelect(null); }).catch(onError);
      }}>Delete</button>
      <button type="button" aria-label="Move pixel earlier" disabled={index <= 0} onClick={() => { change(reorder(items, index, index - 1)); }}><ArrowUp size={THEME_METRICS.iconSizeSmall} /></button>
      <button type="button" aria-label="Move pixel later" disabled={index < 0 || index === items.length - 1} onClick={() => { change(reorder(items, index, index + 1)); }}><ArrowDown size={THEME_METRICS.iconSizeSmall} /></button>
    </div>
    <div className="composition-tree pixel-list" aria-label="Pixels in output order">
      {items.length === 0 && <p className="composition-tree-empty">Add a pixel to get started.</p>}
      {items.map((pixel, index) => <button type="button" className="pixel-row" aria-pressed={pixel.id === selected} key={pixel.id} onClick={() => { onSelect(pixel.id); }}>
        <span>Pixel {index + 1}</span><span className="composition-tree-meta">{pixel.position.xMeters}, {pixel.position.yMeters}, {pixel.position.zMeters} m · Ø {pixel.diameterMeters} m</span>
      </button>)}
    </div>
    {pixel !== undefined && <div className="pixel-fields">
      {(["xMeters", "yMeters", "zMeters"] as const).map((axis) => <CoordinateField key={`${axis}-${pixel.id}`} label={axis.charAt(0).toUpperCase() + " (m)"} value={pixel.position[axis]} min={-2000} max={2000} onChange={(value) => { change(items.map((item) => item.id === pixel.id ? { ...item, position: { ...item.position, [axis]: value } } : item)); }} />)}
      <CoordinateField key={`diameter-${pixel.id}`} label="Diameter (m)" value={pixel.diameterMeters} min={0.000001} max={100} onChange={(diameterMeters) => { change(items.map((item) => item.id === pixel.id ? { ...item, diameterMeters } : item)); }} />
    </div>}
  </>;
}

function CoordinateField({ label, value, min, max, onChange }: { label: string; value: number; min?: number; max?: number; onChange: (value: number) => void }) {
  return <label>{label}<input key={value} type="number" required step="any" min={min} max={max} defaultValue={value} onKeyDown={(event) => {
    if (event.key === "Enter") event.currentTarget.blur();
    if (event.key === "Escape") { event.currentTarget.value = String(value); event.currentTarget.blur(); }
  }} onBlur={(event) => {
    if (!event.currentTarget.reportValidity()) { event.currentTarget.value = String(value); return; }
    const next = event.currentTarget.valueAsNumber;
    event.currentTarget.value = String(value);
    if (next !== value) onChange(next);
  }} /></label>;
}

function Placement({ value, onChange }: { value: Transform; onChange: (value: Transform) => void }) {
  return <div className="placement-fields">
    {(["xMeters", "yMeters", "zMeters"] as const).map((axis) => <CoordinateField key={axis} label={axis.charAt(0).toUpperCase() + " (m)"} value={value.position[axis]} min={-2000} max={2000} onChange={(next) => { onChange({ ...value, position: { ...value.position, [axis]: next } }); }} />)}
    {(["xDegrees", "yDegrees", "zDegrees"] as const).map((axis) => <CoordinateField key={axis} label={"Rotate " + axis.charAt(0).toUpperCase() + " (°)"} value={value.rotation[axis]} onChange={(next) => { onChange({ ...value, rotation: { ...value.rotation, [axis]: next } }); }} />)}
    {(["x", "y", "z"] as const).map((axis) => <CoordinateField key={axis} label={"Scale " + axis.toUpperCase()} value={value.scale[axis]} onChange={(next) => { onChange({ ...value, scale: { ...value.scale, [axis]: next } }); }} />)}
  </div>;
}

function LayoutTree({ items, pixels, selected, onSelect }: { items: GuiLayoutFixture[]; pixels: { owner: number }[]; selected: number | null; onSelect: (id: number) => void }) {
  return <div className="composition-tree">
    {items.length === 0 && <p className="composition-tree-empty">Right-click to add a group or fixture.</p>}
    {items.map((item) => <LayoutTreeItem key={item.id} item={item} pixels={pixels} selected={selected} onSelect={onSelect} />)}
  </div>;
}

function LayoutTreeItem({ item, pixels, selected, onSelect }: { item: GuiLayoutFixture; pixels: { owner: number }[]; selected: number | null; onSelect: (id: number) => void }) {
  const isGroup = item.kind.type === "group";
  const [open, setOpen] = useState(isGroup);
  const pixelCount = item.kind.type === "group" ? item.kind.children.reduce((total, child) => total + layoutPixelCount(child, pixels), 0) : pixels.filter((pixel) => pixel.owner === item.id).length;
  const metadata = item.kind.type === "group"
    ? `group · ${layoutFixtureCount(item.kind.children)} fixtures · ${pixelCount} pixels`
    : `${item.kind.definition.objectKey} · ${pixelCount} pixels · ${formatPosition(item.kind.transform.position)}`;
  return <details className="composition-tree-item" open={isGroup ? open : undefined} onToggle={(event) => { if (isGroup) setOpen(event.currentTarget.open); }} data-layout-item={item.id}>
    <summary className={selected === item.id ? "selected" : ""} onClick={() => { onSelect(item.id); }}><span className="composition-tree-icon" aria-hidden="true">{isGroup ? (open ? <ChevronDown size={THEME_METRICS.iconSizeExtraSmall} /> : <ChevronRight size={THEME_METRICS.iconSizeExtraSmall} />) : <Circle size={THEME_METRICS.iconSizeExtraSmall} />}</span><span className="composition-tree-name">{item.name}</span><span className="composition-tree-meta">{metadata}</span></summary>
    {item.kind.type === "group" && <div className="composition-tree-children">{item.kind.children.map((child) => <LayoutTreeItem key={child.id} item={child} pixels={pixels} selected={selected} onSelect={onSelect} />)}</div>}
  </details>;
}

function addChild(items: GuiLayoutFixture[], parentId: number | null, child: GuiLayoutFixture): GuiLayoutFixture[] {
  if (parentId === null) return [...items, child];
  return items.map((item) => item.id === parentId && item.kind.type === "group"
    ? { ...item, kind: { type: "group", children: [...item.kind.children, child] } }
    : item.kind.type === "group" ? { ...item, kind: { type: "group", children: addChild(item.kind.children, parentId, child) } } : item);
}

function layoutFixtureCount(items: GuiLayoutFixture[]): number {
  return items.reduce((count, item) => count + (item.kind.type === "group" ? layoutFixtureCount(item.kind.children) : 1), 0);
}

function layoutPixelCount(item: GuiLayoutFixture, pixels: { owner: number }[]): number {
  return item.kind.type === "group" ? item.kind.children.reduce((total, child) => total + layoutPixelCount(child, pixels), 0) : pixels.filter((pixel) => pixel.owner === item.id).length;
}

function formatPosition(position: Transform["position"]): string {
  return `position ${position.xMeters}, ${position.yMeters}, ${position.zMeters}`;
}

function reorder<T>(items: T[], from: number, to: number): T[] {
  if (!Number.isInteger(to) || to < 0 || to >= items.length) return items;
  const result = [...items];
  const [item] = result.splice(from, 1);
  if (item !== undefined) result.splice(to, 0, item);
  return result;
}

function nextInstanceId(fixtures: GuiLayoutFixture[]): number {
  return Math.max(0, ...fixtures.map((fixture) => Math.max(fixture.id, fixture.kind.type === "group" ? nextInstanceId(fixture.kind.children) - 1 : 0))) + 1;
}

function documentLabel(path: string, objectKey: string): string {
  const name = path.split(/[\\/]/).pop();
  return name === undefined || name === "" ? objectKey : name;
}
function findLayoutItem(items: GuiLayoutFixture[], id: number): GuiLayoutFixture | null {
  for (const item of items) {
    if (item.id === id) return item;
    if (item.kind.type === "group") {
      const child = findLayoutItem(item.kind.children, id);
      if (child !== null) return child;
    }
  }
  return null;
}

function updateLayoutItem(items: GuiLayoutFixture[], id: number, update: (item: GuiLayoutFixture) => GuiLayoutFixture | null): GuiLayoutFixture[] {
  return items.flatMap((item) => {
    if (item.id === id) {
      const changed = update(item);
      return changed === null ? [] : [changed];
    }
    return [item.kind.type === "group"
      ? { ...item, kind: { type: "group" as const, children: updateLayoutItem(item.kind.children, id, update) } }
      : item];
  });
}

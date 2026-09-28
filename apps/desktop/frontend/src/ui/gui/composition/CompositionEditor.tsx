import { SpatialSelectionControls } from "./SpatialSelectionControls";
import { layoutItems, selectedItems, selectionClick, moveLayout, type SpatialMove } from "./spatialSelection";
import { LayoutTree } from "./LayoutTree";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { OwnershipActions } from "../OwnershipActions";
import { useState } from "react";
import * as ContextMenu from "@radix-ui/react-context-menu";
import * as Dialog from "@radix-ui/react-dialog";
import { navigateToGuiObject } from "../../../workspace/navigation";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import type { FixtureStorage, GuiDocument, GuiDocumentRequest, GuiLayoutFixture, GuiObjectRef, Point3Meters, Transform } from "../../../types";
import { FixtureEditor } from "./FixtureEditor";
import { Placement } from "./FixtureFields";
import { SpatialCanvas } from "./SpatialCanvas";
import { LayoutAddMenu } from "./LayoutAddMenu";

type Document = Extract<GuiDocument, { type: "fixture" | "layout" }>;
const identityTransform = (position: Point3Meters = { xMeters: 0, yMeters: 0, zMeters: 0 }): Transform => ({ position, rotation: { xDegrees: 0, yDegrees: 0, zDegrees: 0 }, scale: { x: 1, y: 1, z: 1 } });

type TreeAction =
  | { type: "group" | "fixture"; parent: number | null; origin: GuiDocumentRequest; position?: Point3Meters }
  | { type: "rename"; id: number; origin: GuiDocumentRequest };

export function CompositionEditor({ gui }: { gui: Document }) {
  return gui.type === "fixture" ? <FixtureEditor key={guiObjectKey(gui.document.sourceRef)} document={gui.document} /> : <LayoutEditor key={guiObjectKey(gui.document.sourceRef)} gui={gui} />;
}

function LayoutEditor({ gui }: { gui: Extract<Document, { type: "layout" }> }) {
  const [selection, setSelection] = useState<number[]>([]);
  const selected = selection.length === 1 ? selection[0] ?? null : null;
  const setSelected = (id: number | null) => { setSelection(id === null ? [] : [id]); };
  const items = layoutItems(gui.document.fixtures, gui.document.renderPlan);
  const chosen = selectedItems(items, selection);
  const [menuTarget, setMenuTarget] = useState<number | null>(null);
  const [action, setAction] = useState<TreeAction | null>(null);
  const [name, setName] = useState("");
  const [storage, setStorage] = useState<FixtureStorage>("inline");
  const [error, setError] = useState<string | null>(null);
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const inline = useAppStore((state) => state.guiParents.length > 0);
  const pending = useAppStore((state) => state.guiEditPending);
  const readOnly = useAppStore((state) => state.snapshot?.activeBuffer?.readOnly ?? false);
  const ready = request !== null && revision === request.projectRevision;
  const editable = ready && !pending && !readOnly;
  const target = menuTarget !== null ? findLayoutItem(gui.document.fixtures, menuTarget) : null;
  const instance = selected !== null ? findLayoutItem(gui.document.fixtures, selected) : null;
  const fixtureSource = (fixture: GuiLayoutFixture): GuiObjectRef => {
    if (fixture.kind.type !== "fixture") throw new Error("Select a fixture.");
    if (fixture.kind.definition.type === "reference") return fixture.kind.definition.source;
    return { ...gui.document.sourceRef, kind: "fixture", ownedPath: [...gui.document.sourceRef.ownedPath, { type: "fixture", id: fixture.id }] };
  };
  const reportError = (error: unknown) => { setError(String(error)); };
  const changeTransform = (transform: Transform) => {
    if (instance === null) return;
    const fixtures = updateLayoutItem(gui.document.fixtures, instance.id, (item) => item.kind.type === "fixture" ? { ...item, kind: { ...item.kind, transform } } : item);
    void runGuiEditCommand((request) => commands.applyLayoutGuiEdit(request, { type: "setFixtures", fixtures }), request)
      .then(() => { setError(null); }).catch(reportError);
  };
  const beginAdd = (type: "group" | "fixture", parent = target?.id ?? null, position?: Point3Meters) => {
    if (request === null) return;
    setAction(position === undefined
      ? { type, parent, origin: request }
      : { type, parent, origin: request, position });
    setName(type === "group" ? "Group" : "");
    setStorage("inline");
    setError(null);
  };
  const submitAction = async () => {
    if (action === null || name.trim() === "") return;
    let definitionToOpen: GuiObjectRef | null = null;
    const id = nextInstanceId(gui.document.fixtures);
    if (action.type === "fixture") {
      const result = await runGuiEditCommand((request) => commands.applyLayoutGuiEdit(request, {
        type: "addDefinition", name: name.trim(), storage, parent: action.parent, transform: identityTransform(action.position)
      }), action.origin);
      const added = result.document.type === "layout" ? findLayoutItem(result.document.document.fixtures, id) : null;
      if (added?.kind.type !== "fixture") throw new Error("The created fixture was not returned.");
      definitionToOpen = fixtureSource(added);
    } else {
      let fixtures: GuiLayoutFixture[];
      if (action.type === "rename") {
        fixtures = updateLayoutItem(gui.document.fixtures, action.id, (item) => ({ ...item, name: name.trim() }));
      } else {
        fixtures = addChild(gui.document.fixtures, action.parent, { id, name: name.trim(), kind: { type: "group", children: [] } });
      }
      await runGuiEditCommand((request) => commands.applyLayoutGuiEdit(request, { type: "setFixtures", fixtures }), action.origin);
    }
    setSelected(action.type === "rename" ? action.id : id);
    setAction(null);
    setError(null);
    if (definitionToOpen !== null) await navigateToGuiObject(definitionToOpen);
  };
  const addExistingFixtureAt = async (definition: GuiObjectRef, parent: number | null, position?: Point3Meters) => {
    if (request === null) return;
    const id = nextInstanceId(gui.document.fixtures);
    const fixtures = addChild(gui.document.fixtures, parent, {
      id,
      name: definition.objectKey,
      kind: { type: "fixture", definition: { type: "reference", source: definition }, transform: identityTransform(position) }
    });
    await runGuiEditCommand((currentRequest) => commands.applyLayoutGuiEdit(currentRequest, { type: "setFixtures", fixtures }), request);
    setSelected(id);
    setError(null);
  };
  const moveSelection = async (moves: SpatialMove[]) => {
    await runGuiEditCommand((current) => commands.applyLayoutGuiEdit(current, { type: "setFixtures", fixtures: moveLayout(gui.document.fixtures, moves) }), request);
    setError(null);
  };
  const repeat = (offsets: Point3Meters[]) => {
    const originalIds = new Set(items.map((item) => item.id));
    void runGuiEditCommand((current) => commands.applyLayoutGuiEdit(current, { type: "repeatFixtures", ids: chosen.map((item) => item.id), offsets }), request)
      .then((result) => {
        if (result.document.type === "layout") {
          const copies = layoutItems(result.document.document.fixtures, result.document.document.renderPlan);
          setSelection(selectedItems(copies, copies.filter((item) => !originalIds.has(item.id)).map((item) => item.id)).map((item) => item.id));
        }
        setError(null);
      }).catch(reportError);
  };
  const duplicate = () => { repeat([{ xMeters: 0, yMeters: 0, zMeters: 0 }]); };
  const remove = () => {
    let fixtures = gui.document.fixtures;
    for (const item of chosen) fixtures = updateLayoutItem(fixtures, item.id, () => null);
    void runGuiEditCommand((current) => commands.applyLayoutGuiEdit(current, { type: "setFixtures", fixtures }), request).then(() => { setSelection([]); setError(null); }).catch(reportError);
  };
  const layoutMenu = {
    availableFixtures: gui.document.availableFixtures,
    enabled: editable,
    onAddFixture: (definition: GuiObjectRef, position: Point3Meters) => { void addExistingFixtureAt(definition, null, position).catch(reportError); },
    onCreateFixture: (position: Point3Meters) => { beginAdd("fixture", null, position); },
    onAddGroup: () => { beginAdd("group", null); }
  };
  return <div className="layout-authoring">
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild disabled={!ready}>
        <aside className="layout-hierarchy layout-tree-sidebar" onContextMenuCapture={(event) => {
          const row = event.target instanceof Element ? event.target.closest("[data-layout-item]") : null;
          const id = row === null ? null : Number(row.getAttribute("data-layout-item"));
          setMenuTarget(id);
          if (id !== null && !selection.includes(id)) setSelected(id);
        }}>
          {!inline && <h2 className="composition-title">{gui.document.sourceRef.ownedPath.length > 0 ? "Layout" : documentLabel(gui.document.path, gui.document.objectKey)}</h2>}
          {error !== null && action === null && <p role="alert">{error}</p>}
          <fieldset className="composition-controls" disabled={!editable}>
            <LayoutTree items={gui.document.fixtures} pixels={gui.document.renderPlan.pixels} selected={selection} onSelect={(id, additive = false) => { setSelection(selectionClick(selection, id, additive)); }} enabled={editable} request={request} onError={reportError} />
            <SpatialSelectionControls items={chosen} disabled={!editable} onMove={(moves) => { void moveSelection(moves).catch(reportError); }} onRepeat={repeat} onDuplicate={duplicate} onDelete={remove} />
            {instance?.kind.type === "fixture" && <><Placement value={instance.kind.transform} onChange={changeTransform} /><OwnershipActions sources={gui.document.availableFixtures} key={instance.id} source={fixtureSource(instance)} slot={{ type: "fixture", id: instance.id }} label={instance.name} /></>}
          </fieldset>
        </aside>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="menu-content" onCloseAutoFocus={(event) => { if (action !== null) event.preventDefault(); }}>
          {(target === null || target.kind.type === "group") && <LayoutAddMenu
            availableFixtures={gui.document.availableFixtures}
            enabled={editable}
            onAddFixture={(definition) => { void addExistingFixtureAt(definition, target?.id ?? null).catch(reportError); }}
            onCreateFixture={() => { beginAdd("fixture"); }}
            onAddGroup={() => { beginAdd("group"); }}
          />}
          {target?.kind.type === "fixture" && <ContextMenu.Item className="menu-item" onSelect={() => {
            if (target.kind.type === "fixture") void navigateToGuiObject(fixtureSource(target)).catch(reportError);
          }}>Edit fixture</ContextMenu.Item>}
          {target !== null && <>
            <ContextMenu.Item className="menu-item" disabled={!editable} onSelect={duplicate}>Duplicate selection</ContextMenu.Item>
            <ContextMenu.Item className="menu-item" disabled={!editable} onSelect={() => {
              if (request === null) return;
              setAction({ type: "rename", id: target.id, origin: request }); setName(target.name); setError(null);
            }}>Rename</ContextMenu.Item>
            <ContextMenu.Item className="menu-item danger" disabled={!editable} onSelect={remove}>Delete selection</ContextMenu.Item>
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
              {action?.type === "fixture" && <details className="composition-add-advanced">
                <summary>Advanced settings</summary>
                <label>Save fixture in<select value={storage} onChange={(event) => { if (event.target.value === "inline" || event.target.value === "sameFile" || event.target.value === "newFile") setStorage(event.target.value); }}>
                  <option value="inline">Inside this fixture (default)</option>
                  <option value="sameFile">Reusable fixture in this file</option>
                  <option value="newFile">Reusable fixture in a new file</option>
                </select></label>
                <p>{storage === "inline" ? "Stored inside this fixture. Duplicating the fixture makes an independent copy; removing it removes its geometry." : storage === "sameFile" ? "Creates a reusable source in this file. Removing this placement keeps the source available." : "Creates a file in the fixtures folder, named after this fixture. The file stays available when you remove its placement."}</p>
              </details>}
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
    <SpatialCanvas reference={gui.document.sourceRef} plan={gui.document.renderPlan} documentKey={guiObjectKey(gui.document.sourceRef)} selection={selection} items={items} onSelect={setSelection} layoutMenu={layoutMenu} onDelete={remove} onDuplicate={duplicate} onMoveStart={(ids, anchor) => {
      if (!editable) return null;
      const chosen = selectedItems(items, ids);
      const origin = items.find((item) => item.id === anchor)?.origin;
      if (origin === undefined) return null;
      return { origin, owners: chosen.flatMap((item) => item.owners), commit: async (delta) => {
        try { await moveSelection(chosen.map((item) => ({ id: item.id, delta }))); return true; }
        catch (error: unknown) { reportError(error); return false; }
      } };
    }} />
  </div>;
}

function addChild(items: GuiLayoutFixture[], parentId: number | null, child: GuiLayoutFixture): GuiLayoutFixture[] {
  if (parentId === null) return [...items, child];
  return items.map((item) => item.id === parentId && item.kind.type === "group"
    ? { ...item, kind: { type: "group", children: [...item.kind.children, child] } }
    : item.kind.type === "group" ? { ...item, kind: { type: "group", children: addChild(item.kind.children, parentId, child) } } : item);
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

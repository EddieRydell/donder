import { SpatialSelectionControls } from "./SpatialSelectionControls";
import { layoutItems, selectedItems, selectionClick, moveLayout, type SpatialMove } from "./spatialSelection";
import { LayoutTree } from "./LayoutTree";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { OwnershipActions } from "../OwnershipActions";
import { DEFAULT_OBJECT_STORAGE, NEW_OBJECT_STORAGE, ObjectStorageField } from "../objectStorage";
import { useState, type ReactNode } from "react";
import { navigateToGuiObject } from "../../../workspace/navigation";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import type { NewObjectStorage, GuiDocument, GuiDocumentRequest, GuiLayoutFixture, GuiObjectRef, Point3Meters, Transform } from "../../../types";
import { FixtureEditor } from "./FixtureEditor";
import { Placement } from "./FixtureFields";
import { SpatialCanvas } from "./SpatialCanvas";
import { layoutAddEntries } from "./LayoutAddMenu";
import { Copy, FolderInput, FolderMinus, Pencil, Plus, Trash2 } from "lucide-react";
import { Button, ContextMenuArea, Dialog, Field, IconButton, MenuButton, Problem, Section, TextField, type MenuEntry } from "../../kit";
import { OBJECT, useFieldProblem } from "../problems";
import { outputTestEntries, useOutputTesting } from "../outputTest";
import { descendants, layoutIndex, membersOf, membershipCount, nextLayoutId, withMember } from "./layoutGraph";

type Document = Extract<GuiDocument, { type: "fixture" | "layout" }>;
const identityTransform = (position: Point3Meters = { xMeters: 0, yMeters: 0, zMeters: 0 }): Transform => ({ position, rotation: { xDegrees: 0, yDegrees: 0, zDegrees: 0 }, scale: { x: 1, y: 1, z: 1 } });

/** A fixture being created: where it goes, waiting for its name. */
type NewFixture = { parent: number | null; origin: GuiDocumentRequest; position?: Point3Meters };


export function CompositionEditor({ gui }: { gui: Document }) {
  return gui.type === "fixture" ? <FixtureEditor key={guiObjectKey(gui.document.sourceRef)} document={gui.document} /> : <LayoutEditor key={guiObjectKey(gui.document.sourceRef)} gui={gui} />;
}

function LayoutEditor({ gui }: { gui: Extract<Document, { type: "layout" }> }) {
  const [selection, setSelection] = useState<number[]>([]);
  const selected = selection.length === 1 ? selection[0] ?? null : null;
  const setSelected = (id: number | null) => { setSelection(id === null ? [] : [id]); };
  const items = layoutItems(gui.document.fixtures, gui.document.renderPlan);
  const chosen = selectedItems(items, selection);
  /** The right-clicked row: an item and the group listing it (`null` for the root). */
  const [menuTarget, setMenuTarget] = useState<{ id: number; parent: number | null } | null>(null);
  const [creating, setCreating] = useState<NewFixture | null>(null);
  const [name, setName] = useState("");
  const [storage, setStorage] = useState<NewObjectStorage>(DEFAULT_OBJECT_STORAGE);
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const inline = useAppStore((state) => state.guiParents.length > 0);
  const pending = useAppStore((state) => state.guiEditPending);
  const testing = useOutputTesting();
  const ready = request !== null && revision === request.projectRevision;
  const editable = ready && !pending;
  const { fixtures: layoutFixtures, root } = gui.document;
  const layout = layoutIndex(layoutFixtures);
  const target = menuTarget !== null ? layout.get(menuTarget.id) ?? null : null;
  const targetParent = menuTarget?.parent ?? null;
  const instance = selected !== null ? layout.get(selected) ?? null : null;
  const setLayout = (fixtures: GuiLayoutFixture[], origin: GuiDocumentRequest | null, nextRoot = root) =>
    runGuiEditCommand((current) => commands.applyGuiEdit(current, { type: "layout", edit: { type: "setFixtures", fixtures, root: nextRoot } }), origin);
  const updateItem = (id: number, update: (item: GuiLayoutFixture) => GuiLayoutFixture) => layoutFixtures.map((item) => item.id === id ? update(item) : item);
  /** Groups that can list the target without containing themselves or listing it twice. */
  const destinations = target === null ? [] : layoutFixtures.filter((group) =>
    group.kind.type === "group" && !descendants(layout, target.id).includes(group.id) && !membersOf(group).includes(target.id));
  const fixtureDeclaration = (fixture: GuiLayoutFixture): GuiObjectRef => {
    if (fixture.kind.type !== "fixture") throw new Error("Select a fixture.");
    if (fixture.kind.definition.type === "reference") return fixture.kind.definition.object;
    return { ...gui.document.sourceRef, kind: "fixture", ownedPath: [...gui.document.sourceRef.ownedPath, { type: "fixture", id: fixture.id }] };
  };
  const changeItem = (id: number, update: (item: GuiLayoutFixture) => GuiLayoutFixture) => {
    void setLayout(updateItem(id, update), request);
  };
  const changeTransform = (transform: Transform) => {
    if (instance === null) return;
    changeItem(instance.id, (item) => item.kind.type === "fixture" ? { ...item, kind: { ...item.kind, transform } } : item);
  };
  const addGroup = (parent: number | null) => {
    const id = nextLayoutId(layoutFixtures);
    const added = withMember(layoutFixtures, root, parent, { id, name: "Group", description: null, kind: { type: "group", members: [] } });
    void setLayout(added.fixtures, request, added.root).then((result) => { if (result !== null) setSelected(id); });
  };
  const beginCreate = (parent: number | null, position?: Point3Meters) => {
    if (request === null) return;
    setCreating(position === undefined ? { parent, origin: request } : { parent, origin: request, position });
    setName("");
    setStorage(DEFAULT_OBJECT_STORAGE);
  };
  const submitCreate = async () => {
    if (creating === null || name.trim() === "") return;
    const id = nextLayoutId(layoutFixtures);
    const result = await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "layout", edit: {
      type: "addDefinition", name: name.trim(), storage, parent: creating.parent, transform: identityTransform(creating.position)
    } }), creating.origin);
    if (result === null) return;
    const layout = result.change.type === "document" && result.change.document.type === "layout" ? result.change.document.document : null;
    const added = layout?.fixtures.find((item) => item.id === id) ?? null;
    if (added?.kind.type !== "fixture") throw new Error("The created fixture was not returned.");
    setSelected(id);
    setCreating(null);
    await navigateToGuiObject(fixtureDeclaration(added));
  };
  const addExistingFixtureAt = async (definition: GuiObjectRef, parent: number | null, position?: Point3Meters) => {
    if (request === null) return;
    const id = nextLayoutId(layoutFixtures);
    const added = withMember(layoutFixtures, root, parent, {
      id,
      name: definition.objectKey,
      description: null,
      kind: { type: "fixture", definition: { type: "reference", object: definition }, transform: identityTransform(position) }
    });
    const result = await setLayout(added.fixtures, request, added.root);
    if (result !== null) setSelected(id);
  };
  /** Whether the move was applied. */
  const moveSelection = async (moves: SpatialMove[]) => await setLayout(moveLayout(layoutFixtures, moves), request) !== null;
  const repeat = (offsets: Point3Meters[]) => {
    const originalIds = new Set(items.map((item) => item.id));
    void runGuiEditCommand((current) => commands.applyGuiEdit(current, { type: "layout", edit: { type: "repeatFixtures", ids: chosen.map((item) => item.id), offsets } }), request)
      .then((result) => {
        if (result?.change.type === "document" && result.change.document.type === "layout") {
          const copies = layoutItems(result.change.document.document.fixtures, result.change.document.document.renderPlan);
          setSelection(selectedItems(copies, copies.filter((item) => !originalIds.has(item.id)).map((item) => item.id)).map((item) => item.id));
        }
      });
  };
  const duplicate = () => { repeat([{ xMeters: 0, yMeters: 0, zMeters: 0 }]); };
  /** Delete the selection everywhere; members of a deleted group stay in the layout. */
  const remove = () => {
    void runGuiEditCommand((current) => commands.applyGuiEdit(current, { type: "layout", edit: { type: "removeItems", ids: chosen.map((item) => item.id) } }), request)
      .then((result) => { if (result !== null) setSelection([]); });
  };
  /** Opens the fixture's own editor; a group has none. */
  const openFixture = (id: number) => {
    const item = layout.get(id);
    if (item?.kind.type === "fixture") void navigateToGuiObject(fixtureDeclaration(item));
  };
  /** What can be added under `parent` (`null` for the top level). */
  const addEntries = (parent: number | null) => layoutAddEntries({
    availableFixtures: gui.document.availableFixtures,
    enabled: editable,
    onAddFixture: (definition) => { void addExistingFixtureAt(definition, parent); },
    onCreateFixture: () => { beginCreate(parent); },
    onAddGroup: () => { addGroup(parent); }
  });
  const membershipEdit = (edit: { type: "addMember"; id: number; to: number | null; before: null } | { type: "removeMember"; id: number; from: number | null }) => {
    void runGuiEditCommand((current) => commands.applyGuiEdit(current, { type: "layout", edit: edit }), request);
  };
  const layoutMenu = {
    availableFixtures: gui.document.availableFixtures,
    enabled: editable,
    onAddFixture: (definition: GuiObjectRef, position: Point3Meters) => { void addExistingFixtureAt(definition, null, position); },
    onCreateFixture: (position: Point3Meters) => { beginCreate(null, position); },
    onAddGroup: () => { addGroup(null); }
  };
  const menuEntries: MenuEntry[] = [
    ...(target === null || target.kind.type === "group" ? addEntries(target?.id ?? null) : []),
    ...(target?.kind.type === "fixture" ? [{ type: "action", label: "Edit fixture", icon: Pencil, onSelect: () => { openFixture(target.id); } }] satisfies MenuEntry[] : []),
    ...(target === null ? [] : outputTestEntries({ type: "layoutItem", item: target.id }, !ready || pending, testing)),
    ...(target === null ? [] : [
      { type: "submenu", label: "Add to group", icon: FolderInput, disabled: !editable || destinations.length === 0, entries: destinations.map((group): MenuEntry => ({ type: "action", label: group.name, disabled: !editable, onSelect: () => { membershipEdit({ type: "addMember", id: target.id, to: group.id, before: null }); } })) },
      ...(targetParent !== null || membershipCount(layoutFixtures, root, target.id) > 1 ? [{ type: "action", label: targetParent === null ? "Remove from top level" : `Remove from ${layout.get(targetParent)?.name ?? "group"}`, icon: FolderMinus, disabled: !editable, onSelect: () => { membershipEdit({ type: "removeMember", id: target.id, from: targetParent }); } }] satisfies MenuEntry[] : []),
      { type: "separator" },
      { type: "action", label: "Duplicate", icon: Copy, shortcut: "edit.duplicate", disabled: !editable, onSelect: duplicate },
      { type: "action", label: "Delete", icon: Trash2, shortcut: "edit.delete", danger: true, disabled: !editable, onSelect: remove }
    ] satisfies MenuEntry[])
  ];
  return <div className="layout-authoring">
    <ContextMenuArea entries={menuEntries} disabled={!ready}>
      <aside className="layout-hierarchy layout-tree-sidebar" onContextMenuCapture={(event) => {
        const row = event.target instanceof Element ? event.target.closest("[data-layout-item]") : null;
        const id = row === null ? null : Number(row.getAttribute("data-layout-item"));
        const parent = row?.getAttribute("data-layout-parent") ?? null;
        setMenuTarget(id === null ? null : { id, parent: parent === null ? null : Number(parent) });
        if (id !== null && !selection.includes(id)) setSelected(id);
      }}>
        <div className="composition-header">
          {!inline && <h2 className="composition-title">{gui.document.sourceRef.ownedPath.length > 0 ? "Layout" : gui.document.objectKey}</h2>}
          {root.length > 0 && <MenuButton entries={addEntries(null)} align="end"><IconButton icon={Plus} label="Add" disabled={!editable} /></MenuButton>}
        </div>
        <fieldset className="composition-controls" disabled={!editable}>
          <LayoutTree fixtures={layoutFixtures} root={root} pixels={gui.document.renderPlan.pixels} selected={selection} onSelect={(id, additive = false) => { setSelection(selectionClick(selection, id, additive)); }} onOpen={openFixture} enabled={editable} request={request} addEntries={addEntries(null)} />
          <SpatialSelectionControls items={chosen} disabled={!editable} onMove={(moves) => { void moveSelection(moves); }} onRepeat={repeat} onDuplicate={duplicate} onDelete={remove} />
          {instance !== null && <ItemFields key={instance.id} instance={instance} editable={editable} changeItem={changeItem} changeTransform={changeTransform}
            ownership={instance.kind.type === "fixture" ? <OwnershipActions declarations={gui.document.availableFixtures} key={instance.id} source={fixtureDeclaration(instance)} fixture={instance.id} label={instance.name} /> : null} />}
        </fieldset>
      </aside>
    </ContextMenuArea>
    <Dialog
      open={creating !== null}
      onOpenChange={(open) => { if (!open && !pending) setCreating(null); }}
      title="New fixture"
      actions={<>
        <Button onClick={() => { setCreating(null); }}>Cancel</Button>
        <Button variant="primary" disabled={!editable || name.trim() === ""} onClick={() => { void submitCreate(); }}>Add</Button>
      </>}
    >
      <NewFixtureName name={name} disabled={!editable} onChange={setName} />
      <ObjectStorageField value={storage} choices={NEW_OBJECT_STORAGE} disabled={!editable} onChange={setStorage} />
    </Dialog>
    <SpatialCanvas reference={gui.document.sourceRef} plan={gui.document.renderPlan} documentKey={guiObjectKey(gui.document.sourceRef)} selection={selection} items={items} onSelect={setSelection} onOpen={openFixture} layoutMenu={layoutMenu} onDelete={remove} onDuplicate={duplicate} onMoveStart={(ids, anchor) => {
      if (!editable) return null;
      const chosen = selectedItems(items, ids);
      const origin = items.find((item) => item.id === anchor)?.origin;
      if (origin === undefined) return null;
      return { origin, owners: chosen.flatMap((item) => item.owners), commit: (delta) => moveSelection(chosen.map((item) => ({ id: item.id, delta }))) };
    }} />
  </div>;
}

/** A new fixture's name, which its new definition takes; a refused name marks it. */
function NewFixtureName({ name, disabled, onChange }: { name: string; disabled: boolean; onChange: (name: string) => void }) {
  const problem = useFieldProblem(OBJECT, { type: "definition" });
  return <Field label="Name" error={problem}>
    <TextField name="Fixture name" live autoFocus disabled={disabled} value={name} invalid={problem !== null} onCommit={onChange} />
  </Field>;
}

/** The selected item's name, description and, for a fixture, placement and storage. */
function ItemFields({ instance, editable, changeItem, changeTransform, ownership }: {
  instance: GuiLayoutFixture;
  editable: boolean;
  changeItem: (id: number, update: (item: GuiLayoutFixture) => GuiLayoutFixture) => void;
  changeTransform: (transform: Transform) => void;
  ownership: ReactNode;
}) {
  const member = { type: "layoutItem", id: instance.id } as const;
  const nameProblem = useFieldProblem(member, { type: "name" });
  const descriptionProblem = useFieldProblem(member, { type: "description" });
  const placementProblem = useFieldProblem(member, { type: "placement" });
  return <div className="fixture-shape-fields">
    <Field label="Name" error={nameProblem}>
      <TextField name="Name" value={instance.name} invalid={nameProblem !== null} onCommit={(value) => { const name = value.trim(); if (name !== "" && name !== instance.name) changeItem(instance.id, (item) => ({ ...item, name })); }} />
    </Field>
    <Section title="Description" defaultOpen={instance.description !== null}>
      <Problem error={descriptionProblem}>
        <TextField name="Description" multiline value={instance.description ?? ""} invalid={descriptionProblem !== null} onCommit={(value) => { const description = value.trim() === "" ? null : value.trim(); if (description !== instance.description) changeItem(instance.id, (item) => ({ ...item, description })); }} />
      </Problem>
    </Section>
    {instance.kind.type === "fixture" && <><Placement value={instance.kind.transform} disabled={!editable} problem={placementProblem} onChange={changeTransform} />{ownership}</>}
  </div>;
}

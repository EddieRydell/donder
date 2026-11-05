import { guiObjectKey, sameGuiObject } from "../../workspace/guiIdentity";
import { ExternalLink, Link2, MoreHorizontal, Package } from "lucide-react";
import { useState } from "react";
import { commands } from "../../api";
import { runGuiEditCommand, useAppStore } from "../../store";
import type { GuiObjectRef, GuiOwnershipEdit, OwnershipMember, ReusableStorage } from "../../types";
import { navigateToGuiObject } from "../../workspace/navigation";
import { Badge, Button, Dialog, Field, IconButton, MenuButton, Select, TextField, type MenuEntry } from "../kit";

const STORAGE_OPTIONS = [
  { value: "sameFile", label: "This file" },
  { value: "newFile", label: "New file" }
] as const;

/** Whether `source` is stored inside its parent or links a reusable declaration. */
export function OwnershipBadge({ source }: { source: GuiObjectRef }) {
  return source.ownedPath.length > 0
    ? <Badge icon={Package} tip="Stored inside its parent" />
    : <Badge icon={Link2} tip={`Linked to ${source.objectKey} in ${source.path}; edits there affect every use`} />;
}

/** Ownership of the member `source` of the open object, and a menu to change it; a layout fixture names its
 * `fixture` id, since fixtures may link one definition. `entries` add the member's own actions to the menu. */
export function OwnershipActions({ source, declarations, fixture, label, entries = [] }: {
  source: GuiObjectRef; declarations: GuiObjectRef[]; fixture?: number; label: string; entries?: MenuEntry[];
}) {
  const member: OwnershipMember = fixture === undefined ? { type: "object", source } : { type: "fixture", id: fixture };
  const [mode, setMode] = useState<"reusable" | "existing" | null>(null);
  const [selectedDeclaration, setSelectedDeclaration] = useState("");
  const [name, setName] = useState(label);
  const [storage, setStorage] = useState<ReusableStorage>("sameFile");
  const [error, setError] = useState<string | null>(null);
  const pending = useAppStore((state) => state.guiEditPending);
  const ready = useAppStore((state) => state.guiRequest !== null && state.guiDocumentRevision === state.guiRequest.projectRevision);
  const owned = source.ownedPath.length > 0;
  const choices = declarations.filter((candidate) => candidate.kind === source.kind && !sameGuiObject(candidate, source));
  const blocked = pending || !ready;
  const apply = async (edit: GuiOwnershipEdit) => {
    try {
      await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "ownership", member, edit }));
      setMode(null); setError(null);
    } catch (error) { setError(String(error)); }
  };
  const declaration = choices.find((candidate) => guiObjectKey(candidate) === selectedDeclaration);
  const menu: MenuEntry[] = [
    ...(owned ? [] : [{ type: "action", label: "Open declaration", icon: ExternalLink, onSelect: () => void navigateToGuiObject(source) } as const]),
    owned
      ? { type: "action", label: "Make reusable…", disabled: blocked, onSelect: () => { setName(label); setStorage("sameFile"); setError(null); setMode("reusable"); } }
      : { type: "action", label: "Make independent", disabled: blocked, onSelect: () => void apply({ type: "makeIndependent" }) },
    { type: "action", label: "Use existing…", disabled: blocked || choices.length === 0, onSelect: () => {
      setSelectedDeclaration(choices[0] === undefined ? "" : guiObjectKey(choices[0])); setError(null); setMode("existing");
    } },
    ...(entries.length === 0 ? [] : [{ type: "separator" } as const, ...entries])
  ];
  const submit = () => {
    if (mode === "existing") {
      if (declaration !== undefined) void apply({ type: "useExisting", declaration });
    } else {
      void apply({ type: "makeReusable", name: name.trim(), storage });
    }
  };
  return <>
    <span className="ownership-actions">
      <OwnershipBadge source={source} />
      <MenuButton entries={menu} align="end"><IconButton icon={MoreHorizontal} label={`${label} ownership`} /></MenuButton>
    </span>
    <Dialog
      open={mode !== null}
      onOpenChange={(open) => { if (!open) setMode(null); }}
      title={`${mode === "existing" ? "Use existing" : "Make reusable"}: ${label}`}
      actions={<>
        <Button onClick={() => { setMode(null); }}>Cancel</Button>
        <Button variant="primary" disabled={blocked || (mode === "existing" ? declaration === undefined : name.trim() === "")} onClick={submit}>
          {mode === "existing" ? "Use" : "Make reusable"}
        </Button>
      </>}
    >
      {mode === "existing"
        ? <Field label="Declaration">
          <Select name="Declaration" value={selectedDeclaration} disabled={blocked} onChange={setSelectedDeclaration}
            options={choices.map((candidate) => ({ value: guiObjectKey(candidate), label: `${candidate.objectKey} (${candidate.path})` }))} />
        </Field>
        : <>
          <Field label="Name"><TextField name="Name" value={name} disabled={blocked} invalid={name.trim() === ""} onCommit={setName} /></Field>
          <Field label="Save in"><Select name="Save in" value={storage} disabled={blocked} options={STORAGE_OPTIONS} onChange={setStorage} /></Field>
        </>}
      {error !== null && <p role="alert" className="kit-status-error">{error}</p>}
    </Dialog>
  </>;
}

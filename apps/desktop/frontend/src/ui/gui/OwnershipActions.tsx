import { guiObjectKey, sameGuiObject } from "../../workspace/guiIdentity";
import { ExternalLink, FileCode, MoreHorizontal } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { commands } from "../../api";
import { runGuiEditCommand, useAppStore } from "../../store";
import type { GuiEditCommand, GuiObjectRef, GuiOwnershipEdit, OwnershipMember, ReusableStorage } from "../../types";
import { navigateToGuiObject, navigateToText } from "../../workspace/navigation";
import { Button, Dialog, Field, IconButton, MenuButton, Select, TextField, type MenuEntry } from "../kit";
import { DEFAULT_OBJECT_STORAGE, ObjectStorageField, REUSABLE_STORAGE } from "./objectStorage";
import { DraftProblem, OBJECT, useDraftCheck, useFieldProblem } from "./problems";

type Mode = "reusable" | "existing";

/** The "…" menu of the member `source` of the open object: open it, its own `entries`, then where it is stored. A
 * layout fixture names its `fixture` id, since fixtures may link one definition. */
export function OwnershipActions({ source, declarations, fixture, label, entries = [] }: {
  source: GuiObjectRef; declarations: GuiObjectRef[]; fixture?: number; label: string; entries?: MenuEntry[];
}) {
  const member: OwnershipMember = fixture === undefined ? { type: "object", source } : { type: "fixture", id: fixture };
  const [mode, setMode] = useState<Mode | null>(null);
  const pending = useAppStore((state) => state.guiEditPending);
  const ready = useAppStore((state) => state.guiRequest !== null && state.guiDocumentRevision === state.guiRequest.projectRevision);
  const owned = source.ownedPath.length > 0;
  const choices = declarations.filter((candidate) => candidate.kind === source.kind && !sameGuiObject(candidate, source));
  const blocked = pending || !ready;
  const menu: MenuEntry[] = [
    { type: "action", label: "Open", icon: ExternalLink, onSelect: () => void navigateToGuiObject(source) },
    { type: "action", label: "Open as text", icon: FileCode, onSelect: () => void navigateToText(source.path, null) },
    ...entries,
    { type: "separator" },
    owned
      ? { type: "action", label: "Move to file…", disabled: blocked, onSelect: () => { setMode("reusable"); } }
      : { type: "action", label: "Copy inside parent", disabled: blocked, onSelect: () => void runGuiEditCommand((request) =>
        commands.applyGuiEdit(request, { type: "ownership", member, edit: { type: "makeIndependent" } })) },
    { type: "action", label: "Use another…", disabled: blocked || choices.length === 0, onSelect: () => { setMode("existing"); } }
  ];
  return <>
    <MenuButton entries={menu} align="end"><IconButton icon={MoreHorizontal} label={`${label} actions`} /></MenuButton>
    {mode !== null && <OwnershipDialog mode={mode} member={member} label={label} choices={choices} blocked={blocked} onClose={() => { setMode(null); }} />}
  </>;
}

/** Moving a member to a file, or using another declaration in its place: a draft checked as it changes. */
function OwnershipDialog({ mode, member, label, choices, blocked, onClose }: {
  mode: Mode; member: OwnershipMember; label: string; choices: GuiObjectRef[]; blocked: boolean; onClose: () => void;
}) {
  const [selectedDeclaration, setSelectedDeclaration] = useState(choices[0] === undefined ? "" : guiObjectKey(choices[0]));
  const [name, setName] = useState(label);
  const [storage, setStorage] = useState<ReusableStorage>(DEFAULT_OBJECT_STORAGE);
  const request = useAppStore((state) => state.guiRequest);
  const declaration = choices.find((candidate) => guiObjectKey(candidate) === selectedDeclaration);
  const edit = useMemo<GuiEditCommand | null>(() => {
    const ownership: GuiOwnershipEdit | null = mode === "existing"
      ? declaration === undefined ? null : { type: "useExisting", declaration }
      : name.trim() === "" ? null : { type: "makeReusable", name: name.trim(), storage };
    return ownership === null ? null : { type: "ownership", member, edit: ownership };
  }, [mode, declaration, name, storage, member]);
  const { checking, problem } = useDraftCheck(request, edit);
  const apply = () => {
    if (edit !== null) void runGuiEditCommand((current) => commands.applyGuiEdit(current, edit)).then((result) => { if (result !== null) onClose(); });
  };
  return <Dialog
    open
    onOpenChange={(open) => { if (!open) onClose(); }}
    title={mode === "existing" ? `Use another for ${label}` : `Move ${label} to a file`}
    actions={<>
      <Button onClick={onClose}>Cancel</Button>
      <Button variant="primary" disabled={blocked || edit === null || checking || problem !== null} {...(problem === null ? {} : { tip: problem.message })} onClick={apply}>
        {mode === "existing" ? "Use" : "Move"}
      </Button>
    </>}
  ><DraftProblem problem={problem}>
    {mode === "existing"
      ? <DialogField label="Use" field="declaration">{(invalid) =>
        <Select name="Use" value={selectedDeclaration} disabled={blocked} invalid={invalid} onChange={setSelectedDeclaration}
          options={choices.map((candidate) => ({ value: guiObjectKey(candidate), label: candidate.objectKey }))} />}
      </DialogField>
      : <>
        <DialogField label="Name" field="name" missing={name.trim() === ""}>{(invalid) =>
          <TextField name="Name" value={name} disabled={blocked} invalid={invalid} onCommit={setName} />}
        </DialogField>
        <ObjectStorageField value={storage} choices={REUSABLE_STORAGE} disabled={blocked} onChange={setStorage} />
      </>}
  </DraftProblem></Dialog>;
}

/** A field of the dialog, marked when the draft breaks a rule there or lacks its value. */
function DialogField({ label, field, missing = false, children }: { label: string; field: "name" | "declaration"; missing?: boolean; children: (invalid: boolean) => ReactNode }) {
  const problem = useFieldProblem(OBJECT, { type: field });
  return <Field label={label} error={problem}>{children(problem !== null || missing)}</Field>;
}

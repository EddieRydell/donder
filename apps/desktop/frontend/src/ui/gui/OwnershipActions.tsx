import { guiObjectKey, sameGuiObject } from "../../workspace/guiIdentity";
import * as Dialog from "@radix-ui/react-dialog";
import { MoreHorizontal } from "lucide-react";
import { useState, type ReactNode } from "react";
import { commands } from "../../api";
import { runGuiEditCommand, useAppStore } from "../../store";
import type { GuiObjectRef, OwnershipMember, ReusableStorage } from "../../types";
import { navigateToGuiObject } from "../../workspace/navigation";

export function ownershipLabel(source: GuiObjectRef): string {
  return source.ownedPath.length > 0 ? "Stored here" : `Linked - ${source.path}`;
}

/** Ownership actions for the member `source` of the open object; a layout fixture names its `fixture` id, since fixtures may link one definition. */
export function OwnershipActions({ source, declarations, fixture, label, children }: {
  source: GuiObjectRef; declarations: GuiObjectRef[]; fixture?: number; label: string; children?: ReactNode;
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
  const apply = async (edit: import("../../types").GuiOwnershipEdit) => {
    try {
      await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "ownership", member, edit }));
      setMode(null); setError(null);
    } catch (error) { setError(String(error)); }
  };
  return <>
    <details className="ownership-actions">
      <summary aria-label={`${label} ownership actions`}><MoreHorizontal aria-hidden="true" /></summary>
      <div>
        <p className="param-source-description">{owned ? "Stored inside its parent, without a separate declaration." : "Linked to a reusable declaration. Editing the declaration affects every use."}</p>
        {!owned && <button type="button" onClick={() => void navigateToGuiObject(source)}>Open declaration</button>}
        {owned
          ? <button type="button" disabled={pending || !ready} onClick={() => { setName(label); setStorage("sameFile"); setError(null); setMode("reusable"); }}>Make reusable...</button>
          : <button type="button" disabled={pending || !ready} onClick={() => void apply({ type: "makeIndependent" })}>Make independent</button>}
        <button type="button" disabled={pending || !ready || choices.length === 0} onClick={() => { setSelectedDeclaration(choices[0] === undefined ? "" : guiObjectKey(choices[0])); setError(null); setMode("existing"); }}>Use existing declaration...</button>
        {children}
        {error !== null && mode === null && <p role="alert">{error}</p>}
      </div>
    </details>
    <Dialog.Root open={mode !== null} onOpenChange={(open) => { if (!open) setMode(null); }}>
      <Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog-content setup-creation-dialog">
        <Dialog.Title>{mode === "existing" ? "Use existing declaration" : "Make reusable"}: {label}</Dialog.Title>
        <Dialog.Description>{mode === "existing" ? "Replace the current contents or link with a reusable declaration. Changes to that declaration will be shared." : "Move this item to a named declaration and keep a link here. Other items can then use the same declaration."}</Dialog.Description>
        <form onSubmit={(event) => { event.preventDefault(); if (mode === "existing") {
            const declaration = choices.find((candidate) => guiObjectKey(candidate) === selectedDeclaration);
            if (declaration !== undefined) void apply({ type: "useExisting", declaration });
          } else { void apply({ type: "makeReusable", name: name.trim(), storage }); } }}>
          <fieldset className="composition-controls" disabled={pending || !ready}>
            {mode === "existing" ? <label>Reusable declaration<select value={selectedDeclaration} onChange={(event) => { setSelectedDeclaration(event.target.value); }}>
              {choices.map((source) => <option key={guiObjectKey(source)} value={guiObjectKey(source)}>{source.objectKey} ({source.path})</option>)}
            </select></label> : <><label>Name<input required value={name} onChange={(event) => { setName(event.target.value); }} /></label>
            <details className="composition-add-advanced"><summary>Advanced settings</summary>
              <label>Save declaration in<select value={storage} onChange={(event) => { if (event.target.value === "sameFile" || event.target.value === "newFile") setStorage(event.target.value); }}>
                <option value="sameFile">This file</option><option value="newFile">A new file</option>
              </select></label>
            </details></>}
            {error !== null && <p role="alert">{error}</p>}
            <div className="dialog-actions"><Dialog.Close asChild><button type="button">Cancel</button></Dialog.Close><button type="submit" disabled={mode === "existing" ? !choices.some((source) => guiObjectKey(source) === selectedDeclaration) : name.trim() === ""}>{mode === "existing" ? "Use declaration" : "Make reusable"}</button></div>
          </fieldset>
        </form>
      </Dialog.Content></Dialog.Portal>
    </Dialog.Root>
  </>;
}

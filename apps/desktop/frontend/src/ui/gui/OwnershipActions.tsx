import { guiObjectKey, sameGuiObject } from "../../workspace/guiIdentity";
import * as Dialog from "@radix-ui/react-dialog";
import { MoreHorizontal } from "lucide-react";
import { useState, type ReactNode } from "react";
import { commands } from "../../api";
import { runGuiEditCommand, useAppStore } from "../../store";
import type { GuiObjectRef, GuiOwnershipSlot, ReusableStorage } from "../../types";
import { navigateToGuiObject } from "../../workspace/navigation";

export function ownershipLabel(source: GuiObjectRef): string {
  return source.ownedPath.length > 0 ? "Stored here" : `Linked - ${source.path}`;
}

export function OwnershipActions({ source, sources, slot, label, children }: {
  source: GuiObjectRef; sources: GuiObjectRef[]; slot: GuiOwnershipSlot; label: string; children?: ReactNode;
}) {
  const [mode, setMode] = useState<"reusable" | "existing" | null>(null);
  const [selectedSource, setSelectedSource] = useState("");
  const [name, setName] = useState(label);
  const [storage, setStorage] = useState<ReusableStorage>("sameFile");
  const [error, setError] = useState<string | null>(null);
  const pending = useAppStore((state) => state.guiEditPending);
  const readOnly = useAppStore((state) => state.snapshot?.activeBuffer?.readOnly ?? false);
  const ready = useAppStore((state) => state.guiRequest !== null && state.guiDocumentRevision === state.guiRequest.projectRevision);
  const owned = source.ownedPath.length > 0;
  const choices = sources.filter((candidate) => candidate.kind === source.kind && !sameGuiObject(candidate, source));
  const apply = async (edit: import("../../types").GuiOwnershipEdit) => {
    try {
      await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "ownership", slot, edit }));
      setMode(null); setError(null);
    } catch (error) { setError(String(error)); }
  };
  return <>
    <details className="ownership-actions">
      <summary aria-label={`${label} source actions`}><MoreHorizontal aria-hidden="true" /></summary>
      <div>
        <p className="param-source-description">{owned ? "Stored inside its parent, without a separate source." : "Linked to a reusable source. Editing the source affects every use."}</p>
        {!owned && <button type="button" onClick={() => void navigateToGuiObject(source)}>Open source</button>}
        {owned
          ? <button type="button" disabled={pending || readOnly || !ready} onClick={() => { setName(label); setStorage("sameFile"); setError(null); setMode("reusable"); }}>Make reusable...</button>
          : <button type="button" disabled={pending || readOnly || !ready} onClick={() => void apply({ type: "makeIndependent" })}>Make independent</button>}
        <button type="button" disabled={pending || readOnly || !ready || choices.length === 0} onClick={() => { setSelectedSource(choices[0] === undefined ? "" : guiObjectKey(choices[0])); setError(null); setMode("existing"); }}>Use existing source...</button>
        {children}
        {error !== null && mode === null && <p role="alert">{error}</p>}
      </div>
    </details>
    <Dialog.Root open={mode !== null} onOpenChange={(open) => { if (!open) setMode(null); }}>
      <Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog-content setup-creation-dialog">
        <Dialog.Title>{mode === "existing" ? "Use existing source" : "Make reusable"}: {label}</Dialog.Title>
        <Dialog.Description>{mode === "existing" ? "Replace the current contents or link with a reusable source. Changes to that source will be shared." : "Move this item to a named source and keep a link here. Other items can then use the same source."}</Dialog.Description>
        <form onSubmit={(event) => { event.preventDefault(); if (mode === "existing") {
            const source = choices.find((candidate) => guiObjectKey(candidate) === selectedSource);
            if (source !== undefined) void apply({ type: "useExisting", source });
          } else { void apply({ type: "makeReusable", name: name.trim(), storage }); } }}>
          <fieldset className="composition-controls" disabled={pending || readOnly || !ready}>
            {mode === "existing" ? <label>Reusable source<select value={selectedSource} onChange={(event) => { setSelectedSource(event.target.value); }}>
              {choices.map((source) => <option key={guiObjectKey(source)} value={guiObjectKey(source)}>{source.objectKey} ({source.path})</option>)}
            </select></label> : <><label>Name<input required value={name} onChange={(event) => { setName(event.target.value); }} /></label>
            <details className="composition-add-advanced"><summary>Advanced settings</summary>
              <label>Save source in<select value={storage} onChange={(event) => { if (event.target.value === "sameFile" || event.target.value === "newFile") setStorage(event.target.value); }}>
                <option value="sameFile">This file</option><option value="newFile">A new file</option>
              </select></label>
            </details></>}
            {error !== null && <p role="alert">{error}</p>}
            <div className="dialog-actions"><Dialog.Close asChild><button type="button">Cancel</button></Dialog.Close><button type="submit" disabled={mode === "existing" ? !choices.some((source) => guiObjectKey(source) === selectedSource) : name.trim() === ""}>{mode === "existing" ? "Use source" : "Make reusable"}</button></div>
          </fieldset>
        </form>
      </Dialog.Content></Dialog.Portal>
    </Dialog.Root>
  </>;
}

import { Pencil, Trash2 } from "lucide-react";
import { useState } from "react";
import { useAppStore } from "../../store";
import { put, remove } from "./edits";
import type { GuiObjectRef } from "../../types";
import { IconButton } from "../kit";
import { NameDialog } from "./NameDialog";

/** Renaming and deleting `source`, a member of the open project called `label`. A member the project
 * cannot do without is not `deletable`. */
export function MemberActions({ source, label, deletable = true }: { source: GuiObjectRef; label: string; deletable?: boolean }) {
  const [renaming, setRenaming] = useState(false);
  const blocked = useAppStore((state) => state.guiEditPending || state.guiRequest === null || state.guiDocumentRevision !== state.guiRequest.projectRevision);
  return <>
    <IconButton icon={Pencil} label={`Rename ${label}`} disabled={blocked} onClick={() => { setRenaming(true); }} />
    {deletable && <IconButton icon={Trash2} label={`Delete ${label}`} disabled={blocked} onClick={() => void remove({ object: source, member: null, field: null })} />}
    {renaming && <NameDialog title={`Rename ${label}`} action="Rename" initial={label}
      submit={(name) => put({ object: source, member: null, field: { type: "name" } }, { type: "text", text: name })}
      onClose={() => { setRenaming(false); }} />}
  </>;
}

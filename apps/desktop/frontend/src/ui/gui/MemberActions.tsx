import { Pencil, Trash2 } from "lucide-react";
import { useState } from "react";
import { commands } from "../../api";
import { runGuiEditCommand, useAppStore } from "../../store";
import type { GuiObjectRef } from "../../types";
import { guiObjectName } from "../../workspace/guiIdentity";
import { IconButton } from "../kit";
import { NameDialog } from "./NameDialog";

/** A member's name; one stored unnamed inside its owner, such as an inline setup, goes by its kind. */
export function memberName(source: GuiObjectRef): string {
  return guiObjectName(source) ?? source.kind;
}

/** Renaming and deleting `source`, a member of the open project or setup called `label`. A member the project
 * cannot do without is not `deletable`. */
export function MemberActions({ source, label, deletable = true }: { source: GuiObjectRef; label: string; deletable?: boolean }) {
  const [renaming, setRenaming] = useState(false);
  const blocked = useAppStore((state) => state.guiEditPending || state.guiRequest === null || state.guiDocumentRevision !== state.guiRequest.projectRevision);
  return <>
    <IconButton icon={Pencil} label={`Rename ${label}`} disabled={blocked} onClick={() => { setRenaming(true); }} />
    {deletable && <IconButton icon={Trash2} label={`Delete ${label}`} disabled={blocked} onClick={() => void runGuiEditCommand((request) => commands.deleteGuiMember(request, source))} />}
    {renaming && <NameDialog title={`Rename ${label}`} action="Rename" initial={label}
      submit={(name) => runGuiEditCommand((request) => commands.renameGuiMember(request, source, name))}
      onClose={() => { setRenaming(false); }} />}
  </>;
}

import { ChevronRight } from "lucide-react";
import { openGuiParent, useAppStore } from "../store";
import type { GuiDocumentRequest } from "../types";
import { THEME_METRICS } from "../theme";
import { Button, Tip } from "./kit";

/**
 * The objects the open one was opened from, each returning to itself, then the open object. The row
 * is always there in the GUI, so opening a nested object never moves the editor; long labels ellipsize
 * and show in full in their tooltips.
 */
export function GuiBreadcrumb({ request }: { request: GuiDocumentRequest | null }) {
  const parents = useAppStore((store) => store.guiParents);
  const pending = useAppStore((store) => store.guiEditPending);
  const current = request === null ? null : request.objectKey;
  return <nav className="gui-breadcrumb" aria-label="Opened from">
    {request !== null && parents.map((parent, depth) => {
      const name = parent.request.objectKey;
      return <span className="gui-breadcrumb-step" key={depth}>
        <Button variant="ghost" tip={name} disabled={pending} onClick={() => { openGuiParent(depth); }}><span className="gui-breadcrumb-label">{name}</span></Button>
        <ChevronRight size={THEME_METRICS.iconSizeSmall} aria-hidden />
      </span>;
    })}
    {current !== null && <Tip label={current}><span className="gui-breadcrumb-current" aria-current="page">{current}</span></Tip>}
  </nav>;
}

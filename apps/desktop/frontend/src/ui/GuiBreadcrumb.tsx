import { ChevronRight } from "lucide-react";
import { openGuiParent, useAppStore } from "../store";
import type { GuiDocument, GuiDocumentRequest } from "../types";
import { THEME_METRICS } from "../theme";
import { Button, Tip } from "./kit";

/** An open object's label; an owned fixture's name is in its owner, the layout. */
function label(request: GuiDocumentRequest, owner: GuiDocument | null): string {
  const slot = request.ownedPath[request.ownedPath.length - 1];
  if (slot === undefined) return request.objectKey;
  switch (slot.type) {
    case "setup": return "Display setup";
    case "layout": return "Layout";
    case "patch": return "Patch";
    case "fixture": return (owner?.type === "layout" ? owner.document.fixtures.find((fixture) => fixture.id === slot.id)?.name : undefined) ?? "Fixture";
    case "controller":
    case "inputDevice":
    case "sequence":
    case "console": return slot.name;
  }
}

/**
 * The objects the open one was opened from, each returning to itself, then the open object. The row
 * is always there in the GUI, so opening a nested object never moves the editor; long labels ellipsize
 * and show in full in their tooltips.
 */
export function GuiBreadcrumb({ request }: { request: GuiDocumentRequest | null }) {
  const parents = useAppStore((store) => store.guiParents);
  const pending = useAppStore((store) => store.guiEditPending);
  const current = request === null ? null : label(request, parents[parents.length - 1]?.document ?? null);
  return <nav className="gui-breadcrumb" aria-label="Opened from">
    {request !== null && parents.map((parent, depth) => {
      const name = label(parent.request, parents[depth - 1]?.document ?? null);
      return <span className="gui-breadcrumb-step" key={depth}>
        <Button variant="ghost" tip={name} disabled={pending} onClick={() => { openGuiParent(depth); }}><span className="gui-breadcrumb-label">{name}</span></Button>
        <ChevronRight size={THEME_METRICS.iconSizeSmall} aria-hidden />
      </span>;
    })}
    {current !== null && <Tip label={current}><span className="gui-breadcrumb-current" aria-current="page">{current}</span></Tip>}
  </nav>;
}

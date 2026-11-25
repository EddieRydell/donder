import { useCallback, useEffect, useRef, useState } from "react";
import { commands } from "../../../api";
import { runSnapshotCommand } from "../../../store";
import { setDocumentUi } from "../documentUi";
import { OverlayPortal } from "../../OverlayPortal";
import { Dialog } from "../../kit";
import type { CompositionEditing } from "./composition";
import { GraphCanvas } from "./GraphCanvas";
import { useGraphDeletion } from "./sequenceLayers";
import { useUnplacedRefusal } from "../problems";

/** Closing the graph clears what it focused; the render follows the graph once it closes. */
function closeGraph() {
  setDocumentUi({ graphOpen: false, focus: null });
}

export function GraphEditorModal({ composition, description }: {
  composition: CompositionEditing;
  /** What the graph composes: the dialog's title. */
  description: string;
}) {
  useEffect(() => () => { void runSnapshotCommand(commands.finishCompositionGraphEditing); }, []);

  const content = useRef<HTMLDivElement>(null);
  const [portal, setPortal] = useState<HTMLDivElement | null>(null);
  const contentRef = useCallback((node: HTMLDivElement | null) => { content.current = node; setPortal(node); }, []);
  const [previousFocus] = useState(() => window.document.activeElement);
  // The graph covers the status bar, so it says a refused edit no field shows.
  const error = useUnplacedRefusal();
  const clearSelection = useCallback(() => { setDocumentUi({ focus: null }); }, []);
  const requestDelete = useGraphDeletion(composition, clearSelection);
  return <Dialog
    ref={contentRef}
    open
    onOpenChange={(open) => { if (!open) closeGraph(); }}
    title={description}
    size="full"
    focus={{
      onOpenAutoFocus: (event) => {
        event.preventDefault();
        content.current?.querySelector<HTMLElement>(".graph-flow-pane")?.focus();
      },
      onCloseAutoFocus: (event) => {
        event.preventDefault();
        if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus();
      },
      onEscapeKeyDown: (event) => {
        if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) event.preventDefault();
      }
    }}
  >
    <OverlayPortal value={portal}><div className="graph-workspace">
      {error !== null && <p className="kit-status-error graph-edit-error" role="alert">{error}</p>}
      <div className="graph-modal-body">
        <GraphCanvas composition={composition} requestDelete={requestDelete} />
      </div>
    </div></OverlayPortal>
  </Dialog>;
}

import { useSequenceEditorHost } from "../../../editor/host";
import { useCallback, useMemo, useRef, useState } from "react";
import { OverlayPortal } from "../../OverlayPortal";
import { Dialog } from "../../kit";
import type { CompositionEditing } from "./composition";
import { GraphCanvas, type SelectedGraphItem } from "./GraphCanvas";
import { useGraphDeletion } from "./sequenceLayers";

export function GraphEditorModal({
  composition, description, setSelectedItem, onClose
}: {
  composition: CompositionEditing;
  /** What the graph composes: the dialog's title. */
  description: string;
  setSelectedItem: (item: SelectedGraphItem) => void;
  onClose: () => void;
}) {
  const host = useSequenceEditorHost();
  const { store: useAppStore } = host;

  const content = useRef<HTMLDivElement>(null);
  const [portal, setPortal] = useState<HTMLDivElement | null>(null);
  const contentRef = useCallback((node: HTMLDivElement | null) => { content.current = node; setPortal(node); }, []);
  const [previousFocus] = useState(() => window.document.activeElement);
  const error = useAppStore((state) => state.error);
  const clearSelection = useCallback(() => { setSelectedItem(null); }, [setSelectedItem]);
  // Choosing an automation clip happens on the timeline, behind the graph.
  const automation = composition.automation;
  const editing = useMemo<CompositionEditing>(() => automation === null ? composition : {
    ...composition,
    automation: { ...automation, setAutomationClipChooser: (chooser) => {
      automation.setAutomationClipChooser(chooser);
      if (chooser !== null) onClose();
    } }
  }, [composition, automation, onClose]);
  const { requestDelete, dialog } = useGraphDeletion(editing, clearSelection);
  return <Dialog
    ref={contentRef}
    open
    onOpenChange={(open) => { if (!open) onClose(); }}
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
        <GraphCanvas composition={editing} setSelectedItem={setSelectedItem} requestDelete={requestDelete} />
      </div>
    </div></OverlayPortal>
    {dialog}
  </Dialog>;
}

import { useSequenceEditorHost } from "../../../editor/host";
import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { useCallback, useMemo, useRef, useState, useContext } from "react";
import { OverlayPortal } from "../../OverlayPortal";
import { THEME_METRICS } from "../../../theme";
import type { CompositionEditing } from "./composition";
import { GraphCanvas, type SelectedGraphItem } from "./GraphCanvas";
import { useGraphDeletion } from "./sequenceLayers";

export function GraphEditorModal({
  composition, description, setSelectedItem, onClose
}: {
  composition: CompositionEditing;
  /** What the graph composes, under the title. */
  description: string;
  setSelectedItem: (item: SelectedGraphItem) => void;
  onClose: () => void;
}) {
  const host = useSequenceEditorHost();
  const overlayContainer = useContext(OverlayPortal);
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
  return <Dialog.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
    <Dialog.Portal container={overlayContainer}>
      <Dialog.Overlay className="graph-modal-backdrop" />
      <Dialog.Content ref={contentRef} className="graph-modal" aria-describedby={undefined} onOpenAutoFocus={(event) => {
        event.preventDefault();
        content.current?.querySelector<HTMLElement>(".graph-flow-pane")?.focus();
      }} onCloseAutoFocus={(event) => {
        event.preventDefault();
        if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus();
      }} onEscapeKeyDown={(event) => {
        if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) event.preventDefault();
      }}>
        <div className="graph-modal-header">
          <div><Dialog.Title>Composition Graph</Dialog.Title><span>{description}</span></div>
          <Dialog.Close asChild><button type="button" className="graph-modal-close" aria-label="Close graph editor" title="Close"><X size={THEME_METRICS.iconSizeMedium} /></button></Dialog.Close>
        </div>
        <OverlayPortal value={portal}><div className="graph-workspace">
          {error !== null && <p className="graph-edit-error" role="alert">{error}</p>}
          <div className="graph-modal-body">
            <GraphCanvas composition={editing} setSelectedItem={setSelectedItem} requestDelete={requestDelete} />
          </div>
        </div></OverlayPortal>
        {dialog}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>;
}

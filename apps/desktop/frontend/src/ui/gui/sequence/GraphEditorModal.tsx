import { useSequenceEditorHost } from "../../../editor/host";
import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { OverlayPortal } from "../../OverlayPortal";
import { THEME_METRICS } from "../../../theme";
import type { SequenceEditorDocument } from "../../../editor/types";
import type { AutomationClipChooser } from "../shared";
import { GraphCanvas, type SelectedGraphItem } from "./GraphCanvas";
import { useGraphDeletion } from "./sequenceLayers";

export function GraphEditorModal({
  document, setSelectedItem, automationClipChooser, setAutomationClipChooser, onClose
}: {
  document: SequenceEditorDocument;
  setSelectedItem: (item: SelectedGraphItem) => void;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
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
  const { requestDelete, dialog } = useGraphDeletion(document, clearSelection);
  const chooseAutomation = useCallback((chooser: AutomationClipChooser) => {
    setAutomationClipChooser(chooser);
    if (chooser !== null) onClose();
  }, [setAutomationClipChooser, onClose]);
  return <Dialog.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
    <Dialog.Portal>
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
          <div><Dialog.Title>Composition Graph</Dialog.Title><span>Sequence layers and operators</span></div>
          <Dialog.Close asChild><button type="button" className="graph-modal-close" aria-label="Close graph editor" title="Close"><X size={THEME_METRICS.iconSizeMedium} /></button></Dialog.Close>
        </div>
        <OverlayPortal value={portal}><div className="graph-workspace">
          {error !== null && <p className="graph-edit-error" role="alert">{error}</p>}
          <div className="graph-modal-body">
            <GraphCanvas document={document} setSelectedItem={setSelectedItem} requestDelete={requestDelete}
              automationClipChooser={automationClipChooser} setAutomationClipChooser={chooseAutomation} />
          </div>
        </div></OverlayPortal>
        {dialog}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>;
}

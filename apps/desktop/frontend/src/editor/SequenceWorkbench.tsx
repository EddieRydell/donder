import { useEffect, useState } from "react";
import { OverlayPortal } from "../ui/OverlayPortal";
import { SequenceEditor } from "../ui/gui/sequence/SequenceEditor";
import { SequenceInspector } from "../ui/gui/sequence/SequenceInspector";
import { SequenceTransportControls } from "../ui/gui/sequence/SequenceTransportControls";
import { OPEN_LAYER_GRAPH_EVENT } from "../ui/uiEvents";
import type { AutomationClipChooser, GuiFocus, SequenceSelection } from "../ui/gui/shared";
import type { SequenceEditorDocument } from "./types";

export function SequenceWorkbench({ document }: { document: SequenceEditorDocument }) {
  const [selected, setSelected] = useState<GuiFocus>(null);
  const [sequenceSelection, setSequenceSelection] = useState<SequenceSelection>(null);
  const [compositionGraphOpen, setCompositionGraphOpen] = useState(false);
  const [automationClipChooser, setAutomationClipChooser] = useState<AutomationClipChooser>(null);
  const [overlays, setOverlays] = useState<HTMLDivElement | null>(null);
  const [activeMarkCollectionKey, setActiveMarkCollectionKey] = useState<string | null>(document.markCollections[0]?.key ?? null);
  const [visibleMarkCollectionKeys, setVisibleMarkCollectionKeys] = useState(() => new Set(document.markCollections.map(collection => collection.key)));
  useEffect(() => {
    const open = () => { setCompositionGraphOpen(true); };
    window.addEventListener(OPEN_LAYER_GRAPH_EVENT, open);
    return () => { window.removeEventListener(OPEN_LAYER_GRAPH_EVENT, open); };
  }, []);
  const editorProps = {
    document, selected, setSelected, sequenceSelection, setSequenceSelection,
    automationClipChooser, setAutomationClipChooser, activeMarkCollectionKey, setActiveMarkCollectionKey,
    visibleMarkCollectionKeys, setVisibleMarkCollectionKeys
  };
  // An embedded editor is one isolated surface: menus and dialogs portal into
  // its own overlay layer instead of the host page's body.
  return <div className="shared-sequence-workbench donder-editor">
    <OverlayPortal value={overlays}>
      <SequenceTransportControls document={document} previewOpen={false} />
      <div className="shared-sequence-workspace">
        <SequenceEditor {...editorProps} compositionGraphOpen={compositionGraphOpen} setCompositionGraphOpen={setCompositionGraphOpen} />
        <aside className="shared-sequence-inspector"><SequenceInspector {...editorProps} /></aside>
      </div>
    </OverlayPortal>
    <div ref={setOverlays} className="donder-editor-overlays" />
  </div>;
}

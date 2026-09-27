import { useCallback, type KeyboardEvent } from "react";

import type { SequenceEditorDocument } from "../../../types";
import { commands } from "../../../api";
import { runSnapshotCommand, useAppStore } from "../../../store";

import type { AutomationClipChooser, GuiFocus, SequenceSelection } from "../shared";

import { SequenceCanvas } from "./SequenceCanvas";
import { GraphEditorModal } from "./GraphEditorModal";
import { handleSequencePlaybackShortcut, isSequenceTransportUnsupported } from "./SequenceTransportControls";

export function SequenceEditor({
  document,
  selected,
  setSelected,
  compositionGraphOpen,
  setCompositionGraphOpen,
  sequenceSelection,
  setSequenceSelection,
  automationClipChooser,
  setAutomationClipChooser,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  document: SequenceEditorDocument;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  compositionGraphOpen: boolean;
  setCompositionGraphOpen: (open: boolean) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  const setSelectedGraphItem = useCallback(
    (item: { type: "node"; id: string } | { type: "edge"; id: string } | null) => {
      if (item === null) {
        setSelected(null);
        return;
      }
      setSelected(
        item.type === "node"
          ? { type: "graphNode", nodeId: item.id }
          : { type: "graphEdge", edgeId: item.id }
      );
    },
    [setSelected]
  );
  const closeCompositionGraph = useCallback(() => {
    setCompositionGraphOpen(false);
    setSelected(null);
    void runSnapshotCommand(commands.finishCompositionGraphEditing);
  }, [setCompositionGraphOpen, setSelected]);
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (compositionGraphOpen) return;
    if (event.key === "Escape" && automationClipChooser !== null) {
      event.preventDefault();
      setAutomationClipChooser(null);
      return;
    }
    const transport = useAppStore.getState().snapshot?.audioTransport;
    if (transport === undefined) return;
    handleSequencePlaybackShortcut(event, document, transport, isSequenceTransportUnsupported(document, transport));
  };
  return (
    <div className="sequence-editor" tabIndex={-1} onKeyDown={handleKeyDown}>
      <SequenceCanvas
        document={document}
        selected={selected}
        setSelected={setSelected}
        sequenceSelection={sequenceSelection}
        setSequenceSelection={setSequenceSelection}
        automationClipChooser={automationClipChooser}
        setAutomationClipChooser={setAutomationClipChooser}
        activeMarkCollectionKey={activeMarkCollectionKey}
        setActiveMarkCollectionKey={setActiveMarkCollectionKey}
        visibleMarkCollectionKeys={visibleMarkCollectionKeys}
        setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
      />
      {compositionGraphOpen && (
        <GraphEditorModal
          document={document}
          setSelectedItem={setSelectedGraphItem}
          automationClipChooser={automationClipChooser}
          setAutomationClipChooser={setAutomationClipChooser}
          onClose={closeCompositionGraph}
        />
      )}
    </div>
  );
}

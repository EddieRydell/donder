import { commands } from "../../../api";
import { runSnapshotCommand } from "../../../store";
import { useCallback, useMemo } from "react";

import type { SequenceGuiDocument } from "../../../types";

import type { AutomationClipChooser, GuiFocus, SequenceSelection } from "../shared";

import { SequenceCanvas } from "./SequenceCanvas";
import { GraphEditorModal } from "./GraphEditorModal";
import type { SelectedGraphItem } from "./GraphCanvas";
import { sequenceComposition } from "./composition";

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
  document: SequenceGuiDocument;
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
    (item: SelectedGraphItem) => {
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
  const composition = useMemo(() => sequenceComposition(document, {
    automationClips: document.automationClips, automationClipChooser, setAutomationClipChooser
  }), [document, automationClipChooser, setAutomationClipChooser]);
  return (
    <div className="sequence-editor">
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
          composition={composition}
          description="Sequence layers and operators"
          setSelectedItem={setSelectedGraphItem}
          onClose={closeCompositionGraph}
        />
      )}
    </div>
  );
}

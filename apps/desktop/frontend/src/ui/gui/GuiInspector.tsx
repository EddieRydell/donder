import type { AutomationClipChooser, GuiFocus, InspectedGuiDocument, SequenceSelection } from "./shared";
import { ConsoleInspector } from "./console/ConsoleInspector";
import { SequenceInspector } from "./sequence/SequenceInspector";

export function GuiInspector({
  gui,
  selected,
  setSelected,
  sequenceSelection,
  setSequenceSelection,
  automationClipChooser,
  setAutomationClipChooser,
  activeMarkCollectionKey,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  gui: InspectedGuiDocument;
  selected: GuiFocus;
  setSelected: (id: GuiFocus) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
  activeMarkCollectionKey: string | null;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  if (gui.type === "console") return <ConsoleInspector document={gui.document} selected={selected} setSelected={setSelected} />;
  return (
    <SequenceInspector
      document={gui.document}
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
  );
}

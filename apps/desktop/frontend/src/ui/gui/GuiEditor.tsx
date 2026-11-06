import { guiObjectKey, objectViewKey } from "../../workspace/guiIdentity";
import { CompositionEditor } from "./composition/CompositionEditor";
import { useEffect, useState } from "react";

import type { GuiDocument, WorkspaceLayoutState } from "../../types";
import type { AppStaticSnapshot } from "../../store";

import type { AutomationClipChooser, GuiFocus, InspectedGuiDocument, ReadyGuiDocument, SequenceSelection } from "./shared";



import { GuiInspector } from "./GuiInspector";

import { BlockedGui } from "./BlockedGui";

import { SequenceEditor } from "./sequence/SequenceEditor";
import { runGuiEditCommand, useAppStore } from "../../store";
import { commands } from "../../api";
import { DescriptionText } from "./DescriptionField";
import { THEME_METRICS } from "../../theme";


import { WorkspaceResizeHandle } from "../WorkspaceResizeHandle";
import { listenUiEvent } from "../uiEvents";
import { SetupEditor } from "./setup/SetupEditor";
import { ProjectEditor } from "./project/ProjectEditor";
import { LibraryEditor } from "./library/LibraryEditor";
import { ControllerEditor } from "./controller/ControllerEditor";
import { PatchEditor } from "./patch/PatchEditor";
import { InputDeviceEditor } from "./inputDevice/InputDeviceEditor";
import { ConsoleEditor } from "./console/ConsoleEditor";

const INSPECTOR_MIN_WIDTH_PX = THEME_METRICS.inspectorMinWidth;
const INSPECTOR_MAX_WIDTH_PX = THEME_METRICS.inspectorMaxWidth;

export function GuiEditor(props: Parameters<typeof ResourceEditor>[0]) {
  const gui = props.guiDocument;
  const key = gui !== null && gui.type !== "blocked" ? guiEditorKey(props.snapshot.activeFile, gui) : "unavailable";
  return <div className="resource-editor-frame">
    {gui !== null && gui.type !== "blocked" && <ObjectDescription key={key} description={gui.document.description} />}
    <fieldset className="resource-editor-content"><ResourceEditor key={`${key}:${props.resetRevision}`} {...props} /></fieldset>
  </div>;
}

/** The open object's description, edited in place above its editor. */
function ObjectDescription({ description }: { description: string | null }) {
  return <DescriptionText className="gui-object-description" description={description} onCommit={(next) =>
    runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setDescription", item: { type: "object" }, description: next }))} />;
}

function ResourceEditor({
  guiDocument,
  workspaceLayout,
  onWorkspaceLayoutChange,
  sequenceSelection,
  setSequenceSelection
}: {
  guiDocument: GuiDocument | null;
  snapshot: AppStaticSnapshot;
  workspaceLayout: WorkspaceLayoutState;
  onWorkspaceLayoutChange: (layout: WorkspaceLayoutState) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
  resetRevision: number;
}) {
  const gui = guiDocument;
  if (gui === null) return <BlockedGui reason="GUI data is not available for this document." diagnostics={[]} />;
  switch (gui.type) {
    case "blocked": return <BlockedGui reason={gui.reason} diagnostics={gui.diagnostics} />;
    case "setup": return <SetupEditor document={gui.document} />;
    case "project": return <ProjectEditor document={gui.document} />;
    case "curve":
    case "gradient": return <LibraryEditor gui={gui} />;
    case "controller": return <ControllerEditor document={gui.document} />;
    case "patch": return <PatchEditor document={gui.document} />;
    case "inputDevice": return <InputDeviceEditor document={gui.document} />;
    case "fixture":
    case "layout": return <CompositionEditor gui={gui} />;
    case "sequence":
    case "console": return <GuiEditorInner
      gui={gui}
      workspaceLayout={workspaceLayout}
      onWorkspaceLayoutChange={onWorkspaceLayoutChange}
      sequenceSelection={sequenceSelection}
      setSequenceSelection={setSequenceSelection}
    />;
    default: {
      const unhandled: never = gui;
      return unhandled;
    }
  }
}

function GuiEditorInner({
  gui,
  workspaceLayout,
  onWorkspaceLayoutChange,
  sequenceSelection,
  setSequenceSelection
}: {
  gui: InspectedGuiDocument;
  workspaceLayout: WorkspaceLayoutState;
  onWorkspaceLayoutChange: (layout: WorkspaceLayoutState) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
}) {
  const restoreState = useAppStore((store) => store.restoreState);
  const sequenceRestore =
    gui.type === "sequence"
      ? restoreState?.sequenceViewports[objectViewKey(gui.document.sourceRef)]
      : undefined;
  const [selected, setSelected] = useState<GuiFocus>(null);
  const compositionGraphOpen = useAppStore((store) => store.compositionGraphEditing);
  const setCompositionGraphOpen = useAppStore((store) => store.setCompositionGraphEditing);
  const [automationClipChooser, setAutomationClipChooser] = useState<AutomationClipChooser>(null);
  const [activeMarkCollectionKey, setActiveMarkCollectionKey] = useState<string | null>(() =>
    gui.type === "sequence"
      ? sequenceRestore?.activeMarkCollectionKey ?? gui.document.markCollections[0]?.key ?? null
      : null
  );
  const [visibleMarkCollectionKeys, setVisibleMarkCollectionKeys] = useState<Set<string>>(() =>
    new Set(
      gui.type === "sequence" && sequenceRestore !== undefined
        ? sequenceRestore.visibleMarkCollectionKeys
        : gui.type === "sequence"
          ? gui.document.markCollections.map((collection) => collection.key)
          : []
    )
  );

  useEffect(() => {
    if (gui.type !== "sequence") return;
    return listenUiEvent("openLayerGraph", () => { setCompositionGraphOpen(true); });
  }, [gui.type, setCompositionGraphOpen]);

  useEffect(
    () => () => {
      if (gui.type === "sequence") setCompositionGraphOpen(false);
    },
    [gui.type, setCompositionGraphOpen]
  );

  return (
    <div
      className="gui-editor-shell"
      style={{
        gridTemplateColumns: workspaceLayout.inspectorCollapsed
          ? "var(--donder-gui-grid-template-collapsed)"
          : `minmax(0, 1fr) ${workspaceLayout.inspectorWidthPx}px`
      }}
    >
      {gui.type === "sequence" && (
        <SequenceEditor

          document={gui.document}
          selected={selected}
          setSelected={setSelected}
          compositionGraphOpen={compositionGraphOpen}
          setCompositionGraphOpen={setCompositionGraphOpen}
          sequenceSelection={sequenceSelection}
          setSequenceSelection={setSequenceSelection}
          automationClipChooser={automationClipChooser}
          setAutomationClipChooser={setAutomationClipChooser}
          activeMarkCollectionKey={activeMarkCollectionKey}
          setActiveMarkCollectionKey={setActiveMarkCollectionKey}
          visibleMarkCollectionKeys={visibleMarkCollectionKeys}
          setVisibleMarkCollectionKeys={setVisibleMarkCollectionKeys}
        />
      )}
      {gui.type === "console" && <ConsoleEditor document={gui.document} selected={selected} setSelected={setSelected} />}
      <div className={`gui-inspector-resize-shell ${workspaceLayout.inspectorCollapsed ? "collapsed" : ""}`}>
        <WorkspaceResizeHandle
          ariaLabel="Resize inspector"
          collapsed={workspaceLayout.inspectorCollapsed}
          direction="right"
          min={INSPECTOR_MIN_WIDTH_PX}
          max={INSPECTOR_MAX_WIDTH_PX}
          value={workspaceLayout.inspectorWidthPx}
          onChange={(update) => {
            onWorkspaceLayoutChange({
              ...workspaceLayout,
              inspectorCollapsed: update.collapsed,
              inspectorWidthPx: update.width
            });
          }}
        />
        {!workspaceLayout.inspectorCollapsed && (
          <GuiInspector
            gui={gui}
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
        )}
      </div>
    </div>
  );
}

function guiEditorKey(activeFile: string | null, gui: ReadyGuiDocument) {
  return JSON.stringify([activeFile, gui.type, "sourceRef" in gui.document ? guiObjectKey(gui.document.sourceRef) : gui.type === "controller" ? guiObjectKey(gui.document.controller.sourceRef) : gui.type === "inputDevice" ? guiObjectKey(gui.document.device.sourceRef) : [gui.document.path, gui.document.objectKey]]);
}

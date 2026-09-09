import { AddLightForm } from "./layout/AddLightForm";
import { ElementTreeEditor } from "./elements/ElementTreeEditor";
import { commands } from "../../api";
import { runGuiEditCommand } from "../../store";
import { useEffect, useState } from "react";

import type { GuiDocument, WorkspaceLayoutState } from "../../types";
import type { AppStaticSnapshot } from "../../store";

import type { AutomationClipChooser, GuiFocus, ReadyGuiDocument, SequenceSelection } from "./shared";

import { LayoutCanvas } from "./layout/LayoutCanvas";

import { FixtureCanvas } from "./fixture/FixtureCanvas";

import { GuiInspector } from "./GuiInspector";

import { BlockedGui } from "./BlockedGui";

import { SequenceEditor } from "./sequence/SequenceEditor";
import { useAppStore } from "../../store";
import { THEME_METRICS } from "../../theme";

import { handleSequencePlaybackShortcut, isSequenceTransportUnsupported } from "./sequence/SequenceTransportControls";

import { markSelectionConsumesKey } from "./sequence/sequenceSelection";
import { WorkspaceResizeHandle } from "../WorkspaceResizeHandle";
import { OPEN_LAYER_GRAPH_EVENT } from "../uiEvents";
import { SetupEditor } from "./setup/SetupEditor";
import { ProjectEditor } from "./project/ProjectEditor";
import { LibraryEditor } from "./library/LibraryEditor";
import { ControllerEditor } from "./controller/ControllerEditor";
import { PatchEditor } from "./patch/PatchEditor";
import { FixtureProfileEditor } from "./fixtureProfile/FixtureProfileEditor";

const INSPECTOR_MIN_WIDTH_PX = THEME_METRICS.inspectorMinWidth;
const INSPECTOR_MAX_WIDTH_PX = THEME_METRICS.inspectorMaxWidth;

export function GuiEditor(props: Parameters<typeof ResourceEditor>[0]) {
  const readOnly = props.snapshot.activeBuffer?.readOnly ?? false;
  const gui = props.guiDocument;
  const key = gui !== null && gui.type !== "blocked" ? guiEditorKey(props.snapshot.activeFile, gui) : "unavailable";
  return <div className="resource-editor-frame">
    {readOnly && <p className="resource-editor-readonly">Dependency source — read only</p>}
    <fieldset className="resource-editor-content" disabled={readOnly}><ResourceEditor key={`${key}:${props.resetRevision}`} {...props} /></fieldset>
  </div>;
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

  if (!gui) {
    return <BlockedGui reason="GUI data is not available for this document." diagnostics={[]} />;
  }
  if (gui.type === "blocked") {
    return <BlockedGui reason={gui.reason} diagnostics={gui.diagnostics} />;
  }
  if (gui.type === "elementTree") {
    return <ElementTreeEditor document={gui.document} onEdit={(edit) => runGuiEditCommand((request) => commands.applyElementTreeGuiEdit(request, edit))} />;
  }
  if (gui.type === "setup") {
    return <SetupEditor document={gui.document} />;
  }
  if (gui.type === "project") {
    return <ProjectEditor document={gui.document} />;
  }
  if (gui.type === "curve" || gui.type === "gradient") {
    return <LibraryEditor gui={gui} />;
  }
  if (gui.type === "controller") {
    return <ControllerEditor document={gui.document} />;
  }
  if (gui.type === "patch") {
    return <PatchEditor document={gui.document} />;
  }
  if (gui.type === "fixtureProfile") {
    return <FixtureProfileEditor document={gui.document} />;
  }

  return (
    <GuiEditorInner
      gui={gui}
      workspaceLayout={workspaceLayout}
      onWorkspaceLayoutChange={onWorkspaceLayoutChange}
      sequenceSelection={sequenceSelection}
      setSequenceSelection={setSequenceSelection}
    />
  );
}

function GuiEditorInner({
  gui,
  workspaceLayout,
  onWorkspaceLayoutChange,
  sequenceSelection,
  setSequenceSelection
}: {
  gui: ReadyGuiDocument;
  workspaceLayout: WorkspaceLayoutState;
  onWorkspaceLayoutChange: (layout: WorkspaceLayoutState) => void;
  sequenceSelection: SequenceSelection;
  setSequenceSelection: (selection: SequenceSelection) => void;
}) {
  const restoreState = useAppStore((store) => store.restoreState);
  const sequenceRestore =
    gui.type === "sequence"
      ? restoreState?.sequenceViewports[`${gui.document.path}::${gui.document.objectKey}`]
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
    const openLayerGraph = () => {
      setCompositionGraphOpen(true);
    };
    window.addEventListener(OPEN_LAYER_GRAPH_EVENT, openLayerGraph);
    return () => {
      window.removeEventListener(OPEN_LAYER_GRAPH_EVENT, openLayerGraph);
    };
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
          ? "var(--dawn-gui-grid-template-collapsed)"
          : `minmax(0, 1fr) ${workspaceLayout.inspectorWidthPx}px`
      }}
      onKeyDownCapture={(event) => {
        if (gui.type === "sequence" && !markSelectionConsumesKey(selected, event.key)) {
          const audioTransport = useAppStore.getState().snapshot?.audioTransport;
          if (audioTransport === undefined) return;
          handleSequencePlaybackShortcut(
            event,
            gui.document,
            audioTransport,
            isSequenceTransportUnsupported(gui.document, audioTransport)
          );
        }
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
      {gui.type === "preview" && <div className="layout-authoring">
        <aside className="layout-hierarchy"><details><summary>Add light</summary><AddLightForm document={gui.document} /></details><ElementTreeEditor document={gui.document.hierarchy} onEdit={(edit) => runGuiEditCommand((request) => commands.applyPreviewGuiEdit(request, { type: "editElements", edit }))} /></aside>
        <LayoutCanvas document={gui.document} selected={selected} setSelected={setSelected} />
      </div>}
      {gui.type === "prop" && (
        <FixtureCanvas document={gui.document} selected={selected} setSelected={setSelected} />
      )}
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
  switch (gui.type) {
    case "project":
    case "sequence":
    case "preview":
    case "elementTree":
    case "setup":
    case "curve":
    case "gradient":
    case "controller":
    case "patch":
    case "fixtureProfile":
      return `${activeFile ?? ""}:${gui.type}:${gui.document.path}:${gui.document.objectKey}`;
    case "prop":
      return `${activeFile ?? ""}:${gui.type}:${gui.document.path}:${gui.document.fixture.objectKey}`;
  }
}

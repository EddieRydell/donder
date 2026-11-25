import { guiObjectKey, objectViewKey } from "../../workspace/guiIdentity";
import { CompositionEditor } from "./composition/CompositionEditor";

import type { GuiDocument, WorkspaceLayoutState } from "../../types";
import type { AppStaticSnapshot } from "../../store";

import type { InspectedGuiDocument, ReadyGuiDocument } from "./shared";
import { useOpenDocumentUi } from "./documentUi";



import { GuiInspector } from "./GuiInspector";

import { BlockedGui } from "./BlockedGui";

import { SequenceEditor } from "./sequence/SequenceEditor";
import { useAppStore } from "../../store";
import { describe } from "./edits";
import { DescriptionText } from "./DescriptionField";
import { OBJECT } from "./problems";
import { THEME_METRICS } from "../../theme";


import { WorkspaceResizeHandle } from "../WorkspaceResizeHandle";
import { ProjectEditor } from "./project/ProjectEditor";
import { LibraryEditor } from "./library/LibraryEditor";
import { ControllerEditor } from "./controller/ControllerEditor";
import { InputDeviceEditor } from "./inputDevice/InputDeviceEditor";
import { ConsoleEditor } from "./console/ConsoleEditor";

const INSPECTOR_MIN_WIDTH_PX = THEME_METRICS.inspectorMinWidth;
const INSPECTOR_MAX_WIDTH_PX = THEME_METRICS.inspectorMaxWidth;

type GuiEditorProps = {
  guiDocument: GuiDocument | null;
  snapshot: AppStaticSnapshot;
  workspaceLayout: WorkspaceLayoutState;
  onWorkspaceLayoutChange: (layout: WorkspaceLayoutState) => void;
  resetRevision: number;
};

export function GuiEditor(props: GuiEditorProps) {
  const gui = props.guiDocument;
  const key = gui !== null && gui.type !== "blocked" ? guiEditorKey(props.snapshot.activeFile, gui) : "unavailable";
  const editorKey = `${key}:${props.resetRevision}`;
  return <div className="resource-editor-frame">
    {gui !== null && gui.type !== "blocked" && <ObjectDescription key={key} description={gui.document.description} />}
    <fieldset className="resource-editor-content"><ResourceEditor key={editorKey} editorKey={editorKey} {...props} /></fieldset>
  </div>;
}

/** The open object's description, edited in place above its editor. */
function ObjectDescription({ description }: { description: string | null }) {
  return <DescriptionText className="gui-object-description" description={description} member={OBJECT} onCommit={(next) => describe(OBJECT, next)} />;
}

function ResourceEditor({
  guiDocument,
  editorKey,
  workspaceLayout,
  onWorkspaceLayoutChange
}: GuiEditorProps & { editorKey: string }) {
  const gui = guiDocument;
  if (gui === null) return <BlockedGui reason="GUI data is not available for this document." diagnostics={[]} />;
  switch (gui.type) {
    case "blocked": return <BlockedGui reason={gui.reason} diagnostics={gui.diagnostics} />;
    case "project": return <ProjectEditor document={gui.document} />;
    case "curve":
    case "gradient": return <LibraryEditor gui={gui} />;
    case "controller": return <ControllerEditor document={gui.document} />;
    case "inputDevice": return <InputDeviceEditor document={gui.document} />;
    case "fixture":
    case "layout": return <CompositionEditor gui={gui} />;
    case "sequence":
    case "console": return <GuiEditorInner
      gui={gui}
      editorKey={editorKey}
      workspaceLayout={workspaceLayout}
      onWorkspaceLayoutChange={onWorkspaceLayoutChange}
    />;
    default: {
      const unhandled: never = gui;
      return unhandled;
    }
  }
}

function GuiEditorInner({
  gui,
  editorKey,
  workspaceLayout,
  onWorkspaceLayoutChange
}: {
  gui: InspectedGuiDocument;
  editorKey: string;
  workspaceLayout: WorkspaceLayoutState;
  onWorkspaceLayoutChange: (layout: WorkspaceLayoutState) => void;
}) {
  const restoreState = useAppStore((store) => store.restoreState);
  const open = useOpenDocumentUi(editorKey, () => {
    if (gui.type !== "sequence") return {};
    const restored = restoreState?.sequenceViewports[objectViewKey(gui.document.sourceRef)];
    const names = gui.document.markCollections.map((collection) => collection.name);
    return {
      activeMarks: restored?.activeMarkCollectionKey ?? names[0] ?? null,
      visibleMarks: new Set(restored?.visibleMarkCollectionKeys ?? names)
    };
  });
  if (!open) return null;
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
        <SequenceEditor document={gui.document} />
      )}
      {gui.type === "console" && <ConsoleEditor document={gui.document} />}
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
          <GuiInspector gui={gui} />
        )}
      </div>
    </div>
  );
}

function guiEditorKey(activeFile: string | null, gui: ReadyGuiDocument) {
  return JSON.stringify([activeFile, gui.type, "sourceRef" in gui.document ? guiObjectKey(gui.document.sourceRef) : gui.type === "controller" ? guiObjectKey(gui.document.controller.sourceRef) : gui.type === "inputDevice" ? guiObjectKey(gui.document.device.sourceRef) : [gui.document.path, gui.document.objectKey]]);
}

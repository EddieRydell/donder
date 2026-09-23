import { useCallback, useEffect, useState } from "react";
import { commands } from "../api";
import { installGlobalShortcuts } from "../commandRegistry";
import { runSnapshotCommand, subscribeToSnapshots, useAppStore, useStaticAppSnapshot, type AppStaticSnapshot } from "../store";
import type { WorkspaceLayoutState } from "../types";
import { EditorPane } from "./EditorPane";
import { NewProjectDialog } from "./NewProjectDialog";
import { NewSequenceDialog } from "./NewSequenceDialog";
import { SettingsDialog } from "./SettingsDialog";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";
import { WorkspaceResizeHandle } from "./WorkspaceResizeHandle";
import { THEME_METRICS } from "../theme";
import { CommandOverlays } from "../workspace/CommandOverlays";
import { WorkspaceSidebar } from "../workspace/WorkspaceSidebar";
import { openProjectDialog, runWorkspaceTransition, useTransitionStore } from "../workspaceTransitions";
import { UnsavedChangesDialog } from "./UnsavedChangesDialog";
import { listen } from "@tauri-apps/api/event";
import { scheduleViewStateSave } from "../viewStatePersistence";

const PROJECT_TREE_MIN_WIDTH_PX = THEME_METRICS.projectPanelMinWidth;
const PROJECT_TREE_MAX_WIDTH_PX = THEME_METRICS.projectPanelMaxWidth;

export function App() {
  const snapshot = useStaticAppSnapshot();
  const error = useAppStore((store) => store.error);
  const hydrate = useAppStore((store) => store.hydrate);
  const compositionGraphEditing = useAppStore((store) => store.compositionGraphEditing);

  useEffect(() => {
    void hydrate();
    const disposeShortcuts = installGlobalShortcuts();
    let disposeEvents: (() => void) | undefined;
    void subscribeToSnapshots().then((dispose) => {
      disposeEvents = dispose;
    });
    return () => {
      disposeShortcuts();
      disposeEvents?.();
    };
  }, [hydrate]);

  useEffect(() => {
    const close = listen("close_requested", () => { void runWorkspaceTransition({ type: "closeApplication" }); });
    const onFocus = () => { void runSnapshotCommand(commands.reconcileExternalFiles); };
    window.addEventListener("focus", onFocus);
    return () => {
      void close.then((unlisten) => { unlisten(); });
      window.removeEventListener("focus", onFocus);
    };
  }, []);

  if (!snapshot) {
    return <div className="app-loading">Donder</div>;
  }

  return (
    <div className="app-shell">
      <TitleBar />
      <div className="alert-stack">
        {error !== null && error !== "" && <div className="error-strip">{error}</div>}
        {!compositionGraphEditing && snapshot.renderError !== null && snapshot.renderError !== "" && (
          <div className="error-strip">{snapshot.renderError}</div>
        )}
        {snapshot.previewError !== null && snapshot.previewError !== "" && (
          <div className="error-strip">{snapshot.previewError}</div>
        )}
      </div>
      <WorkspaceMain snapshot={snapshot} />
      <StatusBar snapshot={snapshot} />
      <NewProjectDialog />
      <NewSequenceDialog />
      <SettingsDialog />
      <UnsavedChangesDialog />
      <CommandOverlays />
      {snapshot.projectRoot === null && (
        <div className="empty-project">
          <button onClick={() => void openProjectDialog()}>Open Project...</button>
        </div>
      )}
    </div>
  );
}

function WorkspaceMain({ snapshot }: { snapshot: AppStaticSnapshot }) {
  const transitionInProgress = useTransitionStore((store) => store.inProgress);
  const [workspaceLayout, setWorkspaceLayout] = useState<WorkspaceLayoutState>(() => snapshot.workspaceLayout);
  const [serverLayout, setServerLayout] = useState<WorkspaceLayoutState>(() => snapshot.workspaceLayout);

  if (!sameWorkspaceLayout(serverLayout, snapshot.workspaceLayout)) {
    setServerLayout(snapshot.workspaceLayout);
    setWorkspaceLayout(snapshot.workspaceLayout);
  }

  const updateWorkspaceLayout = useCallback((next: WorkspaceLayoutState, immediate = false) => {
    setWorkspaceLayout(next);
    scheduleViewStateSave("layout", async () => {
      useAppStore.getState().setSnapshot(await commands.saveWorkspaceLayoutState(next));
    }, (error) => { useAppStore.getState().setError(String(error)); }, immediate);
  }, []);

  const layout = workspaceLayout;

  return (
    <main className="workbench" inert={transitionInProgress}>
      <div
        className={`project-panel-shell ${layout.sidebarCollapsed ? "collapsed" : ""}`}
        style={{ width: layout.sidebarCollapsed ? undefined : layout.sidebarWidthPx }}
      >
        <WorkspaceSidebar snapshot={snapshot} layout={layout} onLayoutChange={updateWorkspaceLayout} />
        <WorkspaceResizeHandle
          ariaLabel="Resize side bar"
          collapsed={layout.sidebarCollapsed}
          direction="left"
          min={PROJECT_TREE_MIN_WIDTH_PX}
          max={PROJECT_TREE_MAX_WIDTH_PX}
          value={layout.sidebarWidthPx}
          onChange={(update) => {
            updateWorkspaceLayout({
              ...layout,
              sidebarCollapsed: update.collapsed,
              sidebarWidthPx: update.width
            });
          }}
        />
      </div>
      <EditorPane snapshot={snapshot} workspaceLayout={layout} onWorkspaceLayoutChange={updateWorkspaceLayout} />
    </main>
  );
}

function sameWorkspaceLayout(left: WorkspaceLayoutState, right: WorkspaceLayoutState) {
  return left.sidebarWidthPx === right.sidebarWidthPx
    && left.sidebarCollapsed === right.sidebarCollapsed
    && left.activeSidebarView === right.activeSidebarView
    && left.inspectorWidthPx === right.inspectorWidthPx
    && left.inspectorCollapsed === right.inspectorCollapsed;
}

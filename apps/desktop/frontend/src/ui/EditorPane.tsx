import { SequenceEditorHostProvider } from "../editor/host";
import { desktopSequenceEditorHost } from "../editor/desktopHost";
export function EditorPane(props: Parameters<typeof EditorPaneContent>[0]) { return <SequenceEditorHostProvider host={desktopSequenceEditorHost}><EditorPaneContent {...props} /></SequenceEditorHostProvider>; }
import { RefreshCw, Save, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { commands } from "../api";
import { SequenceAudioSync, sequenceAudioKey } from "../sequenceAudioSync";
import type { PersistedEditorViewState, SequenceSelection, WorkspaceLayoutState } from "../types";
import { FOCUS_SIDEBAR_EVENT } from "../commandRegistry";
import { effectiveEditorViewMode } from "../editorViewMode";
import { closeInlineEditor, runSnapshotCommand, useAppStore, type AppStaticSnapshot } from "../store";
import { runWorkspaceTransition } from "../workspaceTransitions";
import { GuiEditor } from "./gui/GuiEditor";
import { ReadOnlySourceNotice } from "./ReadOnlySourceNotice";
import { TextEditor } from "./TextEditor";
import { reconcileSequenceSelection } from "./gui/sequence/sequenceSelection";
import { SequenceTransportControls } from "./gui/sequence/SequenceTransportControls";
import { THEME_METRICS } from "../theme";
import { scheduleViewStateSave } from "../viewStatePersistence";
import { sameGuiDocument } from "../snapshotState";
import { resolveDocumentSyncFailure } from "../store";
import { NAVIGATE_TO_TEXT_EVENT, navigateToText, type TextNavigation } from "../workspace/navigation";
import { displayedProjectHealth } from "../workspace/helpers";

type PathSelection = { path: string | null; resetRevision: number; selection: SequenceSelection | null };

const sequenceAudioSync = new SequenceAudioSync(async (request) => {
  const snapshot = await runSnapshotCommand(() => request === null ? commands.unloadAudio() : commands.loadSequenceAudio(request));
  return request === null || snapshot.projectRevision === request.projectRevision;
});

function EditorPaneContent({
  snapshot,
  workspaceLayout,
  onWorkspaceLayoutChange
}: {
  snapshot: AppStaticSnapshot;
  workspaceLayout: WorkspaceLayoutState;
  onWorkspaceLayoutChange: (layout: WorkspaceLayoutState) => void;
}) {
  const guiDocument = useAppStore((store) => store.guiDocument);
  const guiParents = useAppStore((store) => store.guiParents);
  const inlineEditorOpen = guiParents.length > 0;
  const [savingInline, setSavingInline] = useState(false);
  const saveAndCloseFixture = async () => {
    const origin = useAppStore.getState();
    setSavingInline(true);
    try {
      await runSnapshotCommand(commands.saveAll);
      const current = useAppStore.getState();
      if (current.snapshot?.projectEpoch === origin.snapshot?.projectEpoch && sameGuiDocument(current.guiRequest, origin.guiRequest)) closeInlineEditor();
    }
    catch (error) { useAppStore.getState().setError(String(error)); }
    finally { setSavingInline(false); }
  };
  const parentDocument = inlineEditorOpen ? guiParents[0]?.document ?? null : guiDocument;
  const guiResetRevision = useAppStore((store) => store.guiResetRevision);
  const localText = useAppStore((store) => store.localText);
  const failedDocumentSync = useAppStore((store) => store.failedDocumentSync);
  const restoreState = useAppStore((store) => store.restoreState);
  const setGuiDocument = useAppStore((store) => store.setGuiDocument);
  const activeGuiRequest = useAppStore((store) => store.guiRequest);
  const guiDocumentRevision = useAppStore((store) => store.guiDocumentRevision);
  const guiEditPending = useAppStore((store) => store.guiEditPending);
  const projectionPending = activeGuiRequest !== null && guiDocumentRevision !== activeGuiRequest.projectRevision;
  const interactionPending = projectionPending || guiEditPending;
  const setLocalText = useAppStore((store) => store.setLocalText);
  const [pathSelection, setPathSelection] = useState<PathSelection>({ path: null, resetRevision: 0, selection: null });
  const [pendingTextNavigation, setPendingTextNavigation] = useState<TextNavigation | null>(null);
  const activeBuffer = snapshot.activeBuffer;
  const activePath = activeBuffer?.path ?? null;
  const activeSyntax = activeBuffer?.syntax ?? "plain";
  const activeTabPath = activeBuffer?.path ?? snapshot.activeFile;
  const viewMode = effectiveEditorViewMode(snapshot);
  const activeExternalState = activeBuffer?.externalState ?? "current";
  const activeConflicted = activeExternalState !== "current";
  const activeReadOnly = activeBuffer?.readOnly ?? false;
  const nextGuiPath = activeGuiRequest?.path ?? null;
  const nextGuiView = activeGuiRequest?.view ?? null;
  const nextGuiObjectKey = activeGuiRequest?.objectKey ?? null;
  const activeSequenceDocument =
    viewMode === "gui" && parentDocument?.type === "sequence" ? parentDocument.document : null;
  const editableSequenceDocument = activeSequenceDocument;
  const activeSequenceAudio = activeSequenceDocument?.audio ?? null;
  const activeSequenceAudioKey =
    nextGuiPath !== null && nextGuiView === "sequence" && activeSequenceDocument !== null
      ? sequenceAudioKey(snapshot.projectEpoch, activeSequenceDocument.sourceRef, activeSequenceAudio, activeSequenceDocument.durationSeconds)
      : null;
  const sequenceSelection = reconcileSequenceSelection(activeSequenceDocument,
    pathSelection.path === activePath && pathSelection.resetRevision === guiResetRevision ? pathSelection.selection : null);
  const setSequenceSelection = useCallback(
    (selection: SequenceSelection | null) => {
      setPathSelection({ path: activePath, resetRevision: guiResetRevision, selection });
    },
    [activePath, guiResetRevision]
  );

  useEffect(() => {
    const onNavigate = (event: Event) => {
      setPendingTextNavigation((event as CustomEvent<TextNavigation>).detail);
    };
    window.addEventListener(NAVIGATE_TO_TEXT_EVENT, onNavigate);
    return () => { window.removeEventListener(NAVIGATE_TO_TEXT_EVENT, onNavigate); };
  }, []);

  useEffect(() => {
    if (activeGuiRequest === null) {
      setGuiDocument(null);
      return;
    }
    if (useAppStore.getState().guiDocumentRevision === activeGuiRequest.projectRevision) return;
    let cancelled = false;
    const isCurrent = () => !cancelled
      && useAppStore.getState().guiRequest === activeGuiRequest
      && useAppStore.getState().snapshot?.projectRevision === snapshot.projectRevision;
    commands
      .getGuiDocument(activeGuiRequest)
      .then((result) => {
        if (isCurrent() && result.projectRevision === activeGuiRequest.projectRevision
          && result.request.projectRevision === activeGuiRequest.projectRevision
          && sameGuiDocument(result.request, activeGuiRequest)) setGuiDocument(result.document);
      })
      .catch((error: unknown) => {
        if (isCurrent() && useAppStore.getState().guiDocumentRevision !== activeGuiRequest.projectRevision) {
          setGuiDocument({ type: "blocked", reason: String(error), diagnostics: [] });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [
    activeGuiRequest,
    setGuiDocument,
    snapshot.projectRevision
  ]);

  useEffect(() => {
    if (inlineEditorOpen) return;
    if (viewMode === "gui" && nextGuiView === "sequence" && projectionPending) return;
    if (activeGuiRequest === null || activeSequenceAudioKey === null || nextGuiPath === null || nextGuiView !== "sequence") {
      void sequenceAudioSync.synchronize(null).catch(() => {});
      return;
    }
    void sequenceAudioSync.synchronize({ key: activeSequenceAudioKey, request: activeGuiRequest }).catch(() => {});
  }, [activeGuiRequest, activeSequenceAudioKey, inlineEditorOpen, nextGuiObjectKey, nextGuiPath, nextGuiView, projectionPending, viewMode]);

  useEffect(() => {
    return () => {
      void sequenceAudioSync.synchronize(null).catch(() => {});
    };
  }, []);

  if (snapshot.tabs.length === 0) {
    return (
      <section className="editor-shell empty-editor">
        <span>{snapshot.projectRoot !== null ? "Open a Donder file from the project tree." : "Open a project to start."}</span>
      </section>
    );
  }

  return (
    <section className={`editor-shell ${editableSequenceDocument !== null ? "has-editor-toolbar" : ""} ${activeConflicted || failedDocumentSync !== null ? "has-conflict-banner" : ""}`}>
      <div className="tab-strip">
        {snapshot.tabs.map((tab) => (
          <div
            key={tab.path}
            className={`tab ${tab.path === activeTabPath ? "active" : ""}`}
          >
            <button
              type="button"
              className="tab-select"
              aria-current={tab.path === activeTabPath ? "page" : undefined}
              onClick={() => void runSnapshotCommand(() => commands.setActiveFile(tab.path))}
            >
              <span>{tab.name}</span>
              <span className={tab.dirty ? "dirty-dot unsaved" : "dirty-dot"} aria-hidden="true" />
              {tab.externalState !== "current" && <span className="conflict-dot" />}
            </button>
            <button
              type="button"
              className="tab-close"
              aria-label={`Close ${tab.name}`}
              onClick={(event) => {
                event.stopPropagation();
                void runWorkspaceTransition({ type: "closeFile", path: tab.path });
              }}
            >
              <X size={THEME_METRICS.iconSizeSmall} />
            </button>
          </div>
        ))}
      </div>
      {displayedProjectHealth(snapshot) === "invalid" && (
        <div className="project-invalid-banner">
          <span>The project has errors. Fix them in Text to use GUI editing.</span>
          <button
            type="button"
            onClick={() => {
              window.dispatchEvent(new CustomEvent(FOCUS_SIDEBAR_EVENT, { detail: "problems" }));
            }}
          >
            Open Problems
          </button>
          {activePath !== null && (
            <button
              type="button"
              onClick={() => {
                const diagnostic = snapshot.diagnostics.find((item) =>
                  item.path === activePath
                  || item.path.endsWith(`/${activePath}`)
                  || item.path.endsWith(`\\${activePath}`)
                );
                void navigateToText(activePath, diagnostic?.range ?? null);
              }}
            >
              Edit Text
            </button>
          )}
        </div>
      )}
      {editableSequenceDocument !== null && (
        <div className="editor-toolbar" inert={interactionPending || inlineEditorOpen}>
          <SequenceTransportControls
            document={editableSequenceDocument}
            previewOpen={snapshot.previewOpen}
          />
        </div>
      )}
      {failedDocumentSync !== null && (
        <div className="conflict-banner">
          <span>Your latest text has not been accepted because the document changed.</span>
          <button type="button" onClick={() => void resolveDocumentSyncFailure(false)}>Discard Pending Text</button>
          <button type="button" onClick={() => void resolveDocumentSyncFailure(true)}>Keep My Text</button>
        </div>
      )}
      {activeConflicted && failedDocumentSync === null && (
        <div className="conflict-banner">
          <span>
            {activeExternalState === "deletedOnDisk"
              ? "This file was deleted on disk."
              : "This file changed on disk."}
          </span>
          <button type="button" onClick={() => activeBuffer !== null && void runSnapshotCommand(() => commands.resolveExternalConflict(snapshot.projectEpoch, activeBuffer.path, activeBuffer.documentRevision, "reload"))}>
            <RefreshCw size={THEME_METRICS.iconSizeSmall} />
            Reload from Disk
          </button>
          <button type="button" onClick={() => activeBuffer !== null && void runSnapshotCommand(() => commands.resolveExternalConflict(snapshot.projectEpoch, activeBuffer.path, activeBuffer.documentRevision, "keepWorkingCopy"))}>
            <Save size={THEME_METRICS.iconSizeSmall} />
            Keep Mine
          </button>
        </div>
      )}
      {viewMode === "gui" ? (
        <><div className="gui-projection" inert={interactionPending || inlineEditorOpen} aria-busy={interactionPending}>
          <GuiEditor
            guiDocument={parentDocument}
            snapshot={snapshot}
            workspaceLayout={workspaceLayout}
            onWorkspaceLayoutChange={onWorkspaceLayoutChange}
            sequenceSelection={sequenceSelection}
            setSequenceSelection={setSequenceSelection}
            resetRevision={guiResetRevision}
          />
        </div>
        <Dialog.Root open={inlineEditorOpen} onOpenChange={(open) => { if (!open && !savingInline) closeInlineEditor(); }}>
          <Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog-content resource-editor-dialog" aria-describedby={undefined}>
            <header className="resource-editor-dialog-header"><Dialog.Title>{activeGuiRequest?.objectKey}</Dialog.Title><>{guiDocument?.type === "fixture" ? <button type="button" disabled={interactionPending || savingInline} onClick={() => { void saveAndCloseFixture(); }}>{savingInline ? "Saving..." : "Save and close"}</button> : <Dialog.Close asChild><button type="button" disabled={guiEditPending}>Close</button></Dialog.Close>}</></header>
            <div className="gui-projection" inert={interactionPending || savingInline} aria-busy={interactionPending || savingInline}>
              <GuiEditor
                guiDocument={guiDocument}
                snapshot={snapshot}
                workspaceLayout={workspaceLayout}
                onWorkspaceLayoutChange={onWorkspaceLayoutChange}
                sequenceSelection={sequenceSelection}
                setSequenceSelection={setSequenceSelection}
                resetRevision={guiResetRevision}
              />
            </div>
          </Dialog.Content></Dialog.Portal>
        </Dialog.Root></>
      ) : (
        <div className={`resource-editor-frame ${activeConflicted ? "conflicted" : ""}`}>
          {activeReadOnly && activeBuffer !== null && <ReadOnlySourceNotice name={activeBuffer.name} />}
          {activePath !== null && snapshot.projectRoot !== null && (
            <TextEditor
              root={snapshot.projectRoot}
              path={activePath}
              openPaths={snapshot.tabs.map((tab) => tab.path)}
              syntax={activeSyntax}
              text={localText}
              readOnly={activeConflicted || activeReadOnly}
              restore={restoreState?.editorStates[activePath]}
              navigation={pendingTextNavigation}
              onChange={setLocalText}
              onViewState={scheduleEditorViewStateSave}
            />
          )}
        </div>
      )}
    </section>
  );
}

function scheduleEditorViewStateSave(path: string, state: PersistedEditorViewState) {
  scheduleViewStateSave(JSON.stringify(["editor", path]), () => commands.saveEditorViewState({ path, state }),
    (error) => { useAppStore.getState().setError(String(error)); });
}

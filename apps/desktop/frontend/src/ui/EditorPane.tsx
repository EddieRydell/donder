import { AlertTriangle, FileText, FolderOpen, ListX, RefreshCw, Save, Undo2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { commands } from "../api";
import { SequenceAudioSync, sequenceAudioKey } from "../sequenceAudioSync";
import type { PersistedEditorViewState, SequenceSelection, WorkspaceLayoutState } from "../types";
import { runCommand } from "../commandRegistry";
import { effectiveEditorViewMode } from "../editorViewMode";
import { closeInlineEditor, runSnapshotCommand, useAppStore, type AppStaticSnapshot } from "../store";
import { GuiEditor } from "./gui/GuiEditor";
import { TextEditor } from "./TextEditor";
import { reconcileSequenceSelection } from "./gui/sequence/sequenceSelection";
import { SequenceTransportControls } from "./gui/sequence/SequenceTransportControls";
import { THEME_METRICS } from "../theme";
import { scheduleViewStateSave } from "../viewStatePersistence";
import { sameGuiDocument } from "../snapshotState";
import { guiRequestKind } from "../workspace/guiIdentity";
import { resolveDocumentSyncFailure } from "../store";
import { navigateToText, type TextNavigation } from "../workspace/navigation";
import { listenUiEvent } from "./uiEvents";
import { displayedProjectHealth, sameWorkspacePath } from "../workspace/helpers";
import { Button, Dialog, Tip } from "./kit";
import { OverlayPortal } from "./OverlayPortal";
import { EditorTabStrip } from "./EditorTabStrip";

type PathSelection = { path: string | null; resetRevision: number; selection: SequenceSelection | null };

const sequenceAudioSync = new SequenceAudioSync(async (request) => {
  const snapshot = await runSnapshotCommand(() => request === null ? commands.unloadAudio() : commands.loadSequenceAudio(request));
  return request === null || snapshot.projectRevision === request.projectRevision;
});

export function EditorPane({
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
  const [inlinePortal, setInlinePortal] = useState<HTMLDivElement | null>(null);
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
  const failedDocumentSync = useAppStore((store) => store.failedDocumentSync);
  const restoreState = useAppStore((store) => store.restoreState);
  const setGuiDocument = useAppStore((store) => store.setGuiDocument);
  const activeGuiRequest = useAppStore((store) => store.guiRequest);
  const guiDocumentRevision = useAppStore((store) => store.guiDocumentRevision);
  const guiEditPending = useAppStore((store) => store.guiEditPending);
  const projectionPending = activeGuiRequest !== null && guiDocumentRevision !== activeGuiRequest.projectRevision;
  const interactionPending = projectionPending || guiEditPending;
  const [pathSelection, setPathSelection] = useState<PathSelection>({ path: null, resetRevision: 0, selection: null });
  const [pendingTextNavigation, setPendingTextNavigation] = useState<TextNavigation | null>(null);
  const activeBuffer = snapshot.activeBuffer;
  const activePath = activeBuffer?.path ?? null;
  const activeSyntax = activeBuffer?.syntax ?? "plain";
  const activeTabPath = activeBuffer?.path ?? snapshot.activeFile;
  const viewMode = effectiveEditorViewMode(snapshot);
  const activeExternalState = activeBuffer?.externalState ?? "current";
  const activeConflicted = activeExternalState !== "current";
  const nextGuiPath = activeGuiRequest?.path ?? null;
    const nextGuiView = guiRequestKind(activeGuiRequest, snapshot.activeDocumentDescriptor);
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

  useEffect(() => listenUiEvent("navigateToText", setPendingTextNavigation), []);

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
        {snapshot.projectRoot !== null
          ? <Button variant="ghost" icon={FileText} tip="Quick Open" command="workbench.quickOpen" onClick={() => { runCommand("workbench.quickOpen"); }}>Open file</Button>
          : <Button variant="ghost" icon={FolderOpen} tip="Open Project" command="file.openProject" onClick={() => { runCommand("file.openProject"); }}>Open project</Button>}
      </section>
    );
  }

  return (
    <section className={`editor-shell ${editableSequenceDocument !== null ? "has-editor-toolbar" : ""} ${activeConflicted || failedDocumentSync !== null ? "has-conflict-banner" : ""}`}>
      <EditorTabStrip tabs={snapshot.tabs} activePath={activeTabPath} />
      {displayedProjectHealth(snapshot) === "invalid" && (
        <div className="project-invalid-banner">
          <Tip label="GUI editing needs a project without errors."><span><AlertTriangle size={THEME_METRICS.iconSizeSmall} aria-hidden /> Project has errors</span></Tip>
          <Button icon={ListX} onClick={() => { runCommand("view.focusProblems"); }}>Problems</Button>
          {activePath !== null && (
            <Button
              icon={FileText}
              onClick={() => {
                const diagnostic = snapshot.diagnostics.find((item) =>
                  sameWorkspacePath(item.path, activePath, snapshot.projectRoot)
                );
                void navigateToText(activePath, diagnostic?.range ?? null);
              }}
            >
              Edit text
            </Button>
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
          <Tip label="Your latest text was not accepted because the document changed."><span>Text not applied</span></Tip>
          <Button icon={Undo2} onClick={() => void resolveDocumentSyncFailure(false)}>Discard</Button>
          <Button icon={Save} onClick={() => void resolveDocumentSyncFailure(true)}>Keep mine</Button>
        </div>
      )}
      {activeConflicted && failedDocumentSync === null && (
        <div className="conflict-banner">
          <span>{activeExternalState === "deletedOnDisk" ? "Deleted on disk" : "Changed on disk"}</span>
          <Button icon={RefreshCw} onClick={() => activeBuffer !== null && void runSnapshotCommand(() => commands.resolveExternalConflict(snapshot.projectEpoch, activeBuffer.path, activeBuffer.documentRevision, "reload"))}>Reload</Button>
          <Button icon={Save} onClick={() => activeBuffer !== null && void runSnapshotCommand(() => commands.resolveExternalConflict(snapshot.projectEpoch, activeBuffer.path, activeBuffer.documentRevision, "keepWorkingCopy"))}>Keep mine</Button>
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
        <Dialog
          ref={setInlinePortal}
          open={inlineEditorOpen}
          onOpenChange={(open) => { if (!open && !savingInline && !guiEditPending) closeInlineEditor(); }}
          title={activeGuiRequest?.objectKey ?? ""}
          size="full"
          {...(guiDocument?.type === "fixture" ? { actions: <Button variant="primary" icon={Save} disabled={interactionPending || savingInline} onClick={() => { void saveAndCloseFixture(); }}>{savingInline ? "Saving" : "Save and close"}</Button> } : {})}
        >
          <OverlayPortal value={inlinePortal}><div className="gui-projection" inert={interactionPending || savingInline} aria-busy={interactionPending || savingInline}>
              <GuiEditor
                guiDocument={guiDocument}
                snapshot={snapshot}
                workspaceLayout={workspaceLayout}
                onWorkspaceLayoutChange={onWorkspaceLayoutChange}
                sequenceSelection={sequenceSelection}
                setSequenceSelection={setSequenceSelection}
                resetRevision={guiResetRevision}
              />
            </div></OverlayPortal>
        </Dialog></>
      ) : (
        <div className={`resource-editor-frame ${activeConflicted ? "conflicted" : ""}`}>
          {activePath !== null && snapshot.projectRoot !== null && (
            <TextEditor
              root={snapshot.projectRoot}
              epoch={snapshot.projectEpoch}
              path={activePath}
              revision={activeBuffer?.documentRevision ?? 0}
              openPaths={snapshot.tabs.map((tab) => tab.path)}
              syntax={activeSyntax}
              readOnly={activeConflicted}
              restore={restoreState?.editorStates[activePath]}
              navigation={pendingTextNavigation}
              onViewState={scheduleEditorViewStateSave}
            />
          )}
        </div>
      )}
    </section>
  );
}

function scheduleEditorViewStateSave(path: string, state: PersistedEditorViewState) {
  scheduleViewStateSave(JSON.stringify(["editor", path]), () => commands.saveViewState({ type: "editor", state: { path, state } }),
    (error) => { useAppStore.getState().setError(String(error)); });
}

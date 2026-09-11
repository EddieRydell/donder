import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { cpp } from "@codemirror/lang-cpp";
import { yaml } from "@codemirror/lang-yaml";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { linter, setDiagnostics, type Diagnostic } from "@codemirror/lint";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, ViewUpdate } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { RefreshCw, Save, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type PointerEvent } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { commands } from "../api";
import { SequenceAudioSync, sequenceAudioKey } from "../sequenceAudioSync";
import type { AppSnapshot, PersistedEditorViewState, ProjectDiagnostic, SequenceSelection, TextRange, WorkspaceLayoutState } from "../types";
import { commandRegistry, FOCUS_SIDEBAR_EVENT } from "../commandRegistry";
import { effectiveEditorViewMode } from "../editorViewMode";
import { closeInlineEditor, runSnapshotCommand, useAppStore, type AppStaticSnapshot } from "../store";
import { runWorkspaceTransition } from "../workspaceTransitions";
import { GuiEditor } from "./gui/GuiEditor";
import { SequenceTransportControls } from "./gui/sequence/SequenceTransportControls";
import { THEME_METRICS } from "../theme";
import { scheduleViewStateSave } from "../viewStatePersistence";
import { sameGuiDocument } from "../snapshotState";
import { resolveDocumentSyncFailure } from "../store";
import { NAVIGATE_TO_TEXT_EVENT, navigateToText, type TextNavigation } from "../workspace/navigation";

type BufferExternalState = "current" | "changedOnDisk" | "deletedOnDisk";
type EditorBufferWithExternalState = NonNullable<AppSnapshot["activeBuffer"]>;
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
  const editorHost = useRef<HTMLDivElement | null>(null);
  const view = useRef<EditorView | null>(null);
  const [editorView, setEditorView] = useState<EditorView | null>(null);
  const [editorSignal, setEditorSignal] = useState(0);
  const [pathSelection, setPathSelection] = useState<PathSelection>({ path: null, resetRevision: 0, selection: null });
  const [pendingTextNavigation, setPendingTextNavigation] = useState<TextNavigation | null>(null);
  const latestLocalText = useRef(localText);
  const applyingExternalText = useRef(false);
  const applyingRestoredEditorState = useRef(false);
  const restoredEditorPath = useRef<string | null>(null);
  const activeBuffer = snapshot.activeBuffer;
  const activePath = activeBuffer?.path ?? null;
  const activeTabPath = activeBuffer?.path ?? snapshot.activeFile;
  const viewMode = effectiveEditorViewMode(snapshot);
  const activeExternalState = activeBufferExternalState(activeBuffer);
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
      ? sequenceAudioKey(snapshot.projectEpoch, nextGuiPath, nextGuiObjectKey, activeSequenceAudio, activeSequenceDocument.durationSeconds)
      : null;
  const sequenceSelection =
    pathSelection.path === activePath && pathSelection.resetRevision === guiResetRevision
      ? pathSelection.selection
      : null;
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
    if (
      pendingTextNavigation === null
      || pendingTextNavigation.path !== activePath
      || viewMode !== "text"
      || view.current === null
    ) return;
    const editor = view.current;
    const range = pendingTextNavigation.range;
    if (range === null) {
      editor.focus();
      return;
    }
    const startLine = editor.state.doc.line(
      clamp(range.start.line + 1, 1, editor.state.doc.lines)
    );
    const endLine = editor.state.doc.line(
      clamp(range.end.line + 1, 1, editor.state.doc.lines)
    );
    const anchor = clamp(startLine.from + range.start.character, startLine.from, startLine.to);
    const head = clamp(endLine.from + range.end.character, endLine.from, endLine.to);
    editor.dispatch({
      selection: { anchor, head },
      effects: EditorView.scrollIntoView(anchor, { y: "center" })
    });
    editor.focus();
  }, [activePath, editorView, pendingTextNavigation, viewMode]);

  useEffect(() => {
    latestLocalText.current = localText;
  }, [localText]);

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

  useEffect(() => {
    if (viewMode !== "text") {
      view.current?.destroy();
      view.current = null;
      return;
    }
    if (!editorHost.current || view.current) return;
    const nextView = new EditorView({
      parent: editorHost.current,
      state: createState(
        latestLocalText.current,
        activePath,
        activeConflicted || activeReadOnly,
        (update) => {
          if (update.docChanged || update.viewportChanged || update.geometryChanged) {
            setEditorSignal((signal) => signal + 1);
          }
          if (
            activePath !== null &&
            !applyingRestoredEditorState.current &&
            (update.selectionSet || update.viewportChanged || update.geometryChanged)
          ) {
            scheduleEditorViewStateSave(activePath, readEditorViewState(update.view));
          }
          if (update.docChanged && !applyingExternalText.current) {
            if (activeConflicted || activeReadOnly) {
              return;
            }
            const text = update.state.doc.toString();
            setLocalText(text);
          }
        }
      )
    });
    view.current = nextView;
    let disposed = false;
    window.requestAnimationFrame(() => {
      if (disposed) return;
      setEditorView(nextView);
      setEditorSignal((signal) => signal + 1);
    });
    return () => {
      disposed = true;
      view.current?.destroy();
      view.current = null;
      window.requestAnimationFrame(() => {
        setEditorView(null);
      });
    };
  }, [activeConflicted, activeReadOnly, activePath, setLocalText, viewMode]);

  useEffect(() => {
    if (!view.current || viewMode !== "text" || activePath === null) return;
    if (restoredEditorPath.current === activePath) return;
    const restored = restoreState?.editorStates[activePath];
    if (restored === undefined) return;
    restoredEditorPath.current = activePath;
    applyingRestoredEditorState.current = true;
    const docLength = view.current.state.doc.length;
    view.current.dispatch({
      selection: {
        anchor: clamp(Math.floor(restored.cursorAnchor), 0, docLength),
        head: clamp(Math.floor(restored.cursorHead), 0, docLength)
      }
    });
    window.requestAnimationFrame(() => {
      if (view.current !== null && activePath === snapshot.activeFile) {
        view.current.scrollDOM.scrollTop = Math.max(0, restored.scrollTop);
        setEditorSignal((signal) => signal + 1);
      }
      applyingRestoredEditorState.current = false;
    });
  }, [activePath, restoreState, snapshot.activeFile, viewMode]);

  useEffect(() => {
    if (!view.current) return;
    if (viewMode !== "text") return;
    const current = view.current.state.doc.toString();
    if (current !== localText) {
      applyingExternalText.current = true;
      view.current.dispatch({
        changes: { from: 0, to: current.length, insert: localText }
      });
      applyingExternalText.current = false;
    }
  }, [activePath, localText, viewMode]);

  useEffect(() => {
    if (!view.current) return;
    if (viewMode !== "text") return;
    const diagnostics = editorDiagnostics(snapshot.diagnostics, activePath, snapshot.projectRoot, view.current);
    view.current.dispatch(setDiagnostics(view.current.state, diagnostics));
  }, [activePath, snapshot.diagnostics, snapshot.projectRoot, viewMode]);

  if (snapshot.tabs.length === 0) {
    return (
      <section className="editor-shell empty-editor">
        <span>{snapshot.projectRoot !== null ? "Open a Dawn file from the project tree." : "Open a project to start."}</span>
      </section>
    );
  }

  return (
    <section className={`editor-shell ${editableSequenceDocument !== null ? "has-editor-toolbar" : ""} ${activeConflicted || failedDocumentSync !== null ? "has-conflict-banner" : ""}`}>
      <div className="tab-strip">
        {snapshot.tabs.map((tab) => (
          <button
            key={tab.path}
            type="button"
            className={`tab ${tab.path === activeTabPath ? "active" : ""}`}
            aria-current={tab.path === activeTabPath ? "page" : undefined}
            onClick={() => void runSnapshotCommand(() => commands.setActiveFile(tab.path))}
          >
            <span>{tab.name}</span>
            {tab.dirty && <span className="dirty-dot" />}
            {tabExternalState(tab) !== "current" && <span className="conflict-dot" />}
            <X
              className="tab-close"
              size={THEME_METRICS.iconSizeSmall}
              onClick={(event) => {
                event.stopPropagation();
                void runWorkspaceTransition({ type: "closeFile", path: tab.path });
              }}
            />
          </button>
        ))}
      </div>
      {snapshot.projectHealth === "invalid" && (
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
        <Dialog.Root open={inlineEditorOpen} onOpenChange={(open) => { if (!open) closeInlineEditor(); }}>
          <Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog-content resource-editor-dialog" aria-describedby={undefined}>
            <header className="resource-editor-dialog-header"><Dialog.Title>{activeGuiRequest?.objectKey}</Dialog.Title><Dialog.Close asChild><button type="button" disabled={guiEditPending}>Close</button></Dialog.Close></header>
            <div className="gui-projection" inert={interactionPending} aria-busy={interactionPending}>
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
        <div className="editor-scrollbar-shell">
          <div ref={editorHost} className={`editor-host ${activeConflicted ? "conflicted" : ""}`} />
          <EditorScrollbar
            activePath={activePath}
            diagnostics={snapshot.diagnostics}
            editorSignal={editorSignal}
            projectRoot={snapshot.projectRoot}
            view={editorView}
          />
        </div>
      )}
    </section>
  );
}

type ScrollbarMetrics = {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
  railHeight: number;
  thumbTop: number;
  thumbHeight: number;
  scrollable: boolean;
};

type ScrollbarMarker = {
  id: string;
  severity: "error" | "warning";
  from: number;
  line: number;
  column: number;
  message: string;
  code: string;
  topPercent: number;
};

function EditorScrollbar({
  activePath,
  diagnostics,
  editorSignal,
  projectRoot,
  view
}: {
  activePath: string | null;
  diagnostics: ProjectDiagnostic[];
  editorSignal: number;
  projectRoot: string | null;
  view: EditorView | null;
}) {
  const railRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{ pointerId: number; startY: number; startScrollTop: number } | null>(null);
  const frameRef = useRef<number | null>(null);
  const [metrics, setMetrics] = useState<ScrollbarMetrics>(() => emptyScrollbarMetrics());

  const measure = useCallback(() => {
    if (frameRef.current !== null) return;
    frameRef.current = window.requestAnimationFrame(() => {
      frameRef.current = null;
      setMetrics(readScrollbarMetrics(view, railRef.current));
    });
  }, [view]);

  useEffect(() => {
    measure();
  }, [activePath, diagnostics, editorSignal, measure, projectRoot]);

  useEffect(() => {
    if (view === null) {
      return;
    }
    const scrollDOM = view.scrollDOM;
    const observer = new ResizeObserver(measure);
    scrollDOM.addEventListener("scroll", measure, { passive: true });
    observer.observe(scrollDOM);
    if (railRef.current !== null) observer.observe(railRef.current);
    measure();
    return () => {
      scrollDOM.removeEventListener("scroll", measure);
      observer.disconnect();
      if (frameRef.current !== null) {
        window.cancelAnimationFrame(frameRef.current);
        frameRef.current = null;
      }
    };
  }, [editorSignal, measure, view]);

  const markers = editorDiagnosticMarkers(diagnostics, activePath, projectRoot, view);

  const scrollToRatio = useCallback(
    (ratio: number) => {
      if (view === null) return;
      const scrollDOM = view.scrollDOM;
      const maxScrollTop = Math.max(0, scrollDOM.scrollHeight - scrollDOM.clientHeight);
      setScrollTop(scrollDOM, clamp(ratio, 0, 1) * maxScrollTop);
      measure();
    },
    [measure, view]
  );

  const handleTrackPointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0 || view === null || railRef.current === null) return;
      const rect = railRef.current.getBoundingClientRect();
      scrollToRatio((event.clientY - rect.top) / Math.max(1, rect.height));
    },
    [scrollToRatio, view]
  );

  const handleThumbPointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0 || view === null || !metrics.scrollable) return;
      event.preventDefault();
      event.stopPropagation();
      event.currentTarget.setPointerCapture(event.pointerId);
      dragRef.current = {
        pointerId: event.pointerId,
        startY: event.clientY,
        startScrollTop: view.scrollDOM.scrollTop
      };
    },
    [metrics.scrollable, view]
  );

  const handleThumbPointerMove = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (view === null || dragRef.current === null || dragRef.current.pointerId !== event.pointerId) return;
      const maxScrollTop = Math.max(0, metrics.scrollHeight - metrics.clientHeight);
      const maxThumbTop = Math.max(1, metrics.railHeight - metrics.thumbHeight);
      const deltaScroll = ((event.clientY - dragRef.current.startY) / maxThumbTop) * maxScrollTop;
      setScrollTop(view.scrollDOM, clamp(dragRef.current.startScrollTop + deltaScroll, 0, maxScrollTop));
      measure();
    },
    [measure, metrics.clientHeight, metrics.railHeight, metrics.scrollHeight, metrics.thumbHeight, view]
  );

  const endDrag = useCallback((event: PointerEvent<HTMLDivElement>) => {
    if (dragRef.current === null || dragRef.current.pointerId !== event.pointerId) return;
    dragRef.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
  }, []);

  const jumpToMarker = useCallback(
    (marker: ScrollbarMarker) => {
      if (view === null) return;
      view.dispatch({
        selection: { anchor: marker.from },
        effects: EditorView.scrollIntoView(marker.from, { y: "center" })
      });
      view.focus();
      measure();
    },
    [measure, view]
  );

  return (
    <div className="editor-scrollbar" aria-hidden={view === null}>
      <div ref={railRef} className="editor-scrollbar-rail" onPointerDown={handleTrackPointerDown}>
        {markers.map((marker) => (
          <button
            key={marker.id}
            type="button"
            className={`editor-scrollbar-marker ${marker.severity}`}
            style={{ top: `${marker.topPercent}%` }}
            onClick={(event) => {
              event.stopPropagation();
              jumpToMarker(marker);
            }}
            onPointerDown={(event) => {
              event.stopPropagation();
            }}
            aria-label={`${marker.severity} at ${marker.line}:${marker.column}: ${marker.message}`}
          >
            <span className="editor-scrollbar-tooltip">
              <span className="editor-scrollbar-tooltip-location">
                {marker.line}:{marker.column}
              </span>
              <span className="editor-scrollbar-tooltip-message">{marker.message}</span>
              {marker.code.length > 0 && <span className="editor-scrollbar-tooltip-code">{marker.code}</span>}
            </span>
          </button>
        ))}
        <div
          className={`editor-scrollbar-thumb ${metrics.scrollable ? "" : "disabled"}`}
          style={{ height: `${metrics.thumbHeight}px`, transform: `translateY(${metrics.thumbTop}px)` }}
          onPointerDown={handleThumbPointerDown}
          onPointerMove={handleThumbPointerMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
        />
      </div>
    </div>
  );
}

function scheduleEditorViewStateSave(path: string, state: PersistedEditorViewState) {
  scheduleViewStateSave(JSON.stringify(["editor", path]), () => commands.saveEditorViewState({ path, state }),
    (error) => { useAppStore.getState().setError(String(error)); });
}

function readEditorViewState(view: EditorView): PersistedEditorViewState {
  const selection = view.state.selection.main;
  return {
    cursorAnchor: selection.anchor,
    cursorHead: selection.head,
    scrollTop: view.scrollDOM.scrollTop
  };
}

function activeBufferExternalState(buffer: EditorBufferWithExternalState | null): BufferExternalState {
  return buffer?.externalState ?? "current";
}

function tabExternalState(tab: AppSnapshot["tabs"][number]): BufferExternalState {
  return tab.externalState;
}

function createState(
  text: string,
  path: string | null,
  readOnly: boolean,
  onUpdate: (update: ViewUpdate) => void
) {
  return EditorState.create({
    doc: text,
    extensions: [
      languageForPath(path),
      history(),
      syntaxHighlighting(dawnHighlightStyle),
      EditorState.readOnly.of(readOnly),
      EditorView.editable.of(!readOnly),
      linter(null, { autoPanel: false }),
      keymap.of([
        {
          key: "Mod-s",
          run: () => {
            void commandRegistry["file.save"].run();
            return true;
          }
        },
        ...historyKeymap,
        ...defaultKeymap
      ]),
      EditorView.updateListener.of(onUpdate),
    ]
  });
}

function editorDiagnostics(
  diagnostics: ProjectDiagnostic[],
  activePath: string | null,
  projectRoot: string | null,
  view: EditorView
): Diagnostic[] {
  if (activePath === null) return [];
  return diagnostics.flatMap((diagnostic) => {
    if (!samePath(diagnostic.path, activePath, projectRoot)) return [];
    if (diagnostic.range === null) return [];
    const range = rangeToOffsets(diagnostic.range, view);
    if (range === null) return [];
    return [
      {
        from: range.from,
        to: range.to,
        severity: diagnostic.severity,
        message: diagnostic.message,
        renderMessage: () => renderDiagnosticMessage(diagnostic, range.from, view)
      }
    ];
  });
}

function renderDiagnosticMessage(diagnostic: ProjectDiagnostic, from: number, view: EditorView): Node {
  const fragment = document.createDocumentFragment();
  fragment.append(
    textSpan("cm-diagnostic-location", diagnosticLocation(from, view)),
    textSpan("cm-diagnostic-message", diagnostic.message),
    textSpan("cm-diagnostic-code", diagnostic.code.trim())
  );
  return fragment;
}

function textSpan(className: string, text: string): HTMLSpanElement {
  const span = document.createElement("span");
  span.className = className;
  span.textContent = text;
  return span;
}

function editorDiagnosticMarkers(
  diagnostics: ProjectDiagnostic[],
  activePath: string | null,
  projectRoot: string | null,
  view: EditorView | null
): ScrollbarMarker[] {
  if (activePath === null || view === null) return [];
  const contentHeight = Math.max(1, view.contentHeight);
  return diagnostics.flatMap((diagnostic, index) => {
    if (!samePath(diagnostic.path, activePath, projectRoot)) return [];
    if (diagnostic.range === null) return [];
    const range = rangeToOffsets(diagnostic.range, view);
    if (range === null) return [];
    const line = view.state.doc.lineAt(range.from);
    const block = view.lineBlockAt(range.from);
    return [
      {
        id: `${diagnostic.path}:${index}:${range.from}:${diagnostic.code}`,
        severity: diagnostic.severity,
        from: range.from,
        line: line.number,
        column: range.from - line.from + 1,
        message: diagnostic.message,
        code: diagnostic.code.trim(),
        topPercent: clamp((block.top / contentHeight) * 100, 0, 100)
      }
    ];
  });
}

function diagnosticLocation(from: number, view: EditorView): string {
  const line = view.state.doc.lineAt(from);
  return `${line.number}:${from - line.from + 1}`;
}

function samePath(left: string, right: string, projectRoot: string | null): boolean {
  const normalizedLeft = normalizePath(left);
  const normalizedRight = normalizePath(right);
  if (normalizedLeft === normalizedRight) return true;
  if (projectRoot === null || isAbsolutePath(right)) return false;
  return normalizedLeft === normalizePath(`${projectRoot}/${right}`);
}

function normalizePath(path: string): string {
  return path.replace(/^\/\/\?\//, "").replace(/\\/g, "/").toLowerCase();
}

function isAbsolutePath(path: string): boolean {
  const normalized = normalizePath(path);
  return /^[a-z]:\//.test(normalized) || normalized.startsWith("/");
}

function rangeToOffsets(range: TextRange, view: EditorView): { from: number; to: number } | null {
  const from = positionToOffset(range.start.line, range.start.character, view);
  const rawTo = positionToOffset(range.end.line, range.end.character, view);
  if (from === null || rawTo === null) return null;
  let to = Math.max(rawTo, from);
  if (to === from && from < view.state.doc.length) {
    to += 1;
  }
  return to > from ? { from, to } : null;
}

function positionToOffset(line: number, character: number, view: EditorView): number | null {
  if (!Number.isFinite(line) || !Number.isFinite(character) || line < 0 || character < 0) return null;
  const doc = view.state.doc;
  if (doc.lines === 0) return 0;
  const lineNumber = Math.min(Math.floor(line) + 1, doc.lines);
  const docLine = doc.line(lineNumber);
  const offset = Math.min(Math.floor(character), docLine.length);
  return docLine.from + offset;
}

function emptyScrollbarMetrics(): ScrollbarMetrics {
  return {
    scrollTop: 0,
    clientHeight: 0,
    scrollHeight: 0,
    railHeight: 0,
    thumbTop: 0,
    thumbHeight: 0,
    scrollable: false
  };
}

function readScrollbarMetrics(view: EditorView | null, rail: HTMLElement | null): ScrollbarMetrics {
  if (view === null || rail === null) return emptyScrollbarMetrics();
  const scrollDOM = view.scrollDOM;
  const railHeight = rail.clientHeight;
  const scrollTop = scrollDOM.scrollTop;
  const clientHeight = scrollDOM.clientHeight;
  const scrollHeight = scrollDOM.scrollHeight;
  const scrollable = scrollHeight > clientHeight + 1;
  const thumbHeight = scrollable ? Math.max(28, (clientHeight / scrollHeight) * railHeight) : railHeight;
  const maxScrollTop = Math.max(1, scrollHeight - clientHeight);
  const maxThumbTop = Math.max(0, railHeight - thumbHeight);
  return {
    scrollTop,
    clientHeight,
    scrollHeight,
    railHeight,
    thumbTop: scrollable ? (scrollTop / maxScrollTop) * maxThumbTop : 0,
    thumbHeight,
    scrollable
  };
}

function setScrollTop(scrollDOM: HTMLElement, scrollTop: number): void {
  scrollDOM.scrollTop = scrollTop;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function languageForPath(path: string | null): Extension {
  if (
    path !== null &&
    (path.endsWith(".effect.dawn") || path.endsWith(".operator.dawn"))
  ) {
    return cpp();
  }
  return yaml();
}

const dawnHighlightStyle = HighlightStyle.define([
  { tag: tags.keyword, color: "var(--dawn-code-keyword)" },
  { tag: [tags.name, tags.propertyName, tags.attributeName], color: "var(--dawn-code-name)" },
  { tag: [tags.variableName, tags.definition(tags.variableName)], color: "var(--dawn-text)" },
  { tag: [tags.function(tags.variableName), tags.function(tags.definition(tags.variableName))], color: "var(--dawn-code-function)" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--dawn-code-string)" },
  { tag: [tags.number, tags.bool, tags.null], color: "var(--dawn-code-number)" },
  { tag: [tags.operator, tags.punctuation, tags.separator], color: "var(--dawn-text-muted)" },
  { tag: tags.comment, color: "var(--dawn-text-muted)", fontStyle: "italic" },
  { tag: [tags.typeName, tags.className], color: "var(--dawn-code-type)" },
  { tag: tags.invalid, color: "var(--dawn-code-invalid)" }
]);

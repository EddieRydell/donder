import { commands, errorMessage } from "../../../api";
import { useAppStore, runGuiEditCommand, runSnapshotCommand } from "../../../store";
import { put, site } from "../edits";
import { MIN_CLIP_DURATION_SECONDS, isTimed, markIndexAfterMove, useSequenceSelection } from "./sequenceSelection";
import { focusedClipId, setDocumentUi, showMarks, useDocumentUi } from "../documentUi";
import { hasPrimaryModifier, isMac, isSecondaryClick } from "../../../platform";
import { useEditorCommands } from "../../../shortcuts";
import { OverlayPortal } from "../../OverlayPortal";
import { objectViewKey } from "../../../workspace/guiIdentity";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState, type Dispatch, type MouseEvent, type PointerEvent, type RefObject, type SetStateAction, useContext } from "react";
import { createStore, useStore, type StoreApi } from "zustand";

import { ChevronRight, Scissors, Trash2 } from "lucide-react";
import { ContextMenuEntries, Tip, type MenuEntry } from "../../kit";


import { useCollapsedGroups, useObjectViewState } from "../../../viewStatePersistence";

import type { AppSettings, SequenceClipArrangement, SequenceClipMatch, SequenceSelection, SequenceClipParamMatch, GuiDocumentRequest, FixtureTarget, PersistedSequenceViewportState, PlaybackRange, SequenceCurvePoint, SequenceFollowMode, SequenceAutomationTarget, SequenceGuiDocument, SequenceClipScope, SequenceEffectDefinition, SequenceGuiEdit } from "../../../types";


import { clamp, envelopeOf, formatSeconds, roundToNanosecond, type AudioTransportViewSnapshot } from "../shared";

import { activeMarkCollection, defaultMarkColor, drawSequenceMarks, committedMarkDrafts } from "./marks";

import { graphOperatorDefinition } from "./graphOperator";
import { lanesForTarget, nearestLane, targetAtLane, targetsEqual } from "./sequenceTargets";
import { SequenceWaveform } from "./sequenceWaveform";
import { drawClipRaster, useSequenceClipRasters } from "./sequenceClipRasters";
import { sequenceTransportCommands, useSequenceTransport } from "./SequenceTransportControls";
import {
  automationCurvePointFromCanvas,
  drawEnvelope,
  fitCanvasLabel,
  removeAutomationCurvePoint,
  insertAutomationCurvePoint,
  eraseAutomationCurvePoints,
  moveAutomationCurvePoint,
  alignAutomationCurvePoint,
  type AutomationGuide,
  type AutomationCurveDraft
} from "./sequenceAutomationLayout";
import {
  automationLaneRowHeight,
  rowHeightAt,
  persistRowHeights,
  restoreRowHeights,
  expandedTimelineHeight,
  minRowHeight,
  laneIndexFromCanvasY,
  sequenceRowLayout,
  collapseRows,
  type SequenceRowKind,
  type SequenceRowHeightMap
} from "./sequenceRows";
import { THEME_COLORS, THEME_METRICS, THEME_TYPOGRAPHY } from "../../../theme";
import { definitionEntries, definitionTree } from "./definitionMenu";
import { clipEdgeSnapTimes, markSnapTimes, snapDeltaToTimes, snapToTime } from "./sequenceSnap";
import { toggleSliceTool, useSequenceTool } from "./sequenceTool";
import { CascadeDialog, ClipArrangeMenus } from "./ClipArrange";
import { ClipMatchDialog, ClipMatchMenuItems, type ClipMatchSeed, type MatchClips } from "./ClipMatch";
import { sameGuiDocument } from "../../../snapshotState";

import { hit, laneLabelLayout, reduce, sharedBadgeX, TIMELINE_STRIPS, type CanvasLayout, type GestureContext, type GestureState, type TimelineStrip } from "./canvasGestures";
import { buildSequenceClipLayout, clipRowKind, constrainMarkDelta, earliestStartSeconds, selectionStartSeconds, markMoveDrafts, markRefLookup, mergeSequenceSelection, nextClipSelection, nextMarkSelection, normalizedRect, selectionCount, selectionFromMarqueeClips, selectionFromMarqueeMarks, selectionFromTimeSpan, sequenceHoverEqual, sequenceSelectionsEqual, setMarkDraft, singleSelectionFocus, type MarkDraftLookup, type MarqueeBox, type SequenceClipLayout, type SequenceContextMenu, type SequenceHover, type SequenceMarquee, type SequenceDraft, type SequenceViewport } from "./sequenceSelection";

const SEQUENCE_FOLLOW = {
  edge: THEME_METRICS.sequenceFollowEdge,
  lead: THEME_METRICS.sequenceFollowLead,
  anchor: THEME_METRICS.sequenceFollowAnchor
};

const SEQUENCE_CANVAS = {
  initialLeftGutterPx: THEME_METRICS.sequenceLeftGutter,
  minLeftGutterPx: THEME_METRICS.sequenceMinLeftGutter,
  maxLeftGutterPx: THEME_METRICS.sequenceMaxLeftGutter,
  leftGutterResizeHitWidthPx: THEME_METRICS.sequenceLeftGutterResizeHitWidth,
  audioStripTopPx: THEME_METRICS.sequenceAudioStripTop,
  initialAudioStripHeightPx: THEME_METRICS.sequenceInitialAudioStripHeight,
  initialMarkRulerHeightPx: THEME_METRICS.sequenceInitialMarkRulerHeight,
  initialPxPerSecond: THEME_METRICS.sequenceInitialPixelsPerSecond,
  initialLaneHeightPx: THEME_METRICS.sequenceInitialLaneHeight,
  minPxPerSecond: THEME_METRICS.sequenceMinPixelsPerSecond,
  maxPxPerSecond: THEME_METRICS.sequenceMaxPixelsPerSecond,
  maxZoomPxPerSecond: THEME_METRICS.sequenceMaxZoomPixelsPerSecond,
  keyZoomFactor: THEME_METRICS.sequenceKeyZoomFactor,
  minLaneHeightPx: THEME_METRICS.sequenceMinLaneHeight,
  maxLaneHeightPx: THEME_METRICS.sequenceMaxLaneHeight,
  audioResizeHitHeightPx: THEME_METRICS.sequenceAudioResizeHitHeight,
  wheelZoomScale: THEME_METRICS.sequenceWheelZoomScale,
  pinchZoomScale: THEME_METRICS.sequencePinchZoomScale,
  scrubStepSeconds: THEME_METRICS.sequenceScrubStep,
  nudgeSeconds: THEME_METRICS.sequenceNudgeStep,
  shiftedNudgeSeconds: THEME_METRICS.sequenceShiftedNudgeStep
} as const;

const SEQUENCE_COLORS = {
  page: THEME_COLORS.page,
  panel: THEME_COLORS.panel,
  laneAlt: THEME_COLORS.sequenceLaneAlternate,
  laneSelected: THEME_COLORS.sequenceLaneSelected,
  grid: THEME_COLORS.sequenceGrid,
  border: THEME_COLORS.border,
  timelineMajor: THEME_COLORS.timelineMajor,
  timelineMinor: THEME_COLORS.timelineMinor,
  timelineLabel: THEME_COLORS.textMuted,
  textMuted: THEME_COLORS.textSoft,
  textFaint: THEME_COLORS.textFaint,
  overlay: THEME_COLORS.overlay,
  clipSelected: THEME_COLORS.clipSelected,
  clipHover: THEME_COLORS.clipHover,
  clipBorder: THEME_COLORS.clipBorder,
  automation: THEME_COLORS.automation,
  spectrogram: THEME_COLORS.spectrogram,
  spectrogramHighlight: THEME_COLORS.white,
  automationFill: THEME_COLORS.automationFill,
  automationGraph: THEME_COLORS.automationGraph,
  automationGraphGrid: THEME_COLORS.automationGraphGrid,
  automationGraphGridMajor: THEME_COLORS.automationGraphMajorGrid,
  accent: THEME_COLORS.accent,
  accentSubtle: THEME_COLORS.accentSubtle,
  warning: THEME_COLORS.warning,
  markMarquee: THEME_COLORS.markMarquee,
  markMarqueeFill: THEME_COLORS.markMarqueeFill,
  clipMarqueeFill: THEME_COLORS.clipMarqueeFill,
  sliceLine: THEME_COLORS.sequenceSliceLine,
  sliceGuide: THEME_COLORS.sequenceSliceGuide
} as const;

const SEQUENCE_DRAG_THRESHOLD_PX = THEME_METRICS.sequenceDragThreshold;

type LaneContextMenu = Extract<SequenceContextMenu, { laneIndex: number }>;

/** What a pointer in the ruler and waveform strip would grab. */
type SeekHover = { target: "playhead" | "rangeEdge" | "home" | "time"; seconds: number };
const SEEK_HOVER_CURSORS = { playhead: "grab", rangeEdge: "ew-resize", home: "text", time: "text" } as const;
/** What the transport overlay shows of a pointer gesture; only the overlay re-renders as it changes. */
type SequenceOverlayState = { seekHover: SeekHover | null; rangeDraft: PlaybackRange | null };

/** The slice tool's cut line: a time, and the vertical span it cuts across. */
type SliceGuide = { seconds: number; fromY: number; toY: number };

/** A shared lane's "×N" badge, in canvas coordinates. */
type SharedBadgeHover = { laneIndex: number; y: number; height: number; scrollY: number; left: number };



export function SequenceCanvas({ document }: { document: SequenceGuiDocument }) {
  const sequenceSelection = useSequenceSelection(document);
  // One selected clip or mark is the focused one; a sequence never stores focus apart from its selection.
  const selected = singleSelectionFocus(sequenceSelection);
  const setSequenceSelection = (selection: SequenceSelection | null) => { setDocumentUi({ selection }); };
  const bindingChooser = useDocumentUi((ui) => ui.bindingChooser);
  const activeMarkCollectionKey = useDocumentUi((ui) => ui.activeMarks);
  const visibleMarkCollectionKeys = useDocumentUi((ui) => ui.visibleMarks);
  const overlayContainer = useContext(OverlayPortal);

  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<GestureState | null>(null);
  const playheadClockRef = useRef<(() => number) | null>(null);
  const sequenceSelectionRef = useRef<SequenceSelection | null>(sequenceSelection);
  const [revealAutomation, setRevealAutomation] = useState(false);
  const [automationCurveDraft, setAutomationCurveDraft] = useState<AutomationCurveDraft | null>(null);
  const [groupDraft, setGroupDraft] = useState<SequenceDraft[]>([]);
  const [markDrafts, setMarkDrafts] = useState<MarkDraftLookup>(() => new Map());
  const [sequenceContextMenu, setSequenceContextMenu] = useState<SequenceContextMenu | null>(null);
  // Pointer-move state lives in refs and repaints the canvas on the next frame, so moving the
  // pointer does not re-render the editor. React state keeps what changes the rendered elements.
  const paintRef = useRef<() => void>(() => undefined);
  const paintFrame = useRef<number | null>(null);
  const requestPaint = useCallback(() => {
    if (paintFrame.current !== null) return;
    paintFrame.current = window.requestAnimationFrame(() => {
      paintFrame.current = null;
      paintRef.current();
    });
  }, []);
  useEffect(() => () => { if (paintFrame.current !== null) window.cancelAnimationFrame(paintFrame.current); }, []);
  const setTransient = <T,>(ref: RefObject<T>, next: T) => {
    if (ref.current === next) return;
    ref.current = next;
    requestPaint();
  };
  const hoverRef = useRef<SequenceHover>(null);
  const setHover = (next: SequenceHover) => { if (!sequenceHoverEqual(hoverRef.current, next)) setTransient(hoverRef, next); };
  const rowResizeHoverRef = useRef<{ laneIndex: number; rowKind: SequenceRowKind } | null>(null);
  const setRowResizeHover = (next: { laneIndex: number; rowKind: SequenceRowKind } | null) => {
    const previous = rowResizeHoverRef.current;
    if (previous?.laneIndex !== next?.laneIndex || previous?.rowKind !== next?.rowKind) setTransient(rowResizeHoverRef, next);
  };
  const stripResizeHoverRef = useRef<TimelineStrip | null>(null);
  const setStripResizeHover = (next: TimelineStrip | null) => { setTransient(stripResizeHoverRef, next); };
  const gutterResizeHoverRef = useRef(false);
  const setGutterResizeHover = (next: boolean) => { setTransient(gutterResizeHoverRef, next); };
  // The "×N" badge of the lane under the pointer, and the tooltip shown after the hover delay.
  const [badgeHover, setBadgeHover] = useState<SharedBadgeHover | null>(null);
  const [badgeDelayed, setBadgeDelayed] = useState<SharedBadgeHover | null>(null);
  useEffect(() => {
    if (badgeHover === null) return;
    const timer = window.setTimeout(() => { setBadgeDelayed(badgeHover); }, THEME_METRICS.tooltipDelayMs);
    return () => { window.clearTimeout(timer); };
  }, [badgeHover]);
  const dragCursorRef = useRef<"grabbing" | null>(null);
  const setDragCursor = (next: "grabbing" | null) => { setTransient(dragCursorRef, next); };
  const [overlay] = useState(() => createStore<SequenceOverlayState>(() => ({ seekHover: null, rangeDraft: null })));
  const setSeekHover = (next: SeekHover | null) => {
    const previous = overlay.getState().seekHover;
    if (previous?.target === next?.target && previous?.seconds === next?.seconds) return;
    overlay.setState({ seekHover: next });
    requestPaint();
  };
  const setRangeDraft = (rangeDraft: PlaybackRange | null) => { overlay.setState({ rangeDraft }); };
  const tool = useSequenceTool((state) => state.tool);
  const setTool = useSequenceTool((state) => state.setTool);
  const sliceGuideRef = useRef<SliceGuide | null>(null);
  const setSlice = (next: SliceGuide | null) => { setTransient(sliceGuideRef, next); };
  // A target may have several lanes; the selection keeps the chosen lane, or its target's nearest
  // lane after the layout changes.
  const [selectedRow, setSelectedRow] = useState<{ target: FixtureTarget; lane: number } | null>(null);
  const selectedLaneIndex = selectedRow === null ? null
    : document.targets[selectedRow.lane] !== undefined && targetsEqual(targetAtLane(document, selectedRow.lane), selectedRow.target) ? selectedRow.lane
    : nearestLane(document, selectedRow.target, selectedRow.lane);
  const setSelectedLaneIndex = (index: number) => { setSelectedRow({ target: targetAtLane(document, index), lane: index }); };
  const [selectedTimeSeconds, setSelectedTimeSeconds] = useState<number | null>(null);
  const marqueeRef = useRef<SequenceMarquee | null>(null);
  // A clip box lays out the whole sequence, so only its opening and closing re-render.
  const [clipMarqueeOpen, setClipMarqueeOpen] = useState(false);
  const setMarquee = (next: SequenceMarquee | null) => {
    setClipMarqueeOpen(next?.mode === "clips");
    setTransient(marqueeRef, next);
  };
  const [canvasSize, setCanvasSize] = useState({ width: 0, height: 0 });
  const sequenceScrollbar = useRef<{ pointerId: number; startX: number; startScroll: number } | null>(null);
  const gestureRequest = useRef<GuiDocumentRequest | null>(null);
  /** A right-drag that erased points must not also open the context menu its release raises. */
  const suppressContextMenu = useRef(false);
  const settings = useAppStore((store) => store.snapshot?.settings ?? null);
  const playing = useAppStore((store) => store.snapshot?.audioTransport.state === "playing");
  // Continuous following owns horizontal scrolling while playing; zoom stays anchored on the playhead.
  const followingContinuously = playing && settings?.sequenceFollowMode === "continuous";
  const restoreKey = objectViewKey(document.sourceRef);
  const [restoredViewport, saveViewport] = useObjectViewState("sequenceViewport", document.sourceRef);
  const [viewport, setViewport] = useState<SequenceViewport>(() => sequenceViewportFromPersisted(restoredViewport, document, settings));
  const viewportInitialized = useRef(false);
  const restoredViewportKey = useRef<string | null>(restoredViewport === undefined ? null : restoreKey);
  const left = viewport.leftGutterWidth;
  const scrollbarHeight = THEME_METRICS.scrollbarWidth;
  const timelineWidth = Math.max(1, canvasSize.width - left);
  const maxScrollXSeconds = Math.max(0, document.durationSeconds - timelineWidth / viewport.pxPerSecond);
  const scrollbarThumbWidth = maxScrollXSeconds === 0
    ? timelineWidth
    : Math.min(timelineWidth, Math.max(THEME_METRICS.scrollbarThumbMinHeight, timelineWidth * (timelineWidth / (document.durationSeconds * viewport.pxPerSecond))));
  const scrollbarThumbLeft = maxScrollXSeconds === 0
    ? 0
    : (timelineWidth - scrollbarThumbWidth) * viewport.scrollXSeconds / maxScrollXSeconds;
  const audioStripTop = SEQUENCE_CANVAS.audioStripTopPx;
  const audioStripHeight = viewport.audioStripHeight;
  // The time ruler and waveform strip are the transport area; the mark ruler sits between it and the lanes.
  const audioStripBottom = audioStripTop + audioStripHeight;
  const markRulerHeight = viewport.markRulerHeight;
  const marksLaneOnly = settings?.sequenceMarksLaneOnly ?? false;
  /** Drags snap when the snap setting is on; holding Alt inverts it for the gesture. */
  const snaps = (altKey: boolean) => (settings?.sequenceSnap ?? false) !== altKey;
  const top = audioStripBottom + markRulerHeight;
  const targetMarkCollectionKey = activeMarkCollection(document.markCollections, activeMarkCollectionKey)?.name ?? null;
  const automationRowHeight = automationLaneRowHeight(initialSequenceLaneHeight(settings));
  const envelopeTargets = useMemo(() => document.clips.filter((clip) => clip.content.type === "envelope").map((clip) => clip.target), [document.clips]);
  // Group lanes collapse like folders in a file tree; their members and envelopes take no rows.
  const [collapsedGroups, saveCollapsedGroups] = useCollapsedGroups();
  const layoutRows = useCallback(
    (rowHeights: SequenceRowHeightMap, reveal: boolean) =>
      collapseRows(sequenceRowLayout(document.targets, envelopeTargets, rowHeights, initialSequenceLaneHeight(settings), automationRowHeight, reveal), document.targets, collapsedGroups),
    [document.targets, envelopeTargets, settings, automationRowHeight, collapsedGroups]
  );
  const rows = useMemo(() => layoutRows(viewport.rowHeights, revealAutomation), [layoutRows, viewport.rowHeights, revealAutomation]);
  const visibleMarkCollections = useMemo(
    () => document.markCollections.filter((collection) => visibleMarkCollectionKeys.has(collection.name)),
    [document.markCollections, visibleMarkCollectionKeys]
  );
  const markTimes = useMemo(() => markSnapTimes(visibleMarkCollections), [visibleMarkCollections]);
  /** The "×N" badge of a shared lane, where its tooltip shows. */
  const sharedBadge = (laneIndex: number): SharedBadgeHover | null => {
    const row = rows.find((candidate) => candidate.laneIndex === laneIndex && candidate.kind === "clips");
    return row === undefined ? null : { laneIndex, y: top + row.top - viewport.scrollY, height: row.height, scrollY: viewport.scrollY, left };
  };
  // The tooltip shows once the hover delay passes, until scrolling or resizing moves the badge.
  const badgeTooltip = badgeHover !== null && badgeDelayed === badgeHover && badgeHover.scrollY === viewport.scrollY && badgeHover.left === left ? badgeHover : null;
  const badgeLane = badgeTooltip === null ? undefined : document.targets[badgeTooltip.laneIndex];
  const canvasCursor = (slice: SliceGuide | null) => {
    const seekHover = overlay.getState().seekHover;
    const hover = hoverRef.current;
    return dragCursorRef.current ??
      (slice !== null ? "crosshair" :
      seekHover !== null ? SEEK_HOVER_CURSORS[seekHover.target] :
      stripResizeHoverRef.current !== null || rowResizeHoverRef.current !== null ? "ns-resize" :
      gutterResizeHoverRef.current ? "ew-resize" :
      (hover === null ? "" : hover.kind === "mark" || bindingChooser !== null ? "pointer" : hover.resize === "none" ? "grab" : "ew-resize"));
  };

  const updateSequenceSelection = (selection: SequenceSelection | null) => {
    sequenceSelectionRef.current = selection;
    setSequenceSelection(selection);
  };

  const handleScrollbarPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    // Without this, dragging the scrollbar starts a text selection across the editor.
    event.preventDefault();
    if (maxScrollXSeconds === 0 || followingContinuously) return;
    const rail = event.currentTarget;
    const rect = rail.getBoundingClientRect();
    if (event.target === rail) {
      const nextLeft = clamp(event.clientX - rect.left - scrollbarThumbWidth / 2, 0, rect.width - scrollbarThumbWidth);
      setViewport((current) => ({ ...current, scrollXSeconds: maxScrollXSeconds * nextLeft / Math.max(1, rect.width - scrollbarThumbWidth) }));
      return;
    }
    sequenceScrollbar.current = { pointerId: event.pointerId, startX: event.clientX, startScroll: viewport.scrollXSeconds };
    rail.setPointerCapture(event.pointerId);
  };
  const handleScrollbarPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const drag = sequenceScrollbar.current;
    if (drag === null || drag.pointerId !== event.pointerId || followingContinuously) return;
    const rail = event.currentTarget;
    const travel = Math.max(1, rail.clientWidth - scrollbarThumbWidth);
    setViewport((current) => ({ ...current, scrollXSeconds: clamp(drag.startScroll + (event.clientX - drag.startX) / travel * maxScrollXSeconds, 0, maxScrollXSeconds) }));
  };

  const handleWheel = useCallback((event: WheelEvent) => {
    const target = canvas.current;
    if (target === null) return;
    const rect = target.getBoundingClientRect();
    const offsetX = event.clientX - rect.left;
    const timelineWidth = Math.max(1, rect.width - left);
    const visibleHeight = Math.max(1, rect.height - scrollbarHeight - top);
    const zoomDelta = event.deltaY !== 0 ? event.deltaY : event.deltaX;
    const horizontalDelta = event.deltaX !== 0 ? event.deltaX : event.deltaY;

    event.preventDefault();
    setViewport((current) => {
      const maxScrollXSeconds = Math.max(0, document.durationSeconds - timelineWidth / current.pxPerSecond);
      const maxScrollY = Math.max(0, expandedTimelineHeight(layoutRows(current.rowHeights, revealAutomation)) - visibleHeight);
      // Trackpad pinches arrive as wheel events with ctrlKey set.
      const zoom = hasPrimaryModifier(event) || (isMac && event.ctrlKey);
      const zoomScale = isMac && event.ctrlKey ? SEQUENCE_CANVAS.pinchZoomScale : SEQUENCE_CANVAS.wheelZoomScale;
      if (zoom && event.shiftKey) {
        const scale = Math.exp(-zoomDelta * zoomScale);
        const rowHeights = Object.fromEntries(Object.entries(completeRowHeights(current.rowHeights, document, settings)).map(([id, heights]) => [id, { clips: clamp(heights.clips * scale, SEQUENCE_CANVAS.minLaneHeightPx, SEQUENCE_CANVAS.maxLaneHeightPx), automation: clamp(heights.automation * scale, minRowHeight("automation"), SEQUENCE_CANVAS.maxLaneHeightPx) }]));
        // Keep the content under the cursor fixed: same row, same fraction of its height.
        const previousRows = layoutRows(current.rowHeights, revealAutomation);
        const nextRows = layoutRows(rowHeights, revealAutomation);
        const anchorY = clamp(event.clientY - rect.top - top, 0, visibleHeight);
        const anchorContentY = current.scrollY + anchorY;
        const anchorIndex = previousRows.findIndex((row) => anchorContentY < row.bottom);
        const previousRow = previousRows[anchorIndex];
        const nextRow = nextRows[anchorIndex];
        const nextAnchorContentY = previousRow === undefined || nextRow === undefined
          ? expandedTimelineHeight(nextRows) + anchorContentY - expandedTimelineHeight(previousRows)
          : nextRow.top + (anchorContentY - previousRow.top) / Math.max(1, previousRow.height) * nextRow.height;
        return {
          ...current,
          rowHeights,
          scrollY: clamp(nextAnchorContentY - anchorY, 0, Math.max(0, expandedTimelineHeight(nextRows) - visibleHeight))
        };
      }
      if (zoom) {
        const anchorX = clamp(followingContinuously
          ? ((useAppStore.getState().snapshot?.audioTransport.positionSeconds ?? current.scrollXSeconds) - current.scrollXSeconds) * current.pxPerSecond
          : offsetX - left, 0, timelineWidth);
        const anchorTime = current.scrollXSeconds + anchorX / current.pxPerSecond;
        const nextPxPerSecond = clamp(
          current.pxPerSecond * Math.exp(-zoomDelta * zoomScale),
          minSequencePxPerSecond(timelineWidth, document.durationSeconds),
          SEQUENCE_CANVAS.maxZoomPxPerSecond
        );
        const nextScrollXSeconds = anchorTime - anchorX / nextPxPerSecond;
        return {
          ...current,
          pxPerSecond: nextPxPerSecond,
          scrollXSeconds: clamp(nextScrollXSeconds, 0, Math.max(0, document.durationSeconds - timelineWidth / nextPxPerSecond))
        };
      }
      if (event.shiftKey) {
        if (followingContinuously) return current;
        return {
          ...current,
          scrollXSeconds: clamp(current.scrollXSeconds + horizontalDelta / current.pxPerSecond, 0, maxScrollXSeconds)
        };
      }
      return {
        ...current,
        scrollXSeconds: followingContinuously ? current.scrollXSeconds : clamp(current.scrollXSeconds + event.deltaX / current.pxPerSecond, 0, maxScrollXSeconds),
        scrollY: clamp(current.scrollY + event.deltaY, 0, maxScrollY)
      };
    });
  }, [document, followingContinuously, layoutRows, left, revealAutomation, scrollbarHeight, settings, setViewport, top]);

  useEffect(() => {
    const target = canvas.current;
    if (target === null) return;
    target.addEventListener("wheel", handleWheel, { passive: false });
    return () => {
      target.removeEventListener("wheel", handleWheel);
    };
  }, [handleWheel]);

  useEffect(() => {
    sequenceSelectionRef.current = sequenceSelection;
  }, [sequenceSelection]);

  // An undo or redo replaces the document, and with it what a gesture was drawing.
  useEffect(() => useAppStore.subscribe((store, previous) => {
    if (store.guiHistoryRevision === previous.guiHistoryRevision) return;
    drag.current = null;
    gestureRequest.current = null;
    sequenceSelectionRef.current = null;
    setGroupDraft([]);
    setAutomationCurveDraft(null);
    setMarkDrafts(new Map());
    setRevealAutomation(false);
    dragCursorRef.current = null;
    marqueeRef.current = null;
    setClipMarqueeOpen(false);
    setSequenceContextMenu(null);
    requestPaint();
  }), [requestPaint]);


  useEffect(() => {
    const target = canvas.current;
    if (!target) return;
    const updateSize = () => {
      const rect = target.getBoundingClientRect();
      const visibleHeight = Math.max(0, rect.height - scrollbarHeight);
      setCanvasSize({ width: rect.width, height: visibleHeight });
      const timelineWidth = Math.max(1, rect.width - left);
      if (rect.width > 0 && !viewportInitialized.current) {
        viewportInitialized.current = true;
        if (restoredViewport === undefined) {
          setViewport({
            pxPerSecond: initialSequencePxPerSecond(settings, timelineWidth, document.durationSeconds),
            leftGutterWidth: SEQUENCE_CANVAS.initialLeftGutterPx,
            audioStripHeight: SEQUENCE_CANVAS.initialAudioStripHeightPx,
            markRulerHeight: SEQUENCE_CANVAS.initialMarkRulerHeightPx,
            rowHeights: completeRowHeights({}, document, settings),
            scrollXSeconds: 0,
            scrollY: 0
          });
        }
      }
      setViewport((current) => {
        const minPxPerSecond = minSequencePxPerSecond(timelineWidth, document.durationSeconds);
        const pxPerSecond = Math.max(current.pxPerSecond, minPxPerSecond);
        const scrollXSeconds = clamp(current.scrollXSeconds, 0, Math.max(0, document.durationSeconds - timelineWidth / pxPerSecond));
        const rowHeights = completeRowHeights(current.rowHeights, document, settings);
        const rowsChanged = rowHeights !== current.rowHeights;
        const maxScrollY = Math.max(0, expandedTimelineHeight(layoutRows(rowHeights, revealAutomation)) - Math.max(1, visibleHeight - top));
        const scrollY = clamp(current.scrollY, 0, maxScrollY);
        if (!rowsChanged && pxPerSecond === current.pxPerSecond && scrollXSeconds === current.scrollXSeconds && scrollY === current.scrollY) return current;
        return {
          ...current,
          rowHeights,
          pxPerSecond,
          scrollXSeconds,
          scrollY
        };
      });
    };
    const frame = window.requestAnimationFrame(updateSize);
    const observer = new ResizeObserver(updateSize);
    observer.observe(target);
    return () => {
      window.cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [automationRowHeight, revealAutomation, document, layoutRows, left, restoredViewport, scrollbarHeight, settings, top]);

  useEffect(() => {
    if (restoredViewport === undefined || restoredViewportKey.current === restoreKey) return;
    restoredViewportKey.current = restoreKey;
    setViewport(sequenceViewportFromPersisted(restoredViewport, document, settings));
  }, [document, restoreKey, restoredViewport, settings]);

  useEffect(() => {
    // The view moves every frame while following; it saves once following stops.
    if (followingContinuously) return;
    const state: PersistedSequenceViewportState = {
      pxPerSecond: viewport.pxPerSecond,
      leftGutterWidthPx: viewport.leftGutterWidth,
      audioStripHeightPx: viewport.audioStripHeight,
      markRulerHeightPx: viewport.markRulerHeight,
      rowHeights: persistRowHeights(viewport.rowHeights),
      scrollXSeconds: viewport.scrollXSeconds,
      scrollY: viewport.scrollY,
      activeMarkCollectionKey,
      visibleMarkCollectionKeys: [...visibleMarkCollectionKeys]
    };
    saveViewport(state);
  }, [followingContinuously, activeMarkCollectionKey, saveViewport, viewport, visibleMarkCollectionKeys]);

  const visibleClips = useMemo(
    () => buildSequenceClipLayout(
      document,
      groupDraft,
      automationCurveDraft,
      viewport,
      left,
      top,
      canvasSize,
      rows
    ),
    [automationCurveDraft, canvasSize, document, groupDraft, left, top, viewport, rows]
  );
  // A clip box covers the whole sequence, so clips it holds stay selected after they scroll out of view.
  const marqueeContent = useMemo(() => {
    if (!clipMarqueeOpen) return null;
    const content = { ...viewport, scrollXSeconds: 0, scrollY: 0 };
    const bounds = { width: left + document.durationSeconds * viewport.pxPerSecond, height: top + expandedTimelineHeight(rows) };
    return buildSequenceClipLayout(document, groupDraft, automationCurveDraft, content, left, top, bounds, rows);
  }, [automationCurveDraft, clipMarqueeOpen, document, groupDraft, left, rows, top, viewport]);
  const applyMarquee = (state: SequenceMarquee) => {
    const anchorX = left + (state.anchorSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
    const anchorY = state.mode === "marks" ? state.anchorY : top + state.anchorY - viewport.scrollY;
    const next = { ...state, active: state.active || Math.hypot(state.x - anchorX, state.y - anchorY) >= SEQUENCE_DRAG_THRESHOLD_PX };
    drag.current = { kind: "marquee", state: next };
    setMarquee(next);
    if (!next.active) return;
    let selectedByBox: SequenceSelection;
    if (next.mode === "marks") selectedByBox = selectionFromMarqueeMarks(visibleMarkCollections, { startX: anchorX, startY: anchorY, x: next.x, y: next.y }, left, viewport);
    else if (next.mode === "span") selectedByBox = selectionFromTimeSpan(document, next.anchorSeconds, viewport.scrollXSeconds + (next.x - left) / viewport.pxPerSecond);
    else {
      // The layout of the whole sequence arrives in the render after the box opens.
      if (marqueeContent === null) return;
      const box: MarqueeBox = {
        startX: left + next.anchorSeconds * viewport.pxPerSecond,
        startY: top + next.anchorY,
        x: next.x + viewport.scrollXSeconds * viewport.pxPerSecond,
        y: next.y + viewport.scrollY
      };
      selectedByBox = selectionFromMarqueeClips(marqueeContent, box);
    }
    const merged = mergeSequenceSelection(sequenceSelectionRef.current, selectedByBox, next.shift, next.ctrl);
    // Most moves keep the box's contents; those leave the editor's state alone.
    if (sequenceSelectionsEqual(sequenceSelectionRef.current, merged)) return;
    updateSequenceSelection(merged);
    const startSeconds = selectionStartSeconds(document, merged);
    if (startSeconds !== null) setSelectedTimeSeconds(startSeconds);
  };
  // Scrolling moves the box's anchor with the timeline, so the box selects again.
  const reapplyMarquee = useEffectEvent(() => {
    const current = drag.current;
    if (current?.kind === "marquee") applyMarquee(current.state);
  });
  useEffect(() => { reapplyMarquee(); }, [marqueeContent, viewport]);
  const visibleRasterClips = useMemo(() => {
    return visibleClips
      .filter((clip) => clip.clip.content.type === "effect" && clip.rect.x + clip.rect.width >= left && clip.rect.x <= canvasSize.width && clip.rect.y + clip.rect.height >= top && clip.rect.y <= canvasSize.height);
  }, [canvasSize.height, canvasSize.width, left, top, visibleClips]);
  const visibleEnvelopes = useMemo(() => visibleClips.filter((layout) => layout.clip.content.type === "envelope"), [visibleClips]);
  const clipRasters = useSequenceClipRasters(document, visibleRasterClips);
  /** The slice tool's cut line, while the tool is on. */
  const activeSlice = () => tool === "slice" ? sliceGuideRef.current : null;
  // The clips the cut line crosses, each strictly inside its span.
  const sliceTargetsOf = (slice: SliceGuide | null) => {
    if (slice === null) return [];
    const fromY = Math.min(slice.fromY, slice.toY);
    const toY = Math.max(slice.fromY, slice.toY);
    return visibleClips.filter((clip) =>
      clip.rect.y <= toY && clip.rect.y + clip.rect.height >= fromY
      && clip.clip.placement.startSeconds < slice.seconds && slice.seconds < clip.clip.placement.startSeconds + clip.clip.placement.durationSeconds);
  };
  const selectedClipIds = useMemo(() => new Set<number>(sequenceSelection?.type === "clips" ? sequenceSelection.ids : []), [sequenceSelection]);
  // The clips the focused or hovered envelope drives.
  const automationTargetClipIds = (hover: SequenceHover) => {
    const envelopeIds = new Set<number>();
    const focused = focusedClipId(selected);
    if (focused !== null) envelopeIds.add(focused);
    if (hover?.kind === "clip") envelopeIds.add(hover.clipId);
    const clipIds = new Set<number>();
    for (const clip of document.clips) {
      if (!envelopeIds.has(clip.id)) continue;
      for (const binding of envelopeOf(clip)?.bindings ?? []) {
        if (binding.target.type === "clipParam") clipIds.add(binding.target.clipId);
      }
    }
    return clipIds;
  };
  const selectedMarks = useMemo(
    () => markRefLookup(sequenceSelection?.type === "marks" ? sequenceSelection.marks : []),
    [sequenceSelection]
  );

  const paint = () => {
    const target = canvas.current;
    if (!target) return;
    const hover = hoverRef.current;
    const rowResizeHover = rowResizeHoverRef.current;
    const stripResizeHover = stripResizeHoverRef.current;
    const gutterResizeHover = gutterResizeHoverRef.current;
    const marquee = marqueeRef.current;
    const slice = activeSlice();
    const sliceTargets = sliceTargetsOf(slice);
    const activeAutomationTargetClipIds = automationTargetClipIds(hover);
    target.style.cursor = canvasCursor(slice);
    const rect = target.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    target.width = Math.max(1, Math.floor(rect.width * dpr));
    target.height = Math.max(1, Math.floor(rect.height * dpr));
    const ctx = target.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, rect.width, rect.height);
    ctx.fillStyle = SEQUENCE_COLORS.page;
    ctx.fillRect(0, 0, rect.width, rect.height);
    ctx.font = THEME_TYPOGRAPHY.sequence;

    const timelineWidth = Math.max(1, rect.width - left);
    const totalLaneHeight = expandedTimelineHeight(rows);
    const maxScrollXSeconds = Math.max(0, document.durationSeconds - timelineWidth / viewport.pxPerSecond);
    const timelineHeight = Math.max(0, rect.height - scrollbarHeight);
    const maxScrollY = Math.max(0, totalLaneHeight - Math.max(1, timelineHeight - top));
    const scrollXSeconds = clamp(viewport.scrollXSeconds, 0, maxScrollXSeconds);
    const scrollY = clamp(viewport.scrollY, 0, maxScrollY);

    ctx.fillStyle = SEQUENCE_COLORS.panel;
    ctx.fillRect(0, 0, left, rect.height);
    ctx.fillStyle = SEQUENCE_COLORS.page;
    ctx.fillRect(left, top, timelineWidth, rect.height - top);

    ctx.save();
    ctx.beginPath();
    ctx.rect(0, top, rect.width, rect.height - top);
    ctx.clip();
    rows.forEach((row, index) => {
      if (row.height === 0) return;
      const y = top + row.top - scrollY;
      if (y > rect.height || y + row.height < top) return;
      ctx.fillStyle = index % 2 === 0 ? SEQUENCE_COLORS.page : SEQUENCE_COLORS.laneAlt;
      ctx.fillRect(left, y, timelineWidth, row.height);
      if (row.kind === "clips" && selectedLaneIndex === row.laneIndex) {
        ctx.fillStyle = SEQUENCE_COLORS.laneSelected;
        ctx.fillRect(left, y, timelineWidth, row.height);
      }
      ctx.fillStyle = SEQUENCE_COLORS.panel;
      ctx.fillRect(0, y, left, row.height);
      ctx.strokeStyle = SEQUENCE_COLORS.grid;
      ctx.beginPath();
      ctx.moveTo(0, y + row.height - THEME_METRICS.visualHairlineOffset);
      ctx.lineTo(rect.width, y + row.height - THEME_METRICS.visualHairlineOffset);
      ctx.stroke();
      ctx.fillStyle = row.kind === "clips" ? SEQUENCE_COLORS.textMuted : SEQUENCE_COLORS.automation;
      const lane = document.targets[row.laneIndex];
      if (lane === undefined) throw new Error("Timeline row has no lane.");
      const label = row.kind === "clips" ? lane.label : "Envelopes";
      const labelLayout = laneLabelLayout(lane);
      const labelY = y + row.height / 2;
      if (row.kind === "clips" && lane.kind === "group") {
        drawDisclosure(ctx, labelLayout.disclosureX, labelY, collapsedGroups.has(lane.target.fixture));
      }
      const shared = row.kind === "clips" && lane.occurrences > 1;
      const badgeX = sharedBadgeX(left);
      ctx.fillText(fitCanvasLabel(ctx, label, (shared ? badgeX : left) - labelLayout.textX - THEME_METRICS.sequenceLabelX), labelLayout.textX, labelY + THEME_METRICS.sequenceLabelYOffset);
      // The same target under several groups; clicking the badge visits its next lane.
      if (shared) ctx.fillText(`×${lane.occurrences}`, badgeX, labelY + THEME_METRICS.sequenceLabelYOffset);
      if (rowResizeHover?.laneIndex === row.laneIndex && rowResizeHover.rowKind === row.kind) {
        ctx.fillStyle = SEQUENCE_COLORS.accent;
        ctx.fillRect(0, y + row.height - THEME_METRICS.sequenceLaneResizeIndicatorHeight / 2, rect.width, THEME_METRICS.sequenceLaneResizeIndicatorHeight);
      }
    });
    ctx.restore();

    ctx.strokeStyle = SEQUENCE_COLORS.border;
    ctx.beginPath();
    ctx.moveTo(left, 0);
    ctx.lineTo(left, rect.height);
    ctx.stroke();

    ctx.fillStyle = SEQUENCE_COLORS.panel;
    ctx.fillRect(0, 0, rect.width, top);
    if (document.audio?.exists === true) {
      ctx.clearRect(left, audioStripTop, timelineWidth, audioStripHeight);
    } else {
      ctx.fillStyle = SEQUENCE_COLORS.page;
      ctx.fillRect(left, audioStripTop, timelineWidth, audioStripHeight);
    }
    ctx.strokeStyle = SEQUENCE_COLORS.grid;
    ctx.beginPath();
    ctx.moveTo(left, audioStripTop + audioStripHeight / 2 + THEME_METRICS.visualHairlineOffset);
    ctx.lineTo(left + timelineWidth, audioStripTop + audioStripHeight / 2 + THEME_METRICS.visualHairlineOffset);
    ctx.stroke();
    ctx.fillStyle = SEQUENCE_COLORS.page;
    ctx.fillRect(left, audioStripBottom, timelineWidth, markRulerHeight);
    drawStripLabel(ctx, "Audio", audioStripTop, audioStripHeight, left);
    drawStripLabel(ctx, "Marks", audioStripBottom, markRulerHeight, left);
    const stripBorderWidth = THEME_METRICS.sequenceStripBorderWidth;
    ctx.fillStyle = SEQUENCE_COLORS.grid;
    ctx.strokeStyle = SEQUENCE_COLORS.grid;
    ctx.beginPath();
    for (const y of [audioStripTop, audioStripBottom]) {
      ctx.fillRect(0, y - stripBorderWidth / 2, rect.width, stripBorderWidth);
    }
    ctx.fillRect(left, top - stripBorderWidth / 2, timelineWidth, stripBorderWidth);
    ctx.moveTo(0, top + THEME_METRICS.visualHairlineOffset);
    ctx.lineTo(left, top + THEME_METRICS.visualHairlineOffset);
    ctx.stroke();

    if (stripResizeHover !== null) {
      const indicatorHeight = THEME_METRICS.sequenceLaneResizeIndicatorHeight;
      ctx.fillStyle = SEQUENCE_COLORS.accent;
      ctx.fillRect(0, stripResizeHover === "audio" ? audioStripBottom - indicatorHeight : top - indicatorHeight / 2, rect.width, indicatorHeight);
    }
    drawTimelineGrid(ctx, left, top, rect.width, rect.height, viewport.pxPerSecond, scrollXSeconds, document.frameRate);
    drawSequenceMarks(
      ctx,
      visibleMarkCollections,
      targetMarkCollectionKey,
      selectedMarks,
      marksLaneOnly,
      left,
      audioStripTop,
      audioStripBottom,
      markRulerHeight,
      timelineWidth,
      rect.height,
      viewport.pxPerSecond,
      scrollXSeconds,
      committedMarkDrafts(visibleMarkCollections, markDrafts)
    );

    ctx.save();
    ctx.beginPath();
    ctx.rect(left, top, timelineWidth, rect.height - top);
    ctx.clip();
    const markXs = markTimes.map((time) => left + (time - scrollXSeconds) * viewport.pxPerSecond);
    for (const clip of visibleClips) {
      if (clip.rect.x + clip.rect.width < left || clip.rect.x > rect.width || clip.rect.y + clip.rect.height < top || clip.rect.y > rect.height) {
        continue;
      }
      const hoverResize = hover?.kind === "clip" && hover.clipId === clip.clip.id ? hover.resize : null;
      const envelope = envelopeOf(clip.clip);
      if (envelope !== null) {
        const activePointIndex = drag.current?.kind === "automationPoint" && drag.current.clipId === clip.clip.id ? drag.current.index : null;
        drawEnvelope(ctx, clip, {
          label: envelopeLabel(document, envelope.bindings),
          selected: selectedClipIds.has(clip.clip.id),
          hovered: hoverResize !== null,
          choosing: bindingChooser !== null,
          resize: bindingChooser === null ? hoverResize ?? "none" : "none",
          activePointIndex,
          guide: automationCurveDraft?.id === clip.clip.id ? automationCurveDraft.guide : null,
          markXs
        });
        continue;
      }
      ctx.fillStyle = SEQUENCE_COLORS.textFaint;
      ctx.fillRect(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
      const raster = clipRasters.raster(clip.clip.id);
      if (raster !== undefined) {
        drawClipRaster(ctx, raster, clip.rect);
      }
      const automationTargeted = activeAutomationTargetClipIds.has(clip.clip.id);
      if (automationTargeted) {
        ctx.fillStyle = SEQUENCE_COLORS.accentSubtle;
        ctx.fillRect(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
      }
      if (hoverResize !== null) {
        ctx.fillStyle = SEQUENCE_COLORS.overlay;
        ctx.fillRect(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
      }
      const clipSelected = selectedClipIds.has(clip.clip.id);
      ctx.strokeStyle = clipSelected ? SEQUENCE_COLORS.clipSelected : hoverResize !== null ? SEQUENCE_COLORS.clipHover : automationTargeted ? SEQUENCE_COLORS.accent : SEQUENCE_COLORS.clipBorder;
      ctx.lineWidth = clipSelected || hoverResize !== null || automationTargeted ? THEME_METRICS.visualLineWidthStrong : THEME_METRICS.visualLineWidth;
      ctx.strokeRect(clip.rect.x + THEME_METRICS.visualHairlineOffset, clip.rect.y + THEME_METRICS.visualHairlineOffset, Math.max(0, clip.rect.width - THEME_METRICS.visualLineWidth), Math.max(0, clip.rect.height - THEME_METRICS.visualLineWidth));
      if (hoverResize === "left" || hoverResize === "right") {
        const handleX = hoverResize === "left" ? clip.rect.x : clip.rect.x + clip.rect.width;
        ctx.fillStyle = SEQUENCE_COLORS.warning;
        ctx.fillRect(handleX - THEME_METRICS.sequenceClipHandleHalfWidth, clip.rect.y + THEME_METRICS.sequenceClipHandleInset, THEME_METRICS.sequenceClipHandleHalfWidth * 2, Math.max(THEME_METRICS.sequenceClipHandleHeight, clip.rect.height - THEME_METRICS.sequenceClipHandleInset * 2));
      }
    }
    if (slice !== null) {
      const sliceX = left + (slice.seconds - scrollXSeconds) * viewport.pxPerSecond;
      ctx.strokeStyle = SEQUENCE_COLORS.sliceGuide;
      ctx.lineWidth = THEME_METRICS.visualLineWidth;
      ctx.setLineDash([THEME_METRICS.sequenceMarkerDashSize, THEME_METRICS.sequenceMarkerDashSize]);
      ctx.beginPath();
      ctx.moveTo(sliceX + THEME_METRICS.visualHairlineOffset, top);
      ctx.lineTo(sliceX + THEME_METRICS.visualHairlineOffset, rect.height);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.strokeStyle = SEQUENCE_COLORS.sliceLine;
      ctx.lineWidth = THEME_METRICS.visualLineWidthStrong;
      ctx.beginPath();
      for (const clip of sliceTargets) {
        ctx.moveTo(sliceX, clip.rect.y);
        ctx.lineTo(sliceX, clip.rect.y + clip.rect.height);
      }
      ctx.stroke();
    }
    ctx.restore();

    if (selectedTimeSeconds !== null) {
      const selectedX = left + (clamp(selectedTimeSeconds, 0, document.durationSeconds) - scrollXSeconds) * viewport.pxPerSecond;
      if (selectedX >= left && selectedX <= rect.width) {
        ctx.strokeStyle = SEQUENCE_COLORS.markMarquee;
        ctx.lineWidth = THEME_METRICS.visualLineWidth;
        ctx.beginPath();
        ctx.moveTo(selectedX + THEME_METRICS.visualHairlineOffset, top);
        ctx.lineTo(selectedX + THEME_METRICS.visualHairlineOffset, rect.height);
        ctx.stroke();
      }
    }
    if (marquee?.active === true) {
      const clipBox = marquee.mode !== "marks";
      const anchorX = left + (marquee.anchorSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond;
      const anchorY = clipBox ? top + marquee.anchorY - viewport.scrollY : marquee.anchorY;
      // A span covers every lane, so it spans the whole timeline height.
      const box = marquee.mode === "span" ? normalizedRect(anchorX, top, marquee.x, rect.height) : normalizedRect(anchorX, anchorY, marquee.x, marquee.y);
      // The box is drawn whole and clipped to the timeline, so edges scrolled out of view are not drawn.
      ctx.save();
      ctx.beginPath();
      ctx.rect(left, clipBox ? top : 0, rect.width - left, rect.height);
      ctx.clip();
      ctx.fillStyle = marquee.mode === "marks" ? SEQUENCE_COLORS.markMarqueeFill : SEQUENCE_COLORS.clipMarqueeFill;
      ctx.strokeStyle = marquee.mode === "marks" ? SEQUENCE_COLORS.markMarquee : SEQUENCE_COLORS.warning;
      ctx.lineWidth = THEME_METRICS.visualLineWidth;
      ctx.fillRect(box.x, box.y, box.width, box.height);
      ctx.strokeRect(box.x + THEME_METRICS.visualHairlineOffset, box.y + THEME_METRICS.visualHairlineOffset, Math.max(0, box.width - THEME_METRICS.visualLineWidth), Math.max(0, box.height - THEME_METRICS.visualLineWidth));
      ctx.restore();
    }
    if (gutterResizeHover) {
      const indicatorWidth = THEME_METRICS.sequenceLaneResizeIndicatorHeight;
      ctx.fillStyle = SEQUENCE_COLORS.accent;
      ctx.fillRect(left - indicatorWidth / 2, 0, indicatorWidth, rect.height);
    }
  };
  // Every render paints with its state; pointer moves between renders paint through `paintRef`.
  useEffect(() => {
    paintRef.current = paint;
    paint();
  });

  const seekTimeFromCanvasX = (x: number) =>
    clamp(Math.round((viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond) / SEQUENCE_CANVAS.scrubStepSeconds) * SEQUENCE_CANVAS.scrubStepSeconds, 0, document.durationSeconds);
  const seekFromCanvas = (event: MouseEvent<HTMLCanvasElement>) => {
    const positionSeconds = seekTimeFromCanvasX(Math.max(left, event.nativeEvent.offsetX));
    void runSnapshotCommand(() => commands.audioSeek(positionSeconds));
  };
  const timeFromCanvasX = (x: number) => clamp(roundToNanosecond(viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond), 0, document.durationSeconds);
  /** Range edges and the cut line snap to marks like other timeline drags. */
  const rangeTimeFromCanvasX = (x: number, snap: boolean) => {
    const seconds = timeFromCanvasX(x);
    return snap ? snapToTime(seconds, markTimes, viewport.pxPerSecond) ?? seconds : seconds;
  };
  const rangeFromDrag = (anchorSeconds: number, seconds: number): PlaybackRange =>
    ({ startSeconds: Math.min(anchorSeconds, seconds), endSeconds: Math.max(anchorSeconds, seconds) });
  /** What the canvas draws where, for hit testing a point. */
  const canvasLayout = (): CanvasLayout => ({
    left, top, audioStripTop, audioStripBottom, viewport, rows, lanes: document.targets, clips: visibleClips, envelopes: visibleEnvelopes,
    marks: visibleMarkCollections, activeMarks: targetMarkCollectionKey, transport: useAppStore.getState().snapshot?.audioTransport ?? null
  });
  /** What a drag reads as `event` moves or releases it. */
  const gestureContext = (event: { altKey: boolean; ctrlKey: boolean; metaKey: boolean }): GestureContext => ({
    document,
    selection: sequenceSelectionRef.current,
    pxPerSecond: viewport.pxPerSecond,
    laneAt: (y) => laneIndexFromCanvasY(y, top, viewport.scrollY, document.targets.length, rows),
    clipDelta: (state, x, destinationLane) => clipGestureDelta(state, x, event.altKey, destinationLane),
    envelopeResize: hasPrimaryModifier(event) ? "stretch" : "crop",
    sliced: () => sliceTargetsOf(activeSlice()).map((clip) => clip.clip.id)
  });
  const effectTree = useMemo(
    () => definitionTree(document.effectDefinitions, (definition) => definition.effect.path),
    [document.effectDefinitions]
  );
  /**
   * The collection a new mark goes into: the given one, else the active one, creating a collection
   * when the sequence has none. It reads the latest document, so queued taps see earlier taps' edits.
   */
  const markCollectionKey = (id: number) => document.markCollections.find((collection) => collection.id === id)?.name ?? null;
  const markCollectionForEdit = async (collectionId: number | null): Promise<number | null> => {
    const collectionsNow = () => {
      const guiDocument = useAppStore.getState().guiDocument;
      if (guiDocument?.type !== "sequence") throw new Error("Marks can only be added to an open sequence.");
      return guiDocument.document.markCollections;
    };
    const collections = collectionsNow();
    const chosen = collectionId === null ? activeMarkCollection(collections, activeMarkCollectionKey) : collections.find((collection) => collection.id === collectionId);
    let key = chosen?.name ?? null;
    if (key === null) {
      const result = await runGuiEditCommand((request) =>
        commands.applyGuiEdit(request, { type: "sequence", edit: { type: "composition", edit: {
          type: "addMarkCollections",
          collections: [{ name: "Marks", color: defaultMarkColor(collections.length), marksSeconds: [] }]
        } } })
      );
      if (result === null) return null;
      key = result.created[0] ?? null;
      if (key === null) throw new Error("Creating a mark collection reported no collection.");
      showMarks([key], true);
    } else if (!visibleMarkCollectionKeys.has(key)) showMarks([key], false);
    const name = key;
    const collection = collectionsNow().find((candidate) => candidate.name === name);
    if (collection === undefined) throw new Error("The mark collection was not found.");
    return collection.id;
  };
  const addClipFromContextMenu = async (definition: SequenceEffectDefinition, menu: LaneContextMenu) => {
    const target = document.targets[menu.laneIndex]?.target ?? document.targets[0]?.target;
    if (target === undefined) return;
    const scope: SequenceClipScope = "wholeTarget";
    await runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "addClip",
        target,
        startSeconds: menu.startSeconds,
        durationSeconds: 1,
        content: { type: "effect", initialColor: THEME_COLORS.white, effect: definition.effect, scope }
      } })
    );
  };
  const addMark = async (collectionId: number | null, timeSeconds: number) => {
    const id = await markCollectionForEdit(collectionId);
    if (id === null) return;
    await runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "addMarks",
        collectionId: id,
        timesSeconds: [timeSeconds]
      } })
    );
  };
  // Taps run in order, so fast tapping creates a missing collection once.
  const tapQueue = useRef<Promise<void>>(Promise.resolve());
  const tapMark = () => {
    const clock = playheadClockRef.current;
    if (clock === null) throw new Error("A mark was tapped without a playhead.");
    const timeSeconds = clock();
    tapQueue.current = tapQueue.current
      .then(() => addMark(null, timeSeconds))
      // A tap on a document that is no longer a sequence is dropped; later taps still run.
      .catch((error: unknown) => { console.error(error); });
  };
  const addEnvelopeFromContextMenu = async (menu: LaneContextMenu) => {
    await runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "addClip",
        target: targetAtLane(document, menu.laneIndex),
        startSeconds: menu.startSeconds,
        durationSeconds: Math.min(2, Math.max(MIN_CLIP_DURATION_SECONDS, document.durationSeconds - menu.startSeconds)),
        content: { type: "envelope" }
      } })
    );
  };
  const bindEnvelope = (clip: number) => {
    if (bindingChooser === null) return;
    const chooser = bindingChooser;
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "bind",
        clip,
        target: chooser.target
      } })
    ).then((result) => {
      if (result !== null) setDocumentUi({ bindingChooser: null });
    });
  };
  const deleteItems = async (selection: NonNullable<SequenceSelection>) => {
    const result = await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "sequence", edit: { type: "deleteSelection", selection } }));
    if (result === null) return;
    updateSequenceSelection(result.selection);
  };

  const applySequenceEdit = (edit: SequenceGuiEdit, origin?: GuiDocumentRequest | null) =>
    runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "sequence", edit }), origin);
  const splitClips = async (clipIds: number[], timeSeconds: number) => {
    await applySequenceEdit({ type: "splitClips", ids: clipIds, timeSeconds });
  };

  const commitAutomationCurve = (clipId: number, curve: SequenceCurvePoint[], origin?: GuiDocumentRequest | null) => {
    setAutomationCurveDraft({ id: clipId, curve, guide: { time: null, value: null } });
    void put(site({ type: "clip", id: clipId }, { type: "points" }), { type: "curve", points: curve }, origin ?? undefined).finally(() => {
      setAutomationCurveDraft(null);
    });
  };

  /** The clip-drag delta, snapped to marks and to the edges of unselected clips in the row the grabbed clip lands in. */
  const clipGestureDelta = (current: Extract<GestureState, { kind: "sequence" }>, x: number, altKey: boolean, destinationLane: number) => {
    const delta = roundToNanosecond((x - current.startX) / viewport.pxPerSecond);
    const selection = sequenceSelectionRef.current;
    if (!snaps(altKey) || selection?.type !== "clips") return delta;
    const clip = document.clips.filter(isTimed).find((candidate) => candidate.id === current.id);
    if (clip === undefined) return delta;
    const { startSeconds, durationSeconds } = clip.placement;
    const end = startSeconds + durationSeconds;
    const edges = current.resize === "left" ? [startSeconds] : current.resize === "right" ? [end] : [startSeconds, end];
    const rowTarget = targetAtLane(document, current.resize === "none" ? destinationLane : current.laneIndex);
    // The peers are the unselected clips of the row the grabbed one lands in.
    const peers = document.clips.filter(isTimed).filter((peer) => !selection.ids.includes(peer.id) && clipRowKind(peer) === clipRowKind(clip) && targetsEqual(peer.target, rowTarget));
    return roundToNanosecond(snapDeltaToTimes(edges, delta, [...markTimes, ...clipEdgeSnapTimes(peers.map((peer) => peer.placement))], viewport.pxPerSecond));
  };

  /** Move a curve point, aligning it to neighbors with Shift and snapping its time to marks when snapping applies. */
  const dragAutomationPoint = (layout: SequenceClipLayout, index: number, x: number, y: number, modifiers: { shiftKey: boolean; altKey: boolean }) => {
    const { clip } = layout;
    const { startSeconds, durationSeconds } = clip.placement;
    const curve = envelopeOf(clip)?.curve ?? [];
    let point = automationCurvePointFromCanvas(layout.curveRect, x, y);
    let guide: AutomationGuide = { time: null, value: null };
    if (snaps(modifiers.altKey)) {
      const mark = snapToTime(startSeconds + point.time * durationSeconds, markTimes, viewport.pxPerSecond);
      const time = mark === null ? null : (mark - startSeconds) / durationSeconds;
      if (time !== null && time >= 0 && time <= 1) {
        point = { ...point, time };
        guide = { ...guide, time };
      }
    }
    if (modifiers.shiftKey) {
      const aligned = alignAutomationCurvePoint(curve, index, point, layout.curveRect);
      point = aligned.point;
      guide = { time: aligned.guide.time ?? guide.time, value: aligned.guide.value };
    }
    setAutomationCurveDraft({ id: clip.id, curve: moveAutomationCurvePoint(curve, index, point), guide });
  };
  const retargetContextClip = async (clipId: number, target: FixtureTarget) => {
    await put(site({ type: "clip", id: clipId }, { type: "target" }), { type: "target", target });
  };
  const laneContextMenu = sequenceContextMenu?.kind === "blank" || sequenceContextMenu?.kind === "clip" ? sequenceContextMenu : null;
  const markCollectionsForMenu = () => {
    if (activeMarkCollectionKey === null) return document.markCollections;
    return [
      ...document.markCollections.filter((collection) => collection.name === activeMarkCollectionKey),
      ...document.markCollections.filter((collection) => collection.name !== activeMarkCollectionKey)
    ];
  };
  /** The context menu's items for the place right-clicked, before the clip-arranging submenus. */
  const placeMenuEntries = (menu: SequenceContextMenu): MenuEntry[] => {
    const markEntries: MenuEntry[] = document.markCollections.length === 0
      ? [{ type: "action", label: "Marks", onSelect: () => { void addMark(null, menu.startSeconds); } }]
      : markCollectionsForMenu().map((collection) => ({ type: "action", label: collection.name, color: collection.color, onSelect: () => { void addMark(collection.id, menu.startSeconds); } }));
    return [
      { type: "action", label: "Set Playhead", onSelect: () => { void runSnapshotCommand(() => commands.audioSeek(menu.startSeconds)); } },
      { type: "submenu", label: "Add Mark", entries: markEntries },
      ...(laneContextMenu === null ? [] : [{ type: "action", label: "Add Envelope", onSelect: () => { void addEnvelopeFromContextMenu(laneContextMenu); } } satisfies MenuEntry])
    ];
  };
  /** The context menu's items for the object right-clicked. */
  const objectMenuEntries = (menu: SequenceContextMenu): MenuEntry[] => {
    if (menu.kind === "clip") {
      const targets = document.targets.filter((lane, index) => document.targets.findIndex((other) => targetsEqual(other.target, lane.target)) === index);
      return [
        { type: "separator" },
        { type: "submenu", label: "Retarget", entries: targets.map((lane) => ({ type: "action", label: lane.label, onSelect: () => { void retargetContextClip(menu.clipId, lane.target); } })) },
        { type: "action", label: "Split", icon: Scissors, onSelect: () => { void splitClips([menu.clipId], menu.startSeconds); } },
        { type: "action", label: "Delete", icon: Trash2, danger: true, shortcut: "edit.delete", onSelect: () => { void deleteItems({ type: "clips", ids: [menu.clipId] }); } }
      ];
    }
    if (menu.kind === "mark") {
      return [
        { type: "separator" },
        { type: "action", label: "Delete", icon: Trash2, danger: true, shortcut: "edit.delete", onSelect: () => { void deleteItems({ type: "marks", marks: [{ collectionId: menu.collectionId, index: menu.index }] }); } }
      ];
    }
    return [];
  };

  const selectedMark = selected?.type === "mark" ? { collectionId: selected.collectionId, index: selected.index } : null;
  const activeSelection = sequenceSelection;
  const editable = () => !useAppStore.getState().guiEditPending;
  const hasSelection = () => editable() && activeSelection !== null && selectionCount(activeSelection) > 0;

  const clipMatch = settings?.sequenceClipMatch ?? null;
  const [clipMatchSeed, setClipMatchSeed] = useState<ClipMatchSeed | null>(null);
  const matchClips = useCallback<MatchClips>((seed, criteria, params) => {
    const request = useAppStore.getState().guiRequest;
    if (request === null) return Promise.reject(new Error("No sequence is open."));
    return commands.matchSequenceClips(request, { ...seed, criteria, params });
  }, []);
  /** "Select Matching" seeds from the selected clips. */
  const selectedClipSeed = (): ClipMatchSeed | null => {
    const selection = sequenceSelectionRef.current;
    return selection?.type === "clips" && selectionCount(selection) > 0
      ? { ids: selection.ids }
      : null;
  };
  const selectMatching = (seed: ClipMatchSeed, criteria: SequenceClipMatch, params: SequenceClipParamMatch[]) => {
    const request = useAppStore.getState().guiRequest;
    matchClips(seed, criteria, params).then((selection) => {
      // A result for a sequence that is no longer open names its clips, not these.
      if (!sameGuiDocument(useAppStore.getState().guiRequest, request)) return;
      updateSequenceSelection(selection);
    }).catch((error: unknown) => { useAppStore.getState().reportFailure("command", errorMessage(error)); });
  };
  const [cascadeOpen, setCascadeOpen] = useState(false);
  const arrangeClips = (arrangement: SequenceClipArrangement) => {
    const selection = sequenceSelectionRef.current;
    if (selection?.type !== "clips" || selectionCount(selection) === 0) return;
    void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "sequence", edit: { type: "arrangeClips", ids: selection.ids, arrangement } })).then((result) => {
      if (result !== null) updateSequenceSelection(result.selection);
    });
  };
  const saveClipMatch = (criteria: SequenceClipMatch) => {
    const current = useAppStore.getState().snapshot?.settings;
    if (current === undefined) return;
    void runSnapshotCommand(() => commands.updateAppSettings({ ...current, sequenceClipMatch: criteria }));
  };
  const transferSelection = (type: "copy" | "cut") => {
    if (activeSelection === null) return;
    void runGuiEditCommand((request) => type === "copy"
      ? commands.copySequenceSelection(request, activeSelection)
      : commands.cutSequenceSelection(request, activeSelection)).then((result) => {
      if (result === null) return;
      updateSequenceSelection(result.selection);
    });
  };
  const paste = () => {
    const focused = document.clips.filter(isTimed).find((clip) => clip.id === focusedClipId(selected));
    const lane = selectedLaneIndex ?? (focused === undefined ? -1 : nearestLane(document, focused.target, 0));
    const markTime = selectedMark === null ? undefined : document.markCollections.find((collection) => collection.id === selectedMark.collectionId)?.marksSeconds[selectedMark.index];
    const timeSeconds = selectedTimeSeconds ?? focused?.placement.startSeconds ?? markTime ?? 0;
    void runGuiEditCommand((request) => commands.pasteSequence(request, { lane: lane < 0 ? null : lane, timeSeconds })).then((result) => {
      // A paste that placed nothing keeps the selection.
      if (result === null || result.selection === null) return;
      updateSequenceSelection(result.selection);
    });
  };
  const deleteSelection = () => {
    if (activeSelection !== null) void deleteItems(activeSelection);
  };
  const nudgeMark = (mark: NonNullable<typeof selectedMark>, direction: -1 | 1, shift: boolean) => {
    const collection = document.markCollections.find((candidate) => candidate.id === mark.collectionId);
    const timeSeconds = collection?.marksSeconds[mark.index];
    if (collection === undefined || timeSeconds === undefined) return;
    const deltaSeconds = direction * (shift ? SEQUENCE_CANVAS.shiftedNudgeSeconds : SEQUENCE_CANVAS.nudgeSeconds);
    const nextTimeSeconds = clamp(timeSeconds + deltaSeconds, 0, document.durationSeconds);
    const nextIndex = markIndexAfterMove(collection, mark.index, nextTimeSeconds);
    const nextDrafts: MarkDraftLookup = new Map();
    setMarkDraft(nextDrafts, mark, { collectionId: mark.collectionId, index: mark.index, timeSeconds: nextTimeSeconds, committedIndex: nextIndex });
    setMarkDrafts(nextDrafts);
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: {
        type: "moveMarks",
        marks: [mark],
        timeDeltaSeconds: nextTimeSeconds - timeSeconds
      } })
    ).then((result) => {
      setMarkDrafts(new Map());
      if (result !== null) setSequenceSelection(result.selection);
    });
  };
  const transportCommands = sequenceTransportCommands(document);
  // Arrow keys move a selected mark; otherwise they step the playhead.
  const arrowCommand = (direction: -1 | 1) => selectedMark === null
    ? transportCommands[direction < 0 ? "edit.moveLeft" : "edit.moveRight"]
    : { enabled: editable, run: ({ shift }: { shift: boolean }) => { nudgeMark(selectedMark, direction, shift); } };
  // = and - zoom about the playhead, or about the view's center while the playhead is off screen.
  const zoomTimeline = (direction: -1 | 1) => () => {
    const target = canvas.current;
    if (target === null) return;
    const timelineWidth = Math.max(1, target.getBoundingClientRect().width - left);
    const playheadSeconds = useAppStore.getState().snapshot?.audioTransport.positionSeconds;
    setViewport((current) => {
      const playheadX = playheadSeconds === undefined ? -1 : (playheadSeconds - current.scrollXSeconds) * current.pxPerSecond;
      const anchorX = playheadX >= 0 && playheadX <= timelineWidth ? playheadX : timelineWidth / 2;
      const anchorTime = current.scrollXSeconds + anchorX / current.pxPerSecond;
      const pxPerSecond = clamp(
        current.pxPerSecond * SEQUENCE_CANVAS.keyZoomFactor ** direction,
        minSequencePxPerSecond(timelineWidth, document.durationSeconds),
        SEQUENCE_CANVAS.maxZoomPxPerSecond
      );
      return { ...current, pxPerSecond, scrollXSeconds: clamp(anchorTime - anchorX / pxPerSecond, 0, Math.max(0, document.durationSeconds - timelineWidth / pxPerSecond)) };
    });
  };
  useEditorCommands(canvas, {
    ...transportCommands,
    "view.zoomIn": { run: zoomTimeline(1) },
    "view.zoomOut": { run: zoomTimeline(-1) },
    "edit.cancel": {
      enabled: () => bindingChooser !== null || tool === "slice" || (useAppStore.getState().snapshot?.audioTransport.range ?? null) !== null,
      run: () => {
        if (bindingChooser !== null) setDocumentUi({ bindingChooser: null });
        else if (tool === "slice") setTool("select");
        else void runSnapshotCommand(() => commands.audioSetRange(null));
      }
    },
    "edit.copy": { enabled: hasSelection, run: () => { transferSelection("copy"); } },
    "edit.cut": { enabled: hasSelection, run: () => { transferSelection("cut"); } },
    "edit.paste": { enabled: editable, run: paste },
    "edit.delete": { enabled: editable, run: deleteSelection },
    "edit.selectAll": {
      run: () => {
        const all: NonNullable<SequenceSelection> = { type: "clips", ids: document.clips.map((clip) => clip.id) };
        updateSequenceSelection(selectionCount(all) > 0 ? all : null);
      }
    },
    "edit.moveLeft": arrowCommand(-1),
    "edit.moveRight": arrowCommand(1),
    // M drops a mark at the playhead, so marks can be tapped along with the music.
    "transport.tapMark": { enabled: transportCommands["transport.togglePlayback"].enabled, run: ({ repeat }) => { if (!repeat) tapMark(); } },
    "sequence.toggleSliceTool": { run: toggleSliceTool },
    "sequence.selectMatching": {
      enabled: () => clipMatch !== null && selectedClipSeed() !== null,
      run: () => {
        const seed = selectedClipSeed();
        if (seed !== null && clipMatch !== null) selectMatching(seed, clipMatch, []);
      }
    }
  });

  return (
    <div className="sequence-canvas-shell">
      <ContextMenu.Root onOpenChange={(open) => { if (!open) setSequenceContextMenu(null); }}>
        <ContextMenu.Trigger asChild>
          <canvas
            ref={canvas}
            className="gui-canvas"
            style={{
              clipPath: `polygon(0 0, 100% 0, 100% calc(100% - var(--donder-scrollbar-width)), ${left}px calc(100% - var(--donder-scrollbar-width)), ${left}px 100%, 0 100%)`
            }}
            tabIndex={0}
      onContextMenu={(event) => {
        if (suppressContextMenu.current) {
          suppressContextMenu.current = false;
          event.preventDefault();
          return;
        }
        const x = event.nativeEvent.offsetX;
        const y = event.nativeEvent.offsetY;
        const target = bindingChooser === null ? hit(canvasLayout(), { x, y }, { curves: true, envelopesOnly: false }) : { type: "none" as const };
        const startSeconds = timeFromCanvasX(x);
        switch (target.type) {
          case "markRuler":
            setSelectedTimeSeconds(startSeconds);
            setSequenceContextMenu({ kind: "markRuler", startSeconds });
            return;
          case "mark": {
            const { mark } = target;
            updateSequenceSelection({ type: "marks", marks: [{ collectionId: mark.collectionId, index: mark.index }] });
            setDocumentUi({ activeMarks: markCollectionKey(mark.collectionId) });
            setSelectedTimeSeconds(mark.timeSeconds);
            setSequenceContextMenu({ kind: "mark", startSeconds, collectionId: mark.collectionId, index: mark.index });
            return;
          }
          case "curve":
          case "clip":
          case "lane":
          case "curvePoint": {
            const laneIndex = target.type === "lane" ? target.laneIndex : target.type === "curvePoint" ? target.layout.laneIndex : target.hit.laneIndex;
            setSelectedLaneIndex(laneIndex);
            setSelectedTimeSeconds(startSeconds);
            // Right-clicking a curve point erases it (pointer events do that), so no menu opens over it.
            if (target.type === "curvePoint") break;
            if (target.type === "lane") {
              updateSequenceSelection(null);
              setSequenceContextMenu({ kind: "blank", laneIndex, startSeconds });
              return;
            }
            const id = target.hit.clip.id;
            const active = sequenceSelectionRef.current;
            updateSequenceSelection(active?.type === "clips" && active.ids.includes(id) ? active : { type: "clips", ids: [id] });
            setSequenceContextMenu({ kind: "clip", laneIndex, startSeconds, clipId: id });
            return;
          }
          case "seek":
          case "none":
          case "stripResize":
          case "gutterResize":
          case "rowResize":
          case "sharedBadge":
          case "disclosure":
          case "rowLabel":
        }
        event.preventDefault();
        setSequenceContextMenu(null);
      }}
      onDoubleClick={(event) => {
        const x = event.nativeEvent.offsetX;
        const y = event.nativeEvent.offsetY;
        if (tool === "slice" && y >= top) return;
        // Double-clicking empty mark ruler adds a mark to the active collection.
        if (hit(canvasLayout(), { x, y }, { curves: false, envelopesOnly: false }).type === "markRuler") void addMark(null, timeFromCanvasX(x));
      }}
      onPointerDown={(event) => {
        gestureRequest.current = useAppStore.getState().guiRequest;
        suppressContextMenu.current = false;
        const x = event.nativeEvent.offsetX;
        const y = event.nativeEvent.offsetY;
        const layout = canvasLayout();
        if (isSecondaryClick(event) && bindingChooser === null) {
          const sweep = hit(layout, { x, y }, { curves: true, envelopesOnly: false });
          const sweptCurve = sweep.type === "curvePoint" ? envelopeOf(sweep.layout.clip)?.curve ?? [] : [];
          if (sweep.type === "curvePoint" && sweptCurve.length > 1) {
            event.currentTarget.setPointerCapture(event.pointerId);
            drag.current = { kind: "automationErase", clipId: sweep.layout.clip.id, laneIndex: sweep.layout.laneIndex };
            setAutomationCurveDraft({ id: sweep.layout.clip.id, curve: removeAutomationCurvePoint(sweptCurve, sweep.index), guide: { time: null, value: null } });
            return;
          }
        }
        if (event.button !== 0 || isSecondaryClick(event)) return;
        event.currentTarget.focus();
        event.currentTarget.setPointerCapture(event.pointerId);
        setMarkDrafts(new Map());
        setBadgeHover(null);
        // A curve is edited by a plain click on one clip.
        const editsCurve = !event.shiftKey && !event.ctrlKey && !event.metaKey && (sequenceSelection?.type !== "clips" || selectionCount(sequenceSelection) <= 1);
        const target = hit(layout, { x, y }, { curves: editsCurve, envelopesOnly: bindingChooser !== null });
        if (target.type === "stripResize") {
          event.preventDefault();
          drag.current = { kind: "stripResize", strip: target.strip, startY: y, initialHeight: viewport[TIMELINE_STRIPS[target.strip].height], active: false };
          setStripResizeHover(target.strip);
          return;
        }
        if (target.type === "gutterResize") {
          event.preventDefault();
          drag.current = { kind: "gutterResize", startX: x, initialWidth: viewport.leftGutterWidth, active: false };
          setGutterResizeHover(true);
          return;
        }
        if (bindingChooser !== null) {
          if (target.type === "clip") {
            event.preventDefault();
            event.stopPropagation();
            bindEnvelope(target.hit.clip.id);
          }
          return;
        }
        switch (target.type) {
          case "seek": {
            setSeekHover(null);
            if (target.target === "playhead") {
              drag.current = { kind: "playheadScrub" };
              setDragCursor("grabbing");
              seekFromCanvas(event);
              return;
            }
            const range = useAppStore.getState().snapshot?.audioTransport.range ?? null;
            if (target.target === "rangeEdge" && range !== null) {
              const fromEnd = Math.abs(timeFromCanvasX(x) - range.endSeconds) <= Math.abs(timeFromCanvasX(x) - range.startSeconds);
              drag.current = { kind: "timeRange", startX: x, anchorSeconds: fromEnd ? range.startSeconds : range.endSeconds, fromEdge: true, active: false };
              return;
            }
            drag.current = { kind: "timeRange", startX: x, anchorSeconds: rangeTimeFromCanvasX(x, snaps(event.altKey)), fromEdge: false, active: false };
            return;
          }
          case "mark": {
            const { mark: markHit } = target;
            const mark = { collectionId: markHit.collectionId, index: markHit.index };
            const activeSelection = sequenceSelectionRef.current;
            const wasAlreadySelected = activeSelection?.type === "marks" && activeSelection.marks.some((candidate) => candidate.collectionId === mark.collectionId && candidate.index === mark.index);
            const nextSelection = wasAlreadySelected && !event.shiftKey && !event.ctrlKey && !event.metaKey
              ? activeSelection
              : nextMarkSelection(activeSelection?.type === "marks" ? activeSelection : null, mark, event.shiftKey, event.ctrlKey || event.metaKey);
            updateSequenceSelection(nextSelection);
            setDocumentUi({ activeMarks: markCollectionKey(markHit.collectionId) });
            setSelectedTimeSeconds(markHit.timeSeconds);
            drag.current = { kind: "mark", collectionId: markHit.collectionId, index: markHit.index, startX: x, startY: y, active: false, originalTimeSeconds: markHit.timeSeconds };
            return;
          }
          case "markRuler": {
            // Dragging across empty ruler box-selects marks by time.
            setSelectedTimeSeconds(timeFromCanvasX(x));
            updateSequenceSelection(null);
            const state = { mode: "marks" as const, anchorSeconds: timeFromCanvasX(x), anchorY: y, x, y, active: false, shift: event.shiftKey, ctrl: event.ctrlKey || event.metaKey };
            drag.current = { kind: "marquee", state };
            setMarquee(state);
            return;
          }
          case "rowResize":
            event.preventDefault();
            drag.current = {
              kind: "rowResize",
              laneIndex: target.laneIndex,
              rowKind: target.rowKind,
              startY: y,
              initialHeight: rowHeightAt(viewport.rowHeights, targetAtLane(document, target.laneIndex), target.rowKind, target.rowKind === "clips" ? initialSequenceLaneHeight(settings) : automationRowHeight),
              active: false
            };
            setRowResizeHover({ laneIndex: target.laneIndex, rowKind: target.rowKind });
            return;
          case "sharedBadge": {
            const lane = document.targets[target.laneIndex];
            if (lane === undefined) return;
            const copies = lanesForTarget(document, lane.target);
            const next = copies.find((copy) => copy > target.laneIndex) ?? copies[0] ?? target.laneIndex;
            const nextRow = rows.find((candidate) => candidate.laneIndex === next && candidate.kind === "clips");
            setSelectedLaneIndex(next);
            if (nextRow !== undefined) {
              const maxScrollY = Math.max(0, expandedTimelineHeight(rows) - Math.max(1, canvasSize.height - top));
              setViewport((previous) => ({ ...previous, scrollY: clamp(nextRow.top, 0, maxScrollY) }));
            }
            return;
          }
          case "disclosure": {
            const group = targetAtLane(document, target.laneIndex).fixture;
            const next = new Set(collapsedGroups);
            if (!next.delete(group)) next.add(group);
            saveCollapsedGroups(next);
            return;
          }
          case "rowLabel": {
            const { row } = target;
            const laneTarget = targetAtLane(document, target.laneIndex);
            const rowSelection: SequenceSelection = { type: "clips", ids: row === null ? [] : document.clips.filter((clip) => clipRowKind(clip) === row.kind && targetsEqual(clip.target, laneTarget)).map((clip) => clip.id) };
            setSelectedLaneIndex(target.laneIndex);
            updateSequenceSelection(selectionCount(rowSelection) > 0 ? rowSelection : null);
            return;
          }
          case "curve":
          case "clip":
          case "curvePoint":
          case "lane":
          case "none":
        }
        if (tool === "slice" && x >= left && y >= top) {
          event.preventDefault();
          const seconds = rangeTimeFromCanvasX(x, snaps(event.altKey));
          drag.current = { kind: "slice", seconds, startY: y };
          setSlice({ seconds, fromY: y, toY: y });
          return;
        }
        if (target.type === "curvePoint" || target.type === "curve") {
          const envelope = target.type === "curvePoint" ? target.layout : target.hit;
          updateSequenceSelection({ type: "clips", ids: [envelope.clip.id] });
          let index = target.type === "curvePoint" ? target.index : 0;
          if (target.type === "curve") {
            const added = insertAutomationCurvePoint(envelopeOf(envelope.clip)?.curve ?? [], automationCurvePointFromCanvas(envelope.curveRect, x, y));
            setAutomationCurveDraft({ id: envelope.clip.id, curve: added.curve, guide: { time: null, value: null } });
            index = added.index;
          }
          drag.current = { kind: "automationPoint", clipId: envelope.clip.id, laneIndex: envelope.laneIndex, index, active: false, inserted: target.type === "curve" };
          return;
        }
        if (target.type === "clip") {
          const { hit: clipHit } = target;
          const activeSelection = sequenceSelectionRef.current;
          const wasAlreadySelected = activeSelection?.type === "clips" && activeSelection.ids.includes(clipHit.clip.id);
          const nextSelection = wasAlreadySelected && !event.shiftKey && !event.ctrlKey && !event.metaKey
            ? activeSelection
            : nextClipSelection(activeSelection?.type === "clips" ? activeSelection : null, clipHit.clip.id, event.shiftKey, event.ctrlKey || event.metaKey);
          updateSequenceSelection(nextSelection);
          setSelectedLaneIndex(clipHit.laneIndex);
          setSelectedTimeSeconds(selectionStartSeconds(document, nextSelection) ?? clipHit.clip.placement.startSeconds);
          drag.current = { kind: "sequence", id: clipHit.clip.id, startX: x, startY: y, active: false, originalStartSeconds: clipHit.clip.placement.startSeconds, laneIndex: clipHit.laneIndex, resize: clipHit.resize };
          return;
        }
        if (target.type === "lane") {
          const timeSeconds = timeFromCanvasX(x);
          setSelectedLaneIndex(target.laneIndex);
          setSelectedTimeSeconds(timeSeconds);
          updateSequenceSelection(null);
          // Alt-drag selects every clip in the time span; a plain drag box-selects.
          const state: SequenceMarquee = { mode: event.altKey ? "span" : "clips", anchorSeconds: timeSeconds, anchorY: y - top + viewport.scrollY, x, y, active: false, shift: event.shiftKey, ctrl: event.ctrlKey || event.metaKey };
          drag.current = { kind: "marquee", state };
          setMarquee(state);
        }
      }}
      onPointerMove={(event) => {
        const x = event.nativeEvent.offsetX;
        const y = event.nativeEvent.offsetY;
        const current = drag.current;
        if (current === null) {
          const target = hit(canvasLayout(), { x, y }, { curves: true, envelopesOnly: bindingChooser !== null });
          setStripResizeHover(target.type === "stripResize" ? target.strip : null);
          setGutterResizeHover(target.type === "gutterResize");
          setSeekHover(target.type === "seek" ? { target: target.target, seconds: seekTimeFromCanvasX(x) } : null);
          if (target.type === "stripResize" || target.type === "gutterResize" || target.type === "seek") {
            setSlice(null);
            setRowResizeHover(null);
            setHover(null);
            setBadgeHover(null);
            return;
          }
          const nextBadgeHover = target.type === "sharedBadge" ? sharedBadge(target.laneIndex) : null;
          setBadgeHover((previous) => previous?.laneIndex === nextBadgeHover?.laneIndex && previous?.y === nextBadgeHover?.y && previous?.scrollY === nextBadgeHover?.scrollY && previous?.left === nextBadgeHover?.left ? previous : nextBadgeHover);
          setRowResizeHover(target.type === "rowResize" ? { laneIndex: target.laneIndex, rowKind: target.rowKind } : null);
          if (tool === "slice") {
            const inLanes = x >= left && y >= top;
            setSlice(inLanes ? { seconds: rangeTimeFromCanvasX(x, snaps(event.altKey)), fromY: y, toY: y } : null);
            if (inLanes) {
              setHover(null);
              return;
            }
          }
          setHover(
            target.type === "curvePoint" ? { kind: "clip", clipId: target.layout.clip.id, resize: "none" }
              : target.type === "clip" || target.type === "curve" ? { kind: "clip", clipId: target.hit.clip.id, resize: target.hit.resize }
                : target.type === "mark" ? { kind: "mark", collectionId: target.mark.collectionId, index: target.mark.index }
                  : null
          );
          return;
        }
        const { state, started, clips } = reduce(current, { type: "move", x, y }, gestureContext(event));
        drag.current = state;
        if (state === null) return;
        const grabs = state.kind === "stripResize" || state.kind === "gutterResize" || state.kind === "rowResize" || state.kind === "sequence";
        if (started && grabs) setDragCursor("grabbing");
        if ("active" in state && !state.active) return;
        switch (state.kind) {
          case "stripResize": {
            const strip = TIMELINE_STRIPS[state.strip];
            const height = clamp(state.initialHeight + y - state.startY, strip.minPx, strip.maxPx);
            setViewport((previous) => {
              const next = { ...previous, [strip.height]: height };
              const nextTop = SEQUENCE_CANVAS.audioStripTopPx + next.audioStripHeight + next.markRulerHeight;
              const visibleHeight = Math.max(1, canvasSize.height - nextTop);
              const maxScrollY = Math.max(0, expandedTimelineHeight(layoutRows(previous.rowHeights, revealAutomation)) - visibleHeight);
              return { ...next, scrollY: clamp(previous.scrollY, 0, maxScrollY) };
            });
            return;
          }
          case "gutterResize": {
            // The timeline keeps at least its minimum gutter's worth of width.
            const maxWidth = Math.max(SEQUENCE_CANVAS.minLeftGutterPx, Math.min(SEQUENCE_CANVAS.maxLeftGutterPx, canvasSize.width - SEQUENCE_CANVAS.minLeftGutterPx));
            const leftGutterWidth = clamp(state.initialWidth + x - state.startX, SEQUENCE_CANVAS.minLeftGutterPx, maxWidth);
            setViewport((previous) => ({ ...previous, leftGutterWidth }));
            return;
          }
          case "rowResize": {
            const nextHeight = clamp(state.initialHeight + y - state.startY, minRowHeight(state.rowKind), SEQUENCE_CANVAS.maxLaneHeightPx);
            setViewport((previous) => {
              const target = document.targets[state.laneIndex]?.target;
              if (target === undefined) return previous;
              const complete = completeRowHeights(previous.rowHeights, document, settings);
              const heights = complete[target.fixture];
              if (heights === undefined) throw new Error("Timeline row heights are missing.");
              const rowHeights = { ...complete, [target.fixture]: { ...heights, [state.rowKind]: nextHeight } };
              const maxScrollY = Math.max(0, expandedTimelineHeight(layoutRows(rowHeights, revealAutomation)) - Math.max(1, canvasSize.height - top));
              return { ...previous, rowHeights, scrollY: clamp(previous.scrollY, 0, maxScrollY) };
            });
            return;
          }
          case "playheadScrub":
            seekFromCanvas(event);
            return;
          case "slice":
            setSlice({ seconds: state.seconds, fromY: state.startY, toY: y });
            return;
          case "timeRange":
            setRangeDraft(rangeFromDrag(state.anchorSeconds, rangeTimeFromCanvasX(x, snaps(event.altKey))));
            return;
          case "marquee":
            applyMarquee({ ...state.state, x, y });
            return;
          case "mark": {
            const deltaSeconds = roundToNanosecond((x - state.startX) / viewport.pxPerSecond);
            const timeSeconds = clamp(state.originalTimeSeconds + deltaSeconds, 0, document.durationSeconds);
            const collection = document.markCollections.find((candidate) => candidate.id === state.collectionId);
            const committedIndex = collection === undefined ? state.index : markIndexAfterMove(collection, state.index, timeSeconds);
            const activeSelection = sequenceSelectionRef.current;
            if (activeSelection?.type === "marks" && activeSelection.marks.length > 1 && activeSelection.marks.some((mark) => mark.collectionId === state.collectionId && mark.index === state.index)) {
              setMarkDrafts(markMoveDrafts(document, activeSelection.marks, constrainMarkDelta(document, activeSelection.marks, deltaSeconds)));
            } else {
              const nextDrafts: MarkDraftLookup = new Map();
              setMarkDraft(nextDrafts, { collectionId: state.collectionId, index: state.index }, { collectionId: state.collectionId, index: state.index, timeSeconds, committedIndex });
              setMarkDrafts(nextDrafts);
            }
            setRevealAutomation(false);
            setGroupDraft([]);
            return;
          }
          case "automationErase": {
            const layout = visibleEnvelopes.find((candidate) => candidate.clip.id === state.clipId && candidate.laneIndex === state.laneIndex);
            if (layout === undefined) return;
            const before = envelopeOf(layout.clip)?.curve ?? [];
            const curve = eraseAutomationCurvePoints(before, layout, x, y);
            if (curve.length !== before.length) setAutomationCurveDraft({ id: layout.clip.id, curve, guide: { time: null, value: null } });
            return;
          }
          case "automationPoint": {
            const layout = visibleEnvelopes.find((candidate) => candidate.clip.id === state.clipId && candidate.laneIndex === state.laneIndex);
            if (layout !== undefined) dragAutomationPoint(layout, state.index, x, y, event);
            return;
          }
          case "sequence": {
            const selection = sequenceSelectionRef.current;
            const grabbed = document.clips.find((clip) => clip.id === state.id);
            if (started && state.resize === "none" && grabbed !== undefined && selection?.type === "clips" && document.clips.some((clip) => clip.content.type === "envelope" && selection.ids.includes(clip.id))) {
              const sourceKind = clipRowKind(grabbed);
              const before = rows.find((row) => row.laneIndex === state.laneIndex && row.kind === sourceKind);
              const expanded = layoutRows(viewport.rowHeights, true);
              const after = expanded.find((row) => row.laneIndex === state.laneIndex && row.kind === sourceKind);
              if (before === undefined || after === undefined) throw new Error("Dragged clip row is missing.");
              // Keep the grabbed row under the pointer while exposing empty drop rows.
              setViewport((previous) => ({ ...previous, scrollY: previous.scrollY + after.top - before.top }));
              setRevealAutomation(true);
              return;
            }
            if (clips !== undefined) setGroupDraft(clips);
          }
        }
      }}
      onPointerUp={(event) => {
        const current = drag.current;
        drag.current = null;
        setDragCursor(null);
        setMarquee(null);
        if (current === null) return;
        const x = event.nativeEvent.offsetX;
        const result = reduce(current, { type: "up", x, y: event.nativeEvent.offsetY }, gestureContext(event));
        switch (current.kind) {
          case "stripResize": setStripResizeHover(null); return;
          case "gutterResize": setGutterResizeHover(false); return;
          case "rowResize": setRowResizeHover(null); return;
          case "timeRange": {
            if (current.active) {
              const range = rangeFromDrag(current.anchorSeconds, rangeTimeFromCanvasX(x, snaps(event.altKey)));
              if (range.endSeconds <= range.startSeconds) {
                setRangeDraft(null);
                return;
              }
              void runSnapshotCommand(() => commands.audioSetRange(range)).finally(() => { setRangeDraft(null); });
              return;
            }
            if (current.fromEdge) return;
            // A click places the playhead and clears the range.
            const positionSeconds = seekTimeFromCanvasX(current.startX);
            const hadRange = (useAppStore.getState().snapshot?.audioTransport.range ?? null) !== null;
            void runSnapshotCommand(async () => {
              if (hadRange) await commands.audioSetRange(null);
              return commands.audioSeek(positionSeconds);
            });
            return;
          }
          case "slice":
            setSlice({ seconds: current.seconds, fromY: event.nativeEvent.offsetY, toY: event.nativeEvent.offsetY });
            if (result.edit !== undefined) void applySequenceEdit(result.edit);
            return;
          case "mark": {
            if (result.edit === undefined) {
              setMarkDrafts(new Map());
              setGroupDraft([]);
              return;
            }
            void applySequenceEdit(result.edit, gestureRequest.current).then((update) => {
              setMarkDrafts(new Map());
              if (update === null) return;
              updateSequenceSelection(update.selection);
            });
            return;
          }
          case "automationErase":
            suppressContextMenu.current = true;
            if (automationCurveDraft !== null) commitAutomationCurve(automationCurveDraft.id, automationCurveDraft.curve, gestureRequest.current);
            return;
          case "automationPoint":
            if (!(current.active || current.inserted) || automationCurveDraft === null) {
              setAutomationCurveDraft(null);
              return;
            }
            commitAutomationCurve(automationCurveDraft.id, automationCurveDraft.curve, gestureRequest.current);
            return;
          case "sequence": {
            const clearDrafts = () => { setGroupDraft([]); setRevealAutomation(false); };
            const { edit, clips } = result;
            if (edit === undefined || clips === undefined) { clearDrafts(); return; }
            void applySequenceEdit(edit, gestureRequest.current).then((update) => {
              if (update === null) return;
              updateSequenceSelection(update.selection);
              setSelectedLaneIndex(current.laneIndex + (edit.type === "moveClips" ? edit.laneDelta : 0));
              const startSeconds = earliestStartSeconds(clips);
              if (startSeconds !== null) setSelectedTimeSeconds(startSeconds);
            }).finally(clearDrafts);
            return;
          }
          case "marquee":
          case "playheadScrub":
        }
      }}
      onPointerCancel={() => {
        drag.current = null;
        gestureRequest.current = null;
        setGroupDraft([]);
        setAutomationCurveDraft(null);
        setMarkDrafts(new Map());
        setRevealAutomation(false);
        setDragCursor(null);
        setMarquee(null);
        setRangeDraft(null);
        setSlice(null);
      }}
      onPointerLeave={() => {
        if (drag.current === null) {
          setSeekHover(null);
          setHover(null);
          setRowResizeHover(null);
          setStripResizeHover(null);
          setGutterResizeHover(false);
          setBadgeHover(null);
          setSlice(null);
        }
      }}
          />
        </ContextMenu.Trigger>
        {sequenceContextMenu !== null && (
          <ContextMenu.Portal container={overlayContainer}>
            <ContextMenu.Content className="menu-content">
              <ContextMenuEntries entries={[
                ...(laneContextMenu === null ? [] : [{
                  type: "submenu",
                  label: "Add Effect",
                  entries: definitionEntries(effectTree, (definition) => definition.name, (definition) => void addClipFromContextMenu(definition, laneContextMenu), "No effects")
                }] satisfies MenuEntry[]),
                ...placeMenuEntries(sequenceContextMenu)
              ]} />
              {sequenceContextMenu.kind === "clip" && clipMatch !== null && (
                <>
                  <ContextMenu.Separator className="menu-separator" />
                  <ContextMenu.Sub>
                    <ContextMenu.SubTrigger className="menu-item">
                      Select Matching <ChevronRight size={THEME_METRICS.iconSizeSmall} aria-hidden />
                    </ContextMenu.SubTrigger>
                    <ContextMenu.Portal container={overlayContainer}>
                      <ContextMenu.SubContent className="menu-content">
                        <ClipMatchMenuItems
                          criteria={clipMatch}
                          onCriteria={saveClipMatch}
                          onSelectMatching={() => {
                            const seed = selectedClipSeed();
                            if (seed !== null) selectMatching(seed, clipMatch, []);
                          }}
                          onOptions={() => { setClipMatchSeed(selectedClipSeed()); }}
                        />
                      </ContextMenu.SubContent>
                    </ContextMenu.Portal>
                  </ContextMenu.Sub>
                  <ClipArrangeMenus
                    reference={sequenceContextMenu.clipId}
                    canAlign={activeSelection !== null && selectionCount(activeSelection) > 1}
                    markCollections={markTimes.length === 0 ? [] : visibleMarkCollections.map((collection) => collection.id)}
                    onArrange={arrangeClips}
                    onCascade={() => { setCascadeOpen(true); }}
                  />
                </>
              )}
              <ContextMenuEntries entries={objectMenuEntries(sequenceContextMenu)} />
            </ContextMenu.Content>
          </ContextMenu.Portal>
        )}
      </ContextMenu.Root>
      {cascadeOpen && (
        <CascadeDialog
          onCascade={(stepSeconds, reverse) => {
            arrangeClips({ type: "cascade", stepSeconds, reverse });
            setCascadeOpen(false);
          }}
          onClose={() => { setCascadeOpen(false); }}
        />
      )}
      {clipMatchSeed !== null && clipMatch !== null && (
        <ClipMatchDialog
          document={document}
          seed={clipMatchSeed}
          criteria={clipMatch}
          match={matchClips}
          onSelect={(criteria, params) => {
            saveClipMatch(criteria);
            selectMatching(clipMatchSeed, criteria, params);
            setClipMatchSeed(null);
          }}
          onClose={() => { setClipMatchSeed(null); }}
        />
      )}
      <div className="sequence-gutter-scrollbar-divider" style={{ left }} aria-hidden="true" />
      <Tip
        side="right"
        open={badgeTooltip !== null && badgeLane !== undefined}
        label={badgeLane === undefined ? null : `${badgeLane.label} is in ${badgeLane.occurrences} places in this list because it belongs to more than one group. Every copy shows the same clips. Click to jump to the next one.`}
      >
        <span
          className="sequence-shared-badge-anchor"
          style={badgeTooltip === null ? undefined : { left: sharedBadgeX(left), top: badgeTooltip.y, width: left - sharedBadgeX(left), height: badgeTooltip.height }}
          aria-hidden="true"
        />
      </Tip>
      <div className="sequence-horizontal-scrollbar" style={{ left, width: timelineWidth }} onPointerDown={handleScrollbarPointerDown} onPointerMove={handleScrollbarPointerMove} onPointerUp={() => { sequenceScrollbar.current = null; }} onPointerCancel={() => { sequenceScrollbar.current = null; }} aria-label="Sequence horizontal scrollbar" role="scrollbar" aria-orientation="horizontal" aria-valuemin={0} aria-valuemax={Math.round(maxScrollXSeconds * 1000)} aria-valuenow={Math.round(viewport.scrollXSeconds * 1000)}>
        <div className={`sequence-horizontal-scrollbar-thumb ${maxScrollXSeconds === 0 ? "disabled" : ""}`} style={{ left: scrollbarThumbLeft, width: scrollbarThumbWidth }} />
      </div>
      <SequenceWaveform
        audio={document.audio}
        settings={settings}
        left={left}
        top={audioStripTop}
        width={Math.max(0, canvasSize.width - left)}
        height={audioStripHeight}
        pxPerSecond={viewport.pxPerSecond}
        scrollXSeconds={viewport.scrollXSeconds}
      />
      <SequenceTransportOverlay
        document={document}
        viewport={viewport}
        setViewport={setViewport}
        left={left}
        audioStripTop={audioStripTop}
        seekZoneHeight={audioStripBottom}
        followMode={settings?.sequenceFollowMode ?? null}
        overlay={overlay}
        canvasSize={canvasSize}
        playheadClockRef={playheadClockRef}
      />
    </div>
  );
}

function SequenceTransportOverlay({
  document,
  viewport,
  setViewport,
  left,
  audioStripTop,
  seekZoneHeight,
  followMode,
  overlay,
  canvasSize,
  playheadClockRef
}: {
  document: SequenceGuiDocument;
  viewport: SequenceViewport;
  setViewport: Dispatch<SetStateAction<SequenceViewport>>;
  left: number;
  audioStripTop: number;
  seekZoneHeight: number;
  followMode: SequenceFollowMode | null;
  overlay: StoreApi<SequenceOverlayState>;
  canvasSize: { width: number; height: number };
  playheadClockRef: RefObject<(() => number) | null>;
}) {

  const transport = useAppStore((store) => store.snapshot?.audioTransport ?? null);
  if (transport === null) return null;
  return (
    <SequenceTransportMarkers
      document={document}
      transport={transport}
      viewport={viewport}
      setViewport={setViewport}
      left={left}
      audioStripTop={audioStripTop}
      seekZoneHeight={seekZoneHeight}
      followMode={followMode}
      overlay={overlay}
      canvasSize={canvasSize}
      playheadClockRef={playheadClockRef}
    />
  );
}

function SequenceTransportMarkers({
  document,
  transport,
  viewport,
  setViewport,
  left,
  audioStripTop,
  seekZoneHeight,
  followMode,
  overlay,
  canvasSize,
  playheadClockRef
}: {
  document: SequenceGuiDocument;
  transport: AudioTransportViewSnapshot;
  viewport: SequenceViewport;
  setViewport: Dispatch<SetStateAction<SequenceViewport>>;
  left: number;
  audioStripTop: number;
  seekZoneHeight: number;
  followMode: SequenceFollowMode | null;
  overlay: StoreApi<SequenceOverlayState>;
  canvasSize: { width: number; height: number };
  playheadClockRef: RefObject<(() => number) | null>;
}) {
  const seekHover = useStore(overlay, (state) => state.seekHover);
  const rangeDraft = useStore(overlay, (state) => state.rangeDraft);
  const liveTransport = useSequenceTransport(transport, playheadClockRef);
  const markerHeight = Math.max(0, canvasSize.height - audioStripTop - THEME_METRICS.scrollbarWidth);
  const markerLeft = (seconds: number) =>
    left + (clamp(seconds, 0, document.durationSeconds) - viewport.scrollXSeconds) * viewport.pxPerSecond;
  const playheadLeft = markerLeft(liveTransport.positionSeconds);
  const homeLeft = markerLeft(liveTransport.homeSeconds);
  const visible = (x: number) => x >= left && x <= canvasSize.width;

  // Page mode pages the view forward when the playhead passes the edge, and jumps to the playhead
  // when playback starts off screen or the playhead leaves the view, including a loop wrapping back.
  // If the user scrolls away during playback, the view stays put until the playhead is visible
  // again or playback restarts. Continuous mode holds the playhead at the anchor, moving freely
  // only where the view meets either end of the sequence.
  const following = liveTransport.state === "playing" && followMode !== null && followMode !== "off";
  const positionSeconds = liveTransport.positionSeconds;
  const followFrame = useRef<{ onScreen: boolean; scrollXSeconds: number } | null>(null);
  useEffect(() => {
    if (!following) {
      followFrame.current = null;
      return;
    }
    const visibleSeconds = Math.max(0, canvasSize.width - left) / viewport.pxPerSecond;
    const maxScrollXSeconds = Math.max(0, document.durationSeconds - visibleSeconds);
    if (followMode === "continuous") {
      const scrollXSeconds = clamp(positionSeconds - visibleSeconds * SEQUENCE_FOLLOW.anchor, 0, maxScrollXSeconds);
      setViewport((current) => current.scrollXSeconds === scrollXSeconds ? current : { ...current, scrollXSeconds });
      return;
    }
    const pageEnd = viewport.scrollXSeconds + visibleSeconds * SEQUENCE_FOLLOW.edge;
    const onScreen = positionSeconds >= viewport.scrollXSeconds && positionSeconds <= viewport.scrollXSeconds + visibleSeconds;
    const previous = followFrame.current;
    followFrame.current = { onScreen, scrollXSeconds: viewport.scrollXSeconds };
    // The playhead, not the view, moved off screen since the last frame.
    const playheadLeftView = previous !== null && previous.onScreen && previous.scrollXSeconds === viewport.scrollXSeconds;
    const page = previous === null
      ? !onScreen || positionSeconds > pageEnd
      : onScreen ? positionSeconds > pageEnd : playheadLeftView;
    if (!page) return;
    const scrollXSeconds = clamp(positionSeconds - visibleSeconds * SEQUENCE_FOLLOW.lead, 0, maxScrollXSeconds);
    setViewport((current) => current.scrollXSeconds === scrollXSeconds ? current : { ...current, scrollXSeconds });
  }, [following, followMode, positionSeconds, viewport.scrollXSeconds, viewport.pxPerSecond, canvasSize.width, left, document.durationSeconds, setViewport]);

  const range = rangeDraft ?? liveTransport.range;
  const rangeLeft = range === null ? 0 : Math.max(left, markerLeft(range.startSeconds));
  const rangeRight = range === null ? 0 : Math.min(canvasSize.width, markerLeft(range.endSeconds));
  const hoverLeft = seekHover === null ? 0 : markerLeft(seekHover.seconds);
  const showSeekHover = seekHover !== null && (seekHover.target === "time" || seekHover.target === "home") && visible(hoverLeft);

  return (
    <>
      {rangeRight > rangeLeft && (
        <div
          className="sequence-transport-range"
          style={{ left: rangeLeft, width: rangeRight - rangeLeft, top: 0, height: audioStripTop + markerHeight }}
        />
      )}
      {showSeekHover && (
        <div className="sequence-seek-hover" style={{ left: hoverLeft, top: 0, height: seekZoneHeight }}>
          <span className="sequence-seek-hover-label">
            {seekHover.target === "home" ? "Home" : formatSeconds(seekHover.seconds)}
          </span>
        </div>
      )}
      {visible(homeLeft) && (
        <div
          className="sequence-transport-marker home"
          style={{ left: homeLeft, top: audioStripTop, height: markerHeight }}
        />
      )}
      {visible(playheadLeft) && (
        <>
          <div className="sequence-playhead-head" style={{ left: playheadLeft, top: audioStripTop }} />
          <div
            className="sequence-transport-marker playhead"
            style={{ left: playheadLeft, top: audioStripTop, height: markerHeight }}
          />
        </>
      )}
    </>
  );
}

function envelopeLabel(document: SequenceGuiDocument, bindings: NonNullable<ReturnType<typeof envelopeOf>>["bindings"]) {
  const primary = bindings[0];
  if (primary === undefined) return "Unassigned envelope";
  const label = automationTargetLabel(document, primary.target);
  return bindings.length > 1 ? `${label} +${bindings.length - 1}` : label;
}

function automationTargetLabel(document: SequenceGuiDocument, target: SequenceAutomationTarget) {
  if (target.type === "clipParam") {
    const clip = document.clips.find((candidate) => candidate.id === target.clipId);
    if (clip?.content.type !== "effect") throw new Error(`Automation target clip ${target.clipId} is missing`);
    return `${clip.content.effectName}: ${target.param}`;
  }
  const node = document.compositionGraph.nodes.find((candidate) => candidate.id === target.nodeId);
  if (node === undefined || node.kind.type !== "operator") {
    throw new Error(`Automation target operator ${target.nodeId} is missing`);
  }
  return `${graphOperatorDefinition(document.compositionGraph.operatorCatalog, node.kind.operator).displayName}: ${target.param}`;
}

/** A filled triangle: pointing right when collapsed, down when expanded. */
function drawDisclosure(ctx: CanvasRenderingContext2D, x: number, centerY: number, collapsed: boolean) {
  const size = THEME_METRICS.sequenceDisclosureSize;
  const half = size / 2;
  ctx.beginPath();
  if (collapsed) {
    ctx.moveTo(x, centerY - half);
    ctx.lineTo(x + size, centerY);
    ctx.lineTo(x, centerY + half);
  } else {
    ctx.moveTo(x, centerY - half);
    ctx.lineTo(x + size, centerY - half);
    ctx.lineTo(x + half, centerY + half);
  }
  ctx.closePath();
  ctx.fill();
}

function completeRowHeights(heights: SequenceRowHeightMap, document: SequenceGuiDocument, settings: AppSettings | null): SequenceRowHeightMap {
  const missing = document.targets.filter((lane) => heights[lane.target.fixture] === undefined);
  if (missing.length === 0) return heights;
  const defaultHeight = initialSequenceLaneHeight(settings);
  return { ...heights, ...Object.fromEntries(missing.map((lane) => [lane.target.fixture, { clips: defaultHeight, automation: automationLaneRowHeight(defaultHeight) }])) };
}

function sequenceViewportFromPersisted(state: PersistedSequenceViewportState | undefined, document: SequenceGuiDocument, settings: AppSettings | null): SequenceViewport {
  return {
    pxPerSecond: state === undefined ? settings?.sequenceInitialPxPerSecond ?? SEQUENCE_CANVAS.initialPxPerSecond : clamp(state.pxPerSecond, SEQUENCE_CANVAS.minPxPerSecond, SEQUENCE_CANVAS.maxZoomPxPerSecond),
    leftGutterWidth: clamp(state?.leftGutterWidthPx ?? SEQUENCE_CANVAS.initialLeftGutterPx, SEQUENCE_CANVAS.minLeftGutterPx, SEQUENCE_CANVAS.maxLeftGutterPx),
    audioStripHeight: clamp(state?.audioStripHeightPx ?? SEQUENCE_CANVAS.initialAudioStripHeightPx, TIMELINE_STRIPS.audio.minPx, TIMELINE_STRIPS.audio.maxPx),
    markRulerHeight: clamp(state?.markRulerHeightPx ?? SEQUENCE_CANVAS.initialMarkRulerHeightPx, TIMELINE_STRIPS.marks.minPx, TIMELINE_STRIPS.marks.maxPx),
    rowHeights: restoreRowHeights(document.targets, state?.rowHeights, initialSequenceLaneHeight(settings)),
    scrollXSeconds: Math.max(0, state?.scrollXSeconds ?? 0),
    scrollY: Math.max(0, state?.scrollY ?? 0)
  };
}

function initialSequencePxPerSecond(settings: AppSettings | null, timelineWidth: number, durationSeconds: number): number {
  const minPxPerSecond = minSequencePxPerSecond(timelineWidth, durationSeconds);
  if (settings?.sequenceInitialZoomMode === "fixedPxPerSecond") {
    return clamp(settings.sequenceInitialPxPerSecond, minPxPerSecond, SEQUENCE_CANVAS.maxPxPerSecond);
  }
  return clamp(minPxPerSecond, minPxPerSecond, SEQUENCE_CANVAS.maxPxPerSecond);
}

function minSequencePxPerSecond(timelineWidth: number, durationSeconds: number): number {
  if (!Number.isFinite(durationSeconds) || durationSeconds <= 0) {
    return SEQUENCE_CANVAS.minPxPerSecond;
  }
  return Math.max(SEQUENCE_CANVAS.minPxPerSecond, timelineWidth / durationSeconds);
}

function initialSequenceLaneHeight(settings: AppSettings | null): number {
  return clamp(settings?.sequenceInitialLaneHeightPx ?? SEQUENCE_CANVAS.initialLaneHeightPx, SEQUENCE_CANVAS.minLaneHeightPx, SEQUENCE_CANVAS.maxLaneHeightPx);
}

function drawTimelineGrid(
  ctx: CanvasRenderingContext2D,
  left: number,
  top: number,
  width: number,
  height: number,
  pxPerSecond: number,
  scrollXSeconds: number,
  frameRate: number
) {
  const tick = chooseTimelineTick(pxPerSecond, frameRate);
  const firstMinor = Math.floor(scrollXSeconds / tick.minorSeconds) * tick.minorSeconds;
  ctx.lineWidth = THEME_METRICS.visualLineWidth;
  for (let time = firstMinor; ; time += tick.minorSeconds) {
    const x = left + (time - scrollXSeconds) * pxPerSecond;
    if (x > width) break;
    if (x < left) continue;
    const labeled = isMultipleOf(time, tick.labelSeconds);
    ctx.strokeStyle = labeled ? SEQUENCE_COLORS.timelineMajor : SEQUENCE_COLORS.timelineMinor;
    ctx.beginPath();
    ctx.moveTo(x + 0.5, labeled ? 0 : top);
    ctx.lineTo(x + 0.5, height);
    ctx.stroke();
    if (labeled) {
      ctx.fillStyle = SEQUENCE_COLORS.timelineLabel;
      ctx.fillText(formatTimelineSeconds(time, tick.labelSeconds), x + THEME_METRICS.timelineLabelX, THEME_METRICS.timelineLabelY);
    }
  }
}

function chooseTimelineTick(pxPerSecond: number, frameRate: number) {
  const frameSeconds = 1 / Math.max(1, frameRate);
  const minorCandidates = Array.from(new Set([
    frameSeconds,
    frameSeconds * 2,
    frameSeconds * 5,
    frameSeconds * 10,
    0.05,
    0.1,
    0.25,
    0.5,
    1,
    2.5,
    5,
    10,
    30,
    60
  ])).sort((left, right) => left - right);
    const minorSeconds = minorCandidates.find((candidate) => candidate * pxPerSecond >= THEME_METRICS.timelineMinGridWidth) ?? 60;
    const labelSeconds = minorCandidates.find((candidate) => candidate >= minorSeconds && candidate * pxPerSecond >= THEME_METRICS.timelineMinLabelWidth) ?? minorSeconds * 5;
  return { minorSeconds, labelSeconds };
}

function isMultipleOf(value: number, interval: number) {
  return Math.abs(value / interval - Math.round(value / interval)) < 0.0001;
}

function formatTimelineSeconds(value: number, intervalSeconds: number) {
  if (intervalSeconds < 1) {
    const totalMilliseconds = Math.max(0, Math.round(value * 1000));
    const minutes = Math.floor(totalMilliseconds / 60000);
    const seconds = Math.floor((totalMilliseconds % 60000) / 1000);
    const milliseconds = totalMilliseconds % 1000;
    return `${minutes}:${String(seconds).padStart(2, "0")}.${String(milliseconds).padStart(3, "0")}`;
  }
  return formatSeconds(value);
}

function drawStripLabel(ctx: CanvasRenderingContext2D, label: string, stripTop: number, stripHeight: number, left: number) {
  const x = THEME_METRICS.sequenceLabelX;
  ctx.save();
  ctx.font = THEME_TYPOGRAPHY.sequenceHeading;
  ctx.fillStyle = THEME_COLORS.textSoft;
  ctx.fillText(fitCanvasLabel(ctx, label, left - x * 2), x, stripTop + stripHeight / 2 + THEME_METRICS.sequenceLabelYOffset);
  ctx.restore();
}

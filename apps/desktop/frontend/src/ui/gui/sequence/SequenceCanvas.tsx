import { markIndexAfterMove } from "./sequenceSelection";
import { editShortcutTarget, hasPrimaryModifier, isMac, isSecondaryClick } from "../../../platform";
import { OverlayPortal } from "../../OverlayPortal";
import { useSequenceEditorHost, type SequenceEditorHost } from "../../../editor/host";
import { objectViewKey } from "../../../workspace/guiIdentity";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState, type Dispatch, type MouseEvent, type PointerEvent, type RefObject, type SetStateAction, useContext } from "react";

import { ArrowRight, ChevronRight, Scissors, Trash2 } from "lucide-react";

import { GUI_HISTORY_CHANGED_EVENT } from "../../../editor/host";

import { scheduleViewStateSave } from "../../../viewStatePersistence";

import type { AppSettings, GuiDocumentRequest, GuiObjectRef, FixtureTarget, PersistedSequenceViewportState, PlaybackRange, SequenceAutomationClip, SequenceFollowMode, SequenceAutomationTarget, SequenceEditorDocument, SequenceEffectScope, SequenceEffectDefinition, SequenceLane } from "../../../editor/types";


import { clamp, formatSeconds, roundToNanosecond, type AudioTransportViewSnapshot, type AutomationClipChooser, type GuiFocus, type SequenceSelection } from "../shared";

import { activeMarkCollection, defaultMarkColor, drawMarkRulerLabel, drawSequenceMarks, committedMarkDrafts, nextCollectionKey } from "./marks";
import { TAP_MARK_EVENT } from "../../uiEvents";

import { graphOperatorDefinition } from "./graphOperator";
import { targetAtLane, targetsEqual } from "./sequenceTargets";
import { SequenceWaveform } from "./sequenceWaveform";
import { drawClipRaster, useSequenceClipRasters } from "./sequenceClipRasters";
import { useSequenceTransport } from "./SequenceTransportControls";
import {
  automationClipsWithDrafts,
  automationCurvePointFromCanvas,
  automationHoverEqual,
  automationLaneRowHeight,
  rowHeightAt,
  persistRowHeights,
  restoreRowHeights,
  buildAutomationClipLayout,
  drawAutomationClip,
  expandedTimelineHeight,
  fitCanvasLabel,
  hitTimelineClip,
  hitAutomationCurvePoint,
  laneIndexFromCanvasY,
  rowFromCanvasY,
  sequenceRowLayout,
  collapseRows,
  collapsedLaneTargets,
  type SequenceRowLayout,
  type SequenceRowKind,
  removeAutomationCurvePoint,
  insertAutomationCurvePoint,
  moveAutomationCurvePoint,
  alignAutomationCurvePoint,
  hitAutomationCurveLine,
  type AutomationClipLayout,
  type AutomationGuide,
  type AutomationCurvePoint,
  type SequenceRowHeightMap,
  type AutomationCurveDraft,
  type AutomationDraft,
  type AutomationHover
} from "./sequenceAutomationLayout";
import { THEME_COLORS, THEME_METRICS, THEME_TYPOGRAPHY } from "../../../theme";
import { DefinitionMenuItems, definitionTree } from "./definitionMenu";
import { markSnapTimes, snapDeltaToMarks, snapToMark } from "./sequenceSnap";

import { buildSequenceClipLayout, clipSelectionGesture, constrainMarkDelta,  hitSequence, hitSequenceMark, markMoveDrafts, markRefLookup, mergeSequenceSelection, nextEffectSelection, nextAutomationSelection, nextMarkSelection, normalizedRect, selectedEffectId, selectionCount, selectionFromMarqueeEffects, selectionFromMarqueeMarks, sequenceHoverEqual, setMarkDraft, singleEffectSelectionFocus, singleSelectionFocus, selectionFromSingle, type MarkDraftLookup, type SequenceContextMenu, type SequenceHover, type SequenceMarquee, type SequenceDraft, type SequenceViewport } from "./sequenceSelection";

const SEQUENCE_FOLLOW = {
  edge: THEME_METRICS.sequenceFollowEdge,
  lead: THEME_METRICS.sequenceFollowLead,
  anchor: THEME_METRICS.sequenceFollowAnchor
};

const SEQUENCE_CANVAS = {
  leftGutterPx: THEME_METRICS.sequenceLeftGutter,
  audioStripTopPx: THEME_METRICS.sequenceAudioStripTop,
  initialAudioStripHeightPx: THEME_METRICS.sequenceInitialAudioStripHeight,
  initialMarkRulerHeightPx: THEME_METRICS.sequenceInitialMarkRulerHeight,
  initialPxPerSecond: THEME_METRICS.sequenceInitialPixelsPerSecond,
  initialLaneHeightPx: THEME_METRICS.sequenceInitialLaneHeight,
  minPxPerSecond: THEME_METRICS.sequenceMinPixelsPerSecond,
  maxPxPerSecond: THEME_METRICS.sequenceMaxPixelsPerSecond,
  maxZoomPxPerSecond: THEME_METRICS.sequenceMaxZoomPixelsPerSecond,
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
  gridFaint: THEME_COLORS.sequenceGridFaint,
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
  effectMarqueeFill: THEME_COLORS.effectMarqueeFill
} as const;

const SEQUENCE_DRAG_THRESHOLD_PX = THEME_METRICS.sequenceDragThreshold;

type SequenceDragState =
  | null
  | { kind: "stripResize"; strip: TimelineStrip; startY: number; initialHeight: number; active: boolean }
  | { kind: "rowResize"; laneIndex: number; rowKind: SequenceRowKind; startY: number; initialHeight: number; active: boolean }
  | { kind: "sequence"; id: number; startX: number; startY: number; active: boolean; originalStartSeconds: number; laneIndex: number; resize: "none" | "left" | "right" }
  | { kind: "automation"; id: number; startX: number; startY: number; active: boolean; originalStartSeconds: number; rowTarget: FixtureTarget; resize: "none" | "left" | "right" }
  | { kind: "automationPoint"; clipId: number; index: number; active: boolean; inserted: boolean }
  | { kind: "mark"; collectionKey: string; index: number; startX: number; startY: number; active: boolean; originalTimeSeconds: number }
  | { kind: "marquee"; state: SequenceMarquee }
  | { kind: "playheadScrub" }
  // Dragging a range edge anchors the new range at the opposite edge.
  | { kind: "timeRange"; startX: number; anchorSeconds: number; fromEdge: boolean; active: boolean };

type LaneContextMenu = Extract<SequenceContextMenu, { laneIndex: number }>;

/** What a pointer in the ruler and waveform strip would grab. */
type SeekHover = { target: "playhead" | "rangeEdge" | "home" | "time"; seconds: number };
const SEEK_HOVER_CURSORS = { playhead: "grab", rangeEdge: "ew-resize", home: "text", time: "text" } as const;

function rowResizeHit(
  y: number,
  top: number,
  scrollY: number,
  rows: SequenceRowLayout[]
): { laneIndex: number; rowKind: SequenceRowKind; edgeY: number } | null {
  const contentY = y - top + scrollY;
  for (const row of rows) {
    if (row.height === 0) continue;
    if (Math.abs(contentY - row.bottom) <= THEME_METRICS.sequenceLaneResizeHitHeight) {
      return { laneIndex: row.laneIndex, rowKind: row.kind, edgeY: row.bottom };
    }
  }
  return null;
}

/** The resizable strips above the lanes, each with its viewport height and limits. */
type TimelineStrip = "audio" | "marks";
const TIMELINE_STRIPS = {
  audio: { height: "audioStripHeight", minPx: THEME_METRICS.sequenceMinAudioStripHeight, maxPx: THEME_METRICS.sequenceMaxAudioStripHeight },
  marks: { height: "markRulerHeight", minPx: THEME_METRICS.sequenceMinMarkRulerHeight, maxPx: THEME_METRICS.sequenceMaxMarkRulerHeight }
} as const satisfies Record<TimelineStrip, { height: keyof SequenceViewport; minPx: number; maxPx: number }>;

/**
 * Each strip resizes from its bottom edge. The audio strip's grip lies inside the strip, so it
 * never covers the Marks lane below; the Marks lane's grip straddles its border with the lanes.
 */
function stripResizeHit(y: number, audioStripBottom: number, markRulerBottom: number): TimelineStrip | null {
  if (Math.abs(y - markRulerBottom) <= SEQUENCE_CANVAS.audioResizeHitHeightPx / 2) return "marks";
  if (y <= audioStripBottom && audioStripBottom - y <= SEQUENCE_CANVAS.audioResizeHitHeightPx) return "audio";
  return null;
}


export function SequenceCanvas({
  document,
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
  document: SequenceEditorDocument;
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
  const host = useSequenceEditorHost();
  const overlayContainer = useContext(OverlayPortal);
  const { commands, store: useAppStore, runGuiEditCommand, runSnapshotCommand } = host;

  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<SequenceDragState>(null);
  const playheadClockRef = useRef<(() => number) | null>(null);
  const sequenceSelectionRef = useRef<SequenceSelection>(sequenceSelection);
  const [revealAutomation, setRevealAutomation] = useState(false);
  const [automationDrafts, setAutomationDrafts] = useState<AutomationDraft[]>([]);
  const [automationCurveDraft, setAutomationCurveDraft] = useState<AutomationCurveDraft | null>(null);
  const [groupDraft, setGroupDraft] = useState<SequenceDraft[]>([]);
  const [markDrafts, setMarkDrafts] = useState<MarkDraftLookup>(() => new Map());
  const [sequenceContextMenu, setSequenceContextMenu] = useState<SequenceContextMenu | null>(null);
  const [hover, setHover] = useState<SequenceHover>(null);
  const [rowResizeHover, setRowResizeHover] = useState<{ laneIndex: number; rowKind: SequenceRowKind } | null>(null);
  const [stripResizeHover, setStripResizeHover] = useState<TimelineStrip | null>(null);
  const [dragCursor, setDragCursor] = useState<"grabbing" | null>(null);
  const [seekHover, setSeekHover] = useState<SeekHover | null>(null);
  const [rangeDraft, setRangeDraft] = useState<PlaybackRange | null>(null);
  const [selectedTarget, setSelectedTarget] = useState<FixtureTarget | null>(null);
  const selectedLaneIndex = selectedTarget === null ? null : document.lanes.findIndex((lane) => targetsEqual(lane.target, selectedTarget));
  const setSelectedLaneIndex = (index: number) => { setSelectedTarget(targetAtLane(document, index)); };
  const [selectedTimeSeconds, setSelectedTimeSeconds] = useState<number | null>(null);
  const [marquee, setMarquee] = useState<SequenceMarquee | null>(null);
  const [canvasSize, setCanvasSize] = useState({ width: 0, height: 0 });
  const sequenceScrollbar = useRef<{ pointerId: number; startX: number; startScroll: number } | null>(null);
  const restoreState = useAppStore((store) => store.restoreState);
  const gestureRequest = useRef<GuiDocumentRequest | null>(null);
  const settings = useAppStore((store) => store.snapshot?.settings ?? null);
  const playing = useAppStore((store) => store.snapshot?.audioTransport.state === "playing");
  // Continuous following owns horizontal scrolling while playing; zoom stays anchored on the playhead.
  const followingContinuously = playing && settings?.sequenceFollowMode === "continuous";
  const restoreKey = objectViewKey(document.sourceRef);
  const restoredViewport = restoreState?.sequenceViewports[restoreKey];
  const [viewport, setViewport] = useState<SequenceViewport>(() => sequenceViewportFromPersisted(restoredViewport, document, settings));
  const viewportInitialized = useRef(false);
  const restoredViewportKey = useRef<string | null>(restoredViewport === undefined ? null : restoreKey);
  const left = SEQUENCE_CANVAS.leftGutterPx;
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
  const top = audioStripBottom + markRulerHeight;
  const inMarkRuler = (y: number) => y >= audioStripBottom && y < top;
  const targetMarkCollectionKey = activeMarkCollection(document.markCollections, activeMarkCollectionKey)?.key ?? null;
  const automationRowHeight = automationLaneRowHeight(initialSequenceLaneHeight(settings));
  const automationClipsForLayout = useMemo(
    () => automationClipsWithDrafts(document.automationClips, automationDrafts, automationCurveDraft),
    [automationCurveDraft, automationDrafts, document.automationClips]
  );
  // Group lanes collapse like folders in a file tree; their members take no rows.
  const [collapsedGroups, setCollapsedGroups] = useState<ReadonlySet<number>>(() => new Set());
  const hiddenLanes = useMemo(() => collapsedLaneTargets(document.lanes, collapsedGroups), [document.lanes, collapsedGroups]);
  const layoutRows = useCallback(
    (rowHeights: SequenceRowHeightMap, reveal: boolean) =>
      collapseRows(sequenceRowLayout(document.lanes, document.automationClips, rowHeights, initialSequenceLaneHeight(settings), automationRowHeight, reveal), hiddenLanes),
    [document.lanes, document.automationClips, settings, automationRowHeight, hiddenLanes]
  );
  const rows = useMemo(() => layoutRows(viewport.rowHeights, revealAutomation), [layoutRows, viewport.rowHeights, revealAutomation]);
  const visibleMarkCollections = useMemo(
    () => document.markCollections.filter((collection) => visibleMarkCollectionKeys.has(collection.key)),
    [document.markCollections, visibleMarkCollectionKeys]
  );
  const markTimes = useMemo(() => markSnapTimes(visibleMarkCollections), [visibleMarkCollections]);
  const [automationHover, setAutomationHover] = useState<AutomationHover | null>(null);
  const canvasCursor =
    dragCursor ??
    (seekHover !== null ? SEEK_HOVER_CURSORS[seekHover.target] :
    stripResizeHover !== null || rowResizeHover !== null ? "ns-resize" :
    (automationClipChooser !== null && automationHover !== null
      ? "pointer"
      : automationHover !== null
      ? automationHover.resize === "none" ? "grab" : "ew-resize"
      : hover === null ? undefined : hover.kind === "mark" ? "pointer" : hover.resize === "none" ? "grab" : "ew-resize"));

  const updateSequenceSelection = useCallback((selection: SequenceSelection) => {
    sequenceSelectionRef.current = selection;
    setSequenceSelection(selection);
  }, [setSequenceSelection]);

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
        const rowHeights = Object.fromEntries(Object.entries(completeRowHeights(current.rowHeights, document, settings)).map(([id, heights]) => [id, { effects: clamp(heights.effects * scale, SEQUENCE_CANVAS.minLaneHeightPx, SEQUENCE_CANVAS.maxLaneHeightPx), automation: clamp(heights.automation * scale, SEQUENCE_CANVAS.minLaneHeightPx, SEQUENCE_CANVAS.maxLaneHeightPx) }]));
        return {
          ...current,
          rowHeights,
          scrollY: clamp(current.scrollY, 0, Math.max(0, expandedTimelineHeight(layoutRows(rowHeights, revealAutomation)) - visibleHeight))
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
  }, [document, followingContinuously, layoutRows, left, revealAutomation, scrollbarHeight, settings, setViewport, top, useAppStore]);

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

  useEffect(() => {
    const clearHistorySelection = () => {
      drag.current = null;
      gestureRequest.current = null;
      updateSequenceSelection(null);
      setSelected(null);
      setGroupDraft([]);
      setAutomationDrafts([]);
      setAutomationCurveDraft(null);
      setMarkDrafts(new Map());
      setRevealAutomation(false);
      setDragCursor(null);
      setMarquee(null);
      setSequenceContextMenu(null);
    };
    window.addEventListener(GUI_HISTORY_CHANGED_EVENT, clearHistorySelection);
    return () => { window.removeEventListener(GUI_HISTORY_CHANGED_EVENT, clearHistorySelection); };
  }, [setSelected, updateSequenceSelection]);


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
      audioStripHeightPx: viewport.audioStripHeight,
      markRulerHeightPx: viewport.markRulerHeight,
      rowHeights: persistRowHeights(viewport.rowHeights),
      scrollXSeconds: viewport.scrollXSeconds,
      scrollY: viewport.scrollY,
      activeMarkCollectionKey,
      visibleMarkCollectionKeys: [...visibleMarkCollectionKeys]
    };
    scheduleSequenceViewportStateSave(host, document.sourceRef, state);
  }, [host, followingContinuously, activeMarkCollectionKey, automationRowHeight, revealAutomation, document, settings, viewport, visibleMarkCollectionKeys]);

  const visibleClips = useMemo(
    () => buildSequenceClipLayout(
      document,
      groupDraft,
      viewport,
      left,
      top,
      canvasSize,
      rows
    ),
    [canvasSize, document, groupDraft, left, top, viewport, rows]
  );
  const visibleRasterClips = useMemo(() => {
    return visibleClips
      .filter((clip) => clip.rect.x + clip.rect.width >= left && clip.rect.x <= canvasSize.width && clip.rect.y + clip.rect.height >= top && clip.rect.y <= canvasSize.height);
  }, [canvasSize.height, canvasSize.width, left, top, visibleClips]);
  const visibleAutomationClips = useMemo(
    () => buildAutomationClipLayout(automationClipsForLayout, rows, viewport, left, top, canvasSize),
    [automationClipsForLayout, rows, canvasSize, left, top, viewport]
  );
  const clipRasters = useSequenceClipRasters(document, visibleRasterClips, Math.max(...Object.values(viewport.rowHeights).map((heights) => heights.effects), SEQUENCE_CANVAS.minLaneHeightPx), settings);
  const selectedEffectIds = useMemo(() => new Set<number>(sequenceSelection?.type === "clips" ? sequenceSelection.effectIds : []), [sequenceSelection]);
  const activeAutomationTargetEffectIds = useMemo(() => {
    const clipIds = new Set<number>();
    if (selected?.type === "automationClip") clipIds.add(selected.id);
    if (automationHover !== null) clipIds.add(automationHover.clipId);
    const effectIds = new Set<number>();
    if (clipIds.size === 0) return effectIds;
    for (const clip of document.automationClips) {
      if (!clipIds.has(clip.id)) continue;
      for (const binding of clip.bindings) {
        if (binding.target.type === "effectParam") effectIds.add(binding.target.effectId);
      }
    }
    return effectIds;
  }, [automationHover, document.automationClips, selected]);
  const selectedMarks = useMemo(
    () => markRefLookup(sequenceSelection?.type === "marks" ? sequenceSelection.marks : []),
    [sequenceSelection]
  );

  useEffect(() => {
    const target = canvas.current;
    if (!target) return;
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
      if (row.kind === "effects" && selectedLaneIndex === row.laneIndex) {
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
      ctx.fillStyle = row.kind === "effects" ? SEQUENCE_COLORS.textMuted : SEQUENCE_COLORS.automation;
      const lane = document.lanes[row.laneIndex];
      if (lane === undefined) throw new Error("Timeline row has no lane.");
      const label = row.kind === "effects" ? lane.label : "Automation";
      const labelLayout = laneLabelLayout(lane);
      const labelY = y + row.height / 2;
      if (row.kind === "effects" && lane.kind === "group") {
        drawDisclosure(ctx, labelLayout.disclosureX, labelY, collapsedGroups.has(lane.target.fixture));
      }
      ctx.fillText(fitCanvasLabel(ctx, label, left - labelLayout.textX - THEME_METRICS.sequenceLabelX), labelLayout.textX, labelY + THEME_METRICS.sequenceLabelYOffset);
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
    drawMarkRulerLabel(ctx, audioStripBottom, markRulerHeight, left);
    ctx.strokeStyle = SEQUENCE_COLORS.gridFaint;
    ctx.beginPath();
    for (const y of [audioStripBottom, top]) {
      ctx.moveTo(0, y + THEME_METRICS.visualHairlineOffset);
      ctx.lineTo(rect.width, y + THEME_METRICS.visualHairlineOffset);
    }
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
      selected,
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
    for (const clip of visibleClips) {
      if (clip.rect.x + clip.rect.width < left || clip.rect.x > rect.width || clip.rect.y + clip.rect.height < top || clip.rect.y > rect.height) {
        continue;
      }
      const hoverResize = hover?.kind === "effect" && hover.effectId === clip.effect.id ? hover.resize : null;
      ctx.fillStyle = SEQUENCE_COLORS.textFaint;
      ctx.fillRect(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
      const expectedRasterKey = clipRasters.expectedRasterKeys.get(clip.effect.id) ?? null;
      const raster = expectedRasterKey === null ? null : clipRasters.rasters.get(expectedRasterKey) ?? null;
      const rasterError = clipRasters.errors.has(clip.effect.id);
      if (raster !== null) {
        drawClipRaster(ctx, raster, clip.rect);
      }
      if (rasterError) {
        drawClipRasterWarning(ctx, clip.rect);
      }
      const automationTargeted = activeAutomationTargetEffectIds.has(clip.effect.id);
      if (automationTargeted) {
        ctx.fillStyle = SEQUENCE_COLORS.accentSubtle;
        ctx.fillRect(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
      }
      if (hoverResize !== null) {
        ctx.fillStyle = SEQUENCE_COLORS.overlay;
        ctx.fillRect(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
      }
      const clipSelected = selectedEffectIds.has(clip.effect.id) || (selected?.type === "effect" && selected.id === clip.effect.id);
      ctx.strokeStyle = clipSelected ? SEQUENCE_COLORS.clipSelected : hoverResize !== null ? SEQUENCE_COLORS.clipHover : automationTargeted ? SEQUENCE_COLORS.accent : SEQUENCE_COLORS.clipBorder;
      ctx.lineWidth = clipSelected || hoverResize !== null || automationTargeted ? THEME_METRICS.visualLineWidthStrong : THEME_METRICS.visualLineWidth;
      ctx.strokeRect(clip.rect.x + THEME_METRICS.visualHairlineOffset, clip.rect.y + THEME_METRICS.visualHairlineOffset, Math.max(0, clip.rect.width - THEME_METRICS.visualLineWidth), Math.max(0, clip.rect.height - THEME_METRICS.visualLineWidth));
      if (hoverResize === "left" || hoverResize === "right") {
        const handleX = hoverResize === "left" ? clip.rect.x : clip.rect.x + clip.rect.width;
        ctx.fillStyle = SEQUENCE_COLORS.warning;
        ctx.fillRect(handleX - THEME_METRICS.sequenceClipHandleHalfWidth, clip.rect.y + THEME_METRICS.sequenceClipHandleInset, THEME_METRICS.sequenceClipHandleHalfWidth * 2, Math.max(THEME_METRICS.sequenceClipHandleHeight, clip.rect.height - THEME_METRICS.sequenceClipHandleInset * 2));
      }
    }
    const markXs = markTimes.map((time) => left + (time - scrollXSeconds) * viewport.pxPerSecond);
    for (const clip of visibleAutomationClips) {
      const selectedClip = sequenceSelection?.type === "clips" && sequenceSelection.automationIds.includes(clip.clip.id);
      const hoverResize = automationHover?.clipId === clip.clip.id ? automationHover.resize : null;
      const choosingCandidate = automationClipChooser !== null;
      const activePointIndex = drag.current?.kind === "automationPoint" && drag.current.clipId === clip.clip.id ? drag.current.index : null;
      drawAutomationClip(ctx, clip, {
        label: automationClipLabel(document, clip.clip),
        selected: selectedClip,
        hovered: hoverResize !== null,
        choosing: choosingCandidate,
        resize: hoverResize ?? "none",
        activePointIndex,
        guide: automationCurveDraft?.id === clip.clip.id ? automationCurveDraft.guide : null,
        markXs
      });
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
      const box = normalizedRect(marquee.startX, marquee.startY, marquee.x, marquee.y);
      ctx.fillStyle = marquee.mode === "marks" ? SEQUENCE_COLORS.markMarqueeFill : SEQUENCE_COLORS.effectMarqueeFill;
      ctx.strokeStyle = marquee.mode === "marks" ? SEQUENCE_COLORS.markMarquee : SEQUENCE_COLORS.warning;
      ctx.lineWidth = THEME_METRICS.visualLineWidth;
      ctx.fillRect(box.x, box.y, box.width, box.height);
      ctx.strokeRect(box.x + THEME_METRICS.visualHairlineOffset, box.y + THEME_METRICS.visualHairlineOffset, Math.max(0, box.width - THEME_METRICS.visualLineWidth), Math.max(0, box.height - THEME_METRICS.visualLineWidth));
    }

  }, [activeAutomationTargetEffectIds, stripResizeHover, automationCurveDraft, collapsedGroups, automationClipChooser, automationHover, markTimes, rows, document, rowResizeHover, left, top, audioStripTop, audioStripHeight, scrollbarHeight, settings, viewport, visibleClips, visibleAutomationClips, selected, sequenceSelection, selectedEffectIds, selectedMarks, selectedLaneIndex, selectedTimeSeconds, marquee, visibleMarkCollections, targetMarkCollectionKey, marksLaneOnly, audioStripBottom, markRulerHeight, markDrafts, hover, clipRasters]);

  const seekTimeFromCanvasX = (x: number) =>
    clamp(Math.round((viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond) / SEQUENCE_CANVAS.scrubStepSeconds) * SEQUENCE_CANVAS.scrubStepSeconds, 0, document.durationSeconds);
  const seekFromCanvas = (event: MouseEvent<HTMLCanvasElement>) => {
    const positionSeconds = seekTimeFromCanvasX(Math.max(left, event.nativeEvent.offsetX));
    void runSnapshotCommand(() => commands.audioSeek(positionSeconds));
  };
  const timeFromCanvasX = (x: number) => clamp(roundToNanosecond(viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond), 0, document.durationSeconds);
  /** Range edges snap to marks with Alt, like other timeline drags. */
  const rangeTimeFromCanvasX = (x: number, snap: boolean) => {
    const seconds = timeFromCanvasX(x);
    return snap ? snapToMark(seconds, markTimes, viewport.pxPerSecond) ?? seconds : seconds;
  };
  const rangeFromDrag = (anchorSeconds: number, seconds: number): PlaybackRange =>
    ({ startSeconds: Math.min(anchorSeconds, seconds), endSeconds: Math.max(anchorSeconds, seconds) });
  // The playhead's head is in the ruler, so range edges under it stay grabbable in the waveform strip.
  const seekHoverAt = (x: number, y: number): SeekHover => {
    const transport = useAppStore.getState().snapshot?.audioTransport ?? null;
    const near = (seconds: number) =>
      Math.abs(left + (seconds - viewport.scrollXSeconds) * viewport.pxPerSecond - x) <= THEME_METRICS.sequenceTransportHitHalfWidth;
    const seconds = seekTimeFromCanvasX(x);
    if (transport !== null && y < audioStripTop && near(transport.positionSeconds)) return { target: "playhead", seconds };
    const range = transport?.range ?? null;
    if (range !== null && (near(range.startSeconds) || near(range.endSeconds))) return { target: "rangeEdge", seconds };
    if (transport !== null && near(transport.homeSeconds)) return { target: "home", seconds };
    return { target: "time", seconds };
  };
  const effectTree = useMemo(
    () => definitionTree(document.effectDefinitions, (definition) => definition.effect.path),
    [document.effectDefinitions]
  );
  /**
   * The collection a new mark goes into: the given one, else the active one, creating a collection
   * when the sequence has none. It reads the latest document, so queued taps see earlier taps' edits.
   */
  const markCollectionForEdit = async (collectionKey: string | null) => {
    const guiDocument = useAppStore.getState().guiDocument;
    if (guiDocument?.type !== "sequence") throw new Error("Marks can only be added to an open sequence.");
    const collections = guiDocument.document.markCollections;
    let key = collectionKey ?? activeMarkCollection(collections, activeMarkCollectionKey)?.key ?? null;
    if (key === null) {
      const newCollectionKey = nextCollectionKey("Marks", collections);
      await runGuiEditCommand((request) =>
        commands.applySequenceGuiEdit(request, {
          type: "createMarkCollection",
          name: newCollectionKey,
          color: defaultMarkColor(collections.length)
        })
      );
      key = newCollectionKey;
      setActiveMarkCollectionKey(key);
    }
    if (!visibleMarkCollectionKeys.has(key)) setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, key]));
    return key;
  };
  const addEffectFromContextMenu = async (definition: SequenceEffectDefinition, menu: LaneContextMenu) => {
    const hasMarksParams = definition.params.some((param) => param.kind === "marks");
    const markCollectionKey = hasMarksParams ? await markCollectionForEdit(null) : null;
    const target = document.lanes[menu.laneIndex]?.target ?? document.lanes[0]?.target;
    if (target === undefined) return;
    const scope: SequenceEffectScope = "wholeTarget";
    await runGuiEditCommand((request) =>
      commands.applySequenceGuiEdit(request, {
        type: "addEffect",
        initialColor: THEME_COLORS.white,
        effect: definition.effect,
        target,
        scope,
        startSeconds: menu.startSeconds,
        markCollectionKey
      })
    );
  };
  const addMark = async (collectionKey: string | null, timeSeconds: number) => {
    const key = await markCollectionForEdit(collectionKey);
    await runGuiEditCommand((request) =>
      commands.applySequenceGuiEdit(request, {
        type: "addMark",
        collectionKey: key,
        timeSeconds
      })
    );
  };
  // Taps run in order, so fast tapping creates a missing collection once.
  const tapQueue = useRef<Promise<void>>(Promise.resolve());
  const tapMark = useEffectEvent(() => {
    const clock = playheadClockRef.current;
    if (clock === null) throw new Error("A mark was tapped without a playhead.");
    const timeSeconds = clock();
    tapQueue.current = tapQueue.current
      .then(() => addMark(null, timeSeconds))
      .catch((error: unknown) => { useAppStore.getState().setError(String(error)); });
  });
  useEffect(() => {
    const listener = () => { tapMark(); };
    window.addEventListener(TAP_MARK_EVENT, listener);
    return () => { window.removeEventListener(TAP_MARK_EVENT, listener); };
  }, []);
  const addAutomationClipFromContextMenu = async (menu: LaneContextMenu) => {
    await runGuiEditCommand((request) =>
      commands.applySequenceGuiEdit(request, {
        type: "addAutomationClip",
        startSeconds: menu.startSeconds,
        durationSeconds: Math.min(2, Math.max(0.000000001, document.durationSeconds - menu.startSeconds)),
        rowTarget: targetAtLane(document, menu.laneIndex)
      })
    );
  };
  const chooseAutomationClip = (clipId: number) => {
    if (automationClipChooser === null) return;
    const chooser = automationClipChooser;
    void runGuiEditCommand((request) =>
      commands.applySequenceGuiEdit(request, {
        type: "bindAutomationParam",
        clipId,
        target: chooser.target
      })
    ).then(() => {
      setAutomationClipChooser(null);
    });
  };
  const deleteSelectedEffect = async (effectId: number) => {
    await runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, { type: "deleteEffect", id: effectId }));
    setSelected(null);
    updateSequenceSelection(null);
  };
  const deleteAutomationClip = async (clipId: number) => {
    await runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, { type: "deleteAutomationClip", id: clipId }));
    setSelected(null);
    updateSequenceSelection(null);
  };

  const splitAutomationClip = async (clipId: number, timeSeconds: number) => {
    await runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, { type: "splitAutomationClip", id: clipId, timeSeconds }));
  };

  const commitAutomationCurve = (clipId: number, curve: AutomationCurvePoint[], origin: GuiDocumentRequest | null) => {
    setAutomationCurveDraft({ id: clipId, curve, guide: { time: null, value: null } });
    void runGuiEditCommand((request) =>
      commands.applySequenceGuiEdit(request, { type: "updateAutomationCurve", id: clipId, curve }), origin
    ).finally(() => {
      setAutomationCurveDraft(null);
    });
  };

  /** The clip-drag delta, snapped to marks while Alt is held. */
  const clipGestureDelta = (current: Extract<SequenceDragState, { kind: "sequence" | "automation" }>, event: { altKey: boolean; nativeEvent: { offsetX: number } }) => {
    const delta = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
    if (!event.altKey) return delta;
    const clip = current.kind === "automation"
      ? document.automationClips.find((candidate) => candidate.id === current.id)
      : document.effects.find((candidate) => candidate.id === current.id);
    if (clip === undefined) return delta;
    const end = clip.startSeconds + clip.durationSeconds;
    const edges = current.resize === "left" ? [clip.startSeconds] : current.resize === "right" ? [end] : [clip.startSeconds, end];
    return roundToNanosecond(snapDeltaToMarks(edges, delta, markTimes, viewport.pxPerSecond));
  };

  /** Move a curve point, aligning it to neighbors with Shift and snapping its time to marks with Alt. */
  const dragAutomationPoint = (layout: AutomationClipLayout, index: number, x: number, y: number, modifiers: { shiftKey: boolean; altKey: boolean }) => {
    const { clip } = layout;
    let point = automationCurvePointFromCanvas(layout.curveRect, x, y);
    let guide: AutomationGuide = { time: null, value: null };
    if (modifiers.altKey) {
      const mark = snapToMark(clip.contentStartSeconds + point.time * clip.contentDurationSeconds, markTimes, viewport.pxPerSecond);
      const time = mark === null ? null : (mark - clip.contentStartSeconds) / clip.contentDurationSeconds;
      if (time !== null && time >= 0 && time <= 1) {
        point = { ...point, time };
        guide = { ...guide, time };
      }
    }
    if (modifiers.shiftKey) {
      const aligned = alignAutomationCurvePoint(clip.curve, index, point, layout.curveRect);
      point = aligned.point;
      guide = { time: aligned.guide.time ?? guide.time, value: aligned.guide.value };
    }
    setAutomationCurveDraft({ id: clip.id, curve: moveAutomationCurvePoint(clip.curve, index, point), guide });
  };
  const deleteContextMark = async (menu: Extract<SequenceContextMenu, { kind: "mark" }>) => {
    await runGuiEditCommand((request) =>
      commands.applySequenceGuiEdit(request, {
        type: "deleteMark",
        collectionKey: menu.collectionKey,
        index: menu.index
      })
    );
    setSelected(null);
    updateSequenceSelection(null);
  };
  const retargetContextEffect = async (effectId: number, target: FixtureTarget) => {
    await runGuiEditCommand((request) => commands.applySequenceGuiEdit(request, { type: "retargetEffect", id: effectId, target }));
  };
  const laneContextMenu = sequenceContextMenu?.kind === "blank" || sequenceContextMenu?.kind === "effect" || sequenceContextMenu?.kind === "automation" ? sequenceContextMenu : null;
  const markCollectionsForMenu = () => {
    if (activeMarkCollectionKey === null) return document.markCollections;
    return [
      ...document.markCollections.filter((collection) => collection.key === activeMarkCollectionKey),
      ...document.markCollections.filter((collection) => collection.key !== activeMarkCollectionKey)
    ];
  };

  return (
    <div className="sequence-canvas-shell">
      <ContextMenu.Root onOpenChange={(open) => { if (!open) setSequenceContextMenu(null); }}>
        <ContextMenu.Trigger asChild>
          <canvas
            ref={canvas}
            className="gui-canvas"
            style={{
              ...(canvasCursor === undefined ? {} : { cursor: canvasCursor }),
              clipPath: `polygon(0 0, 100% 0, 100% calc(100% - var(--donder-scrollbar-width)), ${left}px calc(100% - var(--donder-scrollbar-width)), ${left}px 100%, 0 100%)`
            }}
            tabIndex={0}
            {...editShortcutTarget(["cut", "copy", "paste"])}
      onKeyDown={(event) => {
        if (event.key === "Escape" && automationClipChooser !== null) {
          event.preventDefault();
          setAutomationClipChooser(null);
          return;
        }
        if (event.key === "Escape" && (useAppStore.getState().snapshot?.audioTransport.range ?? null) !== null) {
          event.preventDefault();
          void runSnapshotCommand(() => commands.audioSetRange(null));
          return;
        }
        const selectedMark = selected?.type === "mark" ? { collectionKey: selected.collectionKey, index: selected.index } : null;
        const focusedEffectId = selectedEffectId(selected);
        const activeSelection = sequenceSelection ?? selectionFromSingle(selected);
        if (hasPrimaryModifier(event) && !isTextEntryElement(event.target)) {
          const key = event.key.toLowerCase();
          if ((key === "c" || key === "x") && activeSelection !== null && selectionCount(activeSelection) > 0) {
            event.preventDefault();
            const editType = key === "c" ? "copy" : "cut";
            void runGuiEditCommand((request) => commands.applySequenceSelectionEdit(request, { type: editType, selection: activeSelection })).then((result) => {
              updateSequenceSelection(result.selection);
              setSelected(singleSelectionFocus(result.selection));
              return result;
            });
            return;
          }
          if (key === "v") {
            event.preventDefault();
            const focused = document.effects.find((effect) => effect.id === focusedEffectId);
            const automation = selected?.type === "automationClip" ? document.automationClips.find((clip) => clip.id === selected.id) : undefined;
            const target = selectedTarget ?? focused?.target ?? automation?.rowTarget ?? null;
            const markTime = selectedMark === null ? undefined : document.markCollections.find((collection) => collection.key === selectedMark.collectionKey)?.marksSeconds[selectedMark.index];
            const timeSeconds = selectedTimeSeconds ?? focused?.startSeconds ?? automation?.startSeconds ?? markTime ?? 0;
            void runGuiEditCommand((request) => commands.applySequenceSelectionEdit(request, {
              type: "paste",
              anchor: { target, timeSeconds }
            })).then((result) => {
              updateSequenceSelection(result.selection);
              setSelected(singleSelectionFocus(result.selection));
              return result;
            });
            return;
          }
        }
        if (
          selectedMark !== null &&
          (event.key === "ArrowLeft" || event.key === "ArrowRight") &&
          !isTextEntryElement(event.target)
        ) {
          const collection = document.markCollections.find((candidate) => candidate.key === selectedMark.collectionKey);
          const timeSeconds = collection?.marksSeconds[selectedMark.index];
          if (collection === undefined || timeSeconds === undefined) return;
          event.preventDefault();
          event.stopPropagation();
          const deltaSeconds = (event.key === "ArrowLeft" ? -1 : 1) * (event.shiftKey ? SEQUENCE_CANVAS.shiftedNudgeSeconds : SEQUENCE_CANVAS.nudgeSeconds);
          const nextTimeSeconds = clamp(timeSeconds + deltaSeconds, 0, document.durationSeconds);
          const nextIndex = markIndexAfterMove(collection, selectedMark.index, nextTimeSeconds);
          const nextDrafts: MarkDraftLookup = new Map();
          setMarkDraft(nextDrafts, selectedMark, { collectionKey: selectedMark.collectionKey, index: selectedMark.index, timeSeconds: nextTimeSeconds, committedIndex: nextIndex });
          setMarkDrafts(nextDrafts);
          void runGuiEditCommand((request) =>
            commands.applySequenceGuiEdit(request, {
              type: "moveMark",
              collectionKey: selectedMark.collectionKey,
              index: selectedMark.index,
              timeSeconds: nextTimeSeconds
            })
          ).then(() => {
            setSelected({ type: "mark", collectionKey: selectedMark.collectionKey, index: nextIndex });
            setMarkDrafts(new Map());
          });
          return;
        }
        if ((event.key !== "Delete" && event.key !== "Backspace") || isTextEntryElement(event.target)) return;
        event.preventDefault();
        if (activeSelection !== null && selectionCount(activeSelection) > 1) {
          void runGuiEditCommand((request) => commands.applySequenceSelectionEdit(request, { type: "delete", selection: activeSelection })).then((result) => {
            updateSequenceSelection(result.selection);
            setSelected(null);
            return result;
          });
          return;
        }
        if (focusedEffectId !== null) {
          void deleteSelectedEffect(focusedEffectId);
          return;
        }
        if (selected?.type === "automationClip") {
          void deleteAutomationClip(selected.id);
          return;
        }
        if (selectedMark === null) return;
        void runGuiEditCommand((request) =>
          commands.applySequenceGuiEdit(request, {
            type: "deleteMark",
            collectionKey: selectedMark.collectionKey,
            index: selectedMark.index
          })
        ).then(() => {
          setSelected(null);
        });
      }}
      onContextMenu={(event) => {
        const x = event.nativeEvent.offsetX;
        const y = event.nativeEvent.offsetY;
        if (automationClipChooser !== null) {
          event.preventDefault();
          setSequenceContextMenu(null);
          return;
        }
        if (x >= left && inMarkRuler(y)) {
          const startSeconds = timeFromCanvasX(x);
          const markHit = hitSequenceMark(visibleMarkCollections, targetMarkCollectionKey, x, left, viewport);
          if (markHit === null) {
            setSelectedTimeSeconds(startSeconds);
            setSequenceContextMenu({ kind: "markRuler", startSeconds });
            return;
          }
          setSelected({ type: "mark", collectionKey: markHit.collectionKey, index: markHit.index });
          updateSequenceSelection({ type: "marks", marks: [{ collectionKey: markHit.collectionKey, index: markHit.index }] });
          setActiveMarkCollectionKey(markHit.collectionKey);
          setSelectedTimeSeconds(markHit.timeSeconds);
          setSequenceContextMenu({ kind: "mark", startSeconds, collectionKey: markHit.collectionKey, index: markHit.index });
          return;
        }
        if (x < left || y < top || document.lanes.length === 0) {
          event.preventDefault();
          setSequenceContextMenu(null);
          return;
        }
        const laneIndex = laneIndexFromCanvasY(y, top, viewport.scrollY, document.lanes.length, rows);
        const startSeconds = timeFromCanvasX(x);
        setSelectedLaneIndex(laneIndex);
        setSelectedTimeSeconds(startSeconds);
        const automationHit = hitTimelineClip(visibleAutomationClips, x, y);
        if (automationHit !== null) {
          setSelected({ type: "automationClip", id: automationHit.clip.id });
          const active = sequenceSelectionRef.current;
          updateSequenceSelection(active?.type === "clips" && active.automationIds.includes(automationHit.clip.id) ? active : { type: "clips", effectIds: [], automationIds: [automationHit.clip.id] });
          setSequenceContextMenu({
            kind: "automation",
            laneIndex: document.lanes.findIndex((lane) => targetsEqual(lane.target, automationHit.clip.rowTarget)),
            startSeconds,
            clipId: automationHit.clip.id
          });
          return;
        }
        const hit = hitSequence(visibleClips, x, y);
        if (hit !== null) {
          setSelected({ type: "effect", id: hit.effect.id });
          updateSequenceSelection({ type: "clips", automationIds: [], effectIds: [hit.effect.id] });
          setSequenceContextMenu({ kind: "effect", laneIndex: hit.laneIndex, startSeconds, effectId: hit.effect.id });
          return;
        }
        setSelected(null);
        updateSequenceSelection(null);
        setSequenceContextMenu({ kind: "blank", laneIndex, startSeconds });
      }}
      onDoubleClick={(event) => {
        const x = event.nativeEvent.offsetX;
        const y = event.nativeEvent.offsetY;
        // Double-clicking empty mark ruler adds a mark to the active collection.
        if (inMarkRuler(y)) {
          if (x >= left && hitSequenceMark(visibleMarkCollections, targetMarkCollectionKey, x, left, viewport) === null) {
            void addMark(null, timeFromCanvasX(x));
          }
          return;
        }
        const automationHit = x >= left && automationClipChooser === null ? hitTimelineClip(visibleAutomationClips, x, y) : null;
        if (automationHit === null || automationHit.clip.curve.length <= 1) return;
        const pointHit = hitAutomationCurvePoint(automationHit, x, y);
        if (pointHit === null) return;
        commitAutomationCurve(automationHit.clip.id, removeAutomationCurvePoint(automationHit.clip.curve, pointHit), null);
      }}
      onPointerDown={(event) => {
        gestureRequest.current = useAppStore.getState().guiRequest;
        if (event.button !== 0 || isSecondaryClick(event)) return;
        event.currentTarget.focus();
        event.currentTarget.setPointerCapture(event.pointerId);
        const x = event.nativeEvent.offsetX;
        const y = event.nativeEvent.offsetY;
        setMarkDrafts(new Map());
        const resizedStrip = stripResizeHit(y, audioStripBottom, top);
        if (resizedStrip !== null) {
          event.preventDefault();
          drag.current = {
            kind: "stripResize",
            strip: resizedStrip,
            startY: y,
            initialHeight: viewport[TIMELINE_STRIPS[resizedStrip].height],
            active: false
          };
          setStripResizeHover(resizedStrip);
          return;
        }
        if (automationClipChooser !== null) {
          const automationHit = x >= left ? hitTimelineClip(visibleAutomationClips, x, y) : null;
          if (automationHit !== null) {
            event.preventDefault();
            event.stopPropagation();
            chooseAutomationClip(automationHit.clip.id);
          }
          return;
        }
        if (x >= left && y < audioStripBottom) {
          const grabbed = seekHoverAt(x, y);
          setSeekHover(null);
          if (grabbed.target === "playhead") {
            drag.current = { kind: "playheadScrub" };
            setDragCursor("grabbing");
            seekFromCanvas(event);
            return;
          }
          const range = useAppStore.getState().snapshot?.audioTransport.range ?? null;
          if (grabbed.target === "rangeEdge" && range !== null) {
            const fromEnd = Math.abs(timeFromCanvasX(x) - range.endSeconds) <= Math.abs(timeFromCanvasX(x) - range.startSeconds);
            drag.current = { kind: "timeRange", startX: x, anchorSeconds: fromEnd ? range.startSeconds : range.endSeconds, fromEdge: true, active: false };
            return;
          }
          drag.current = { kind: "timeRange", startX: x, anchorSeconds: rangeTimeFromCanvasX(x, event.altKey), fromEdge: false, active: false };
          return;
        }
        if (inMarkRuler(y)) {
          if (x < left) return;
          const markHit = hitSequenceMark(visibleMarkCollections, targetMarkCollectionKey, x, left, viewport);
          if (markHit !== null) {
            const mark = { collectionKey: markHit.collectionKey, index: markHit.index };
            const activeSelection = sequenceSelectionRef.current;
            const wasAlreadySelected = activeSelection?.type === "marks" && activeSelection.marks.some((candidate) => candidate.collectionKey === mark.collectionKey && candidate.index === mark.index);
            const nextSelection = wasAlreadySelected && !event.shiftKey && !event.ctrlKey && !event.metaKey
              ? activeSelection
              : nextMarkSelection(activeSelection?.type === "marks" ? activeSelection : null, mark, event.shiftKey, event.ctrlKey || event.metaKey);
            updateSequenceSelection(nextSelection);
            setSelected({ type: "mark", collectionKey: mark.collectionKey, index: mark.index });
            setActiveMarkCollectionKey(markHit.collectionKey);
            setSelectedTimeSeconds(markHit.timeSeconds);
            drag.current = {
              kind: "mark",
              collectionKey: markHit.collectionKey,
              index: markHit.index,
              startX: x,
              startY: y,
              active: false,
              originalTimeSeconds: markHit.timeSeconds
            };
            return;
          }
          // Dragging across empty ruler box-selects marks by time.
          setSelectedTimeSeconds(timeFromCanvasX(x));
          setSelected(null);
          updateSequenceSelection(null);
          const state = { mode: "marks" as const, startX: x, startY: y, x, y, active: false, shift: event.shiftKey, ctrl: event.ctrlKey || event.metaKey };
          drag.current = { kind: "marquee", state };
          setMarquee(state);
          return;
        }
        if (x < left && y >= top && document.lanes.length > 0) {
          const resizeHit = rowResizeHit(y, top, viewport.scrollY, rows);
          if (resizeHit !== null) {
            event.preventDefault();
            drag.current = {
              kind: "rowResize",
              laneIndex: resizeHit.laneIndex,
              rowKind: resizeHit.rowKind,
              startY: y,
              initialHeight: rowHeightAt(viewport.rowHeights, targetAtLane(document, resizeHit.laneIndex), resizeHit.rowKind, resizeHit.rowKind === "effects" ? initialSequenceLaneHeight(settings) : automationRowHeight),
              active: false
            };
            setRowResizeHover({ laneIndex: resizeHit.laneIndex, rowKind: resizeHit.rowKind });
            return;
          }
          const laneIndex = laneIndexFromCanvasY(y, top, viewport.scrollY, document.lanes.length, rows);
          const lane = document.lanes[laneIndex];
          if (lane === undefined) return;
          const row = rowFromCanvasY(y, top, viewport.scrollY, rows);
          if (row?.kind === "effects" && lane.kind === "group" && x < laneLabelLayout(lane).textX) {
            const group = lane.target.fixture;
            setCollapsedGroups((current) => {
              const next = new Set(current);
              if (!next.delete(group)) next.add(group);
              return next;
            });
            return;
          }
          const effectIds = row?.kind === "effects" ? document.effects.filter((effect) => targetsEqual(effect.target, lane.target)).map((effect) => effect.id) : [];
          const automationIds = row?.kind === "automation" ? document.automationClips.filter((clip) => targetsEqual(clip.rowTarget, lane.target)).map((clip) => clip.id) : [];
          setSelectedLaneIndex(laneIndex);
          updateSequenceSelection(effectIds.length + automationIds.length > 0 ? { type: "clips", effectIds, automationIds } : null);
          setSelected(singleSelectionFocus({ type: "clips", effectIds, automationIds }));
          return;
        }
        const automationHit = x >= left ? hitTimelineClip(visibleAutomationClips, x, y) : null;
        if (automationHit !== null) {
          const editsCurve = !event.shiftKey && !event.ctrlKey && !event.metaKey && (sequenceSelection?.type !== "clips" || selectionCount(sequenceSelection) <= 1);
          const belowHeader = y > automationHit.rect.y + Math.min(THEME_METRICS.automationClipHeaderHeight, automationHit.rect.height);
          const pointHit = editsCurve ? hitAutomationCurvePoint(automationHit, x, y) : null;
          const lineHit = editsCurve && pointHit === null && belowHeader && automationHit.resize === "none" ? hitAutomationCurveLine(automationHit, x, y) : null;
          if (pointHit !== null || lineHit !== null) {
            setSelected({ type: "automationClip", id: automationHit.clip.id });
            updateSequenceSelection({ type: "clips", effectIds: [], automationIds: [automationHit.clip.id] });
            if (lineHit !== null) {
              setAutomationCurveDraft({ id: automationHit.clip.id, curve: insertAutomationCurvePoint(automationHit.clip.curve, lineHit.index, lineHit.point), guide: { time: null, value: null } });
            }
            drag.current = {
              kind: "automationPoint",
              clipId: automationHit.clip.id,
              index: pointHit ?? lineHit?.index ?? 0,
              active: false,
              inserted: lineHit !== null
            };
            return;
          }
          setSelected({ type: "automationClip", id: automationHit.clip.id });
          const active = sequenceSelectionRef.current;
          updateSequenceSelection(active?.type === "clips" && active.automationIds.includes(automationHit.clip.id) && !event.shiftKey && !event.ctrlKey && !event.metaKey ? active : nextAutomationSelection(active, automationHit.clip.id, event.shiftKey, event.ctrlKey || event.metaKey));
          setSelectedLaneIndex(document.lanes.findIndex((lane) => targetsEqual(lane.target, automationHit.clip.rowTarget)));
          setSelectedTimeSeconds(automationHit.clip.startSeconds);
          drag.current = {
            kind: "automation",
            id: automationHit.clip.id,
            startX: x,
            startY: y,
            active: false,
            originalStartSeconds: automationHit.clip.startSeconds,
            rowTarget: automationHit.clip.rowTarget,
            resize: automationHit.resize
          };
          return;
        }
        const hit = x >= left ? hitSequence(visibleClips, x, y) : null;
        if (hit !== null) {
          const activeSelection = sequenceSelectionRef.current;
          const wasAlreadySelected = activeSelection?.type === "clips" && activeSelection.effectIds.includes(hit.effect.id);
          const nextSelection = wasAlreadySelected && !event.shiftKey && !event.ctrlKey && !event.metaKey
            ? activeSelection
            : nextEffectSelection(activeSelection?.type === "clips" ? activeSelection : null, hit.effect.id, event.shiftKey, event.ctrlKey || event.metaKey);
          updateSequenceSelection(nextSelection);
          setSelected(nextSelection.type === "clips" ? singleEffectSelectionFocus(nextSelection.effectIds) ?? { type: "effect", id: hit.effect.id } : { type: "effect", id: hit.effect.id });
          setSelectedLaneIndex(hit.laneIndex);
          setSelectedTimeSeconds(hit.effect.startSeconds);
          drag.current = {
            kind: "sequence",
            id: hit.effect.id,
            startX: event.nativeEvent.offsetX,
            startY: event.nativeEvent.offsetY,
            active: false,
            originalStartSeconds: hit.effect.startSeconds,
            laneIndex: hit.laneIndex,
            resize: hit.resize
          };
          return;
        }
        if (x >= left && y >= top) {
        const laneIndex = laneIndexFromCanvasY(y, top, viewport.scrollY, document.lanes.length, rows);
          const timeSeconds = timeFromCanvasX(x);
          setSelectedLaneIndex(laneIndex);
          setSelectedTimeSeconds(timeSeconds);
          setSelected(null);
          updateSequenceSelection(null);
          const state = { mode: "clips" as const, startX: x, startY: y, x, y, active: false, shift: event.shiftKey, ctrl: event.ctrlKey || event.metaKey };
          drag.current = { kind: "marquee", state };
          setMarquee(state);
        }
      }}
      onPointerMove={(event) => {
        const current = drag.current;
        if (current?.kind === "stripResize") {
          if (!current.active) {
            if (Math.abs(event.nativeEvent.offsetY - current.startY) < SEQUENCE_DRAG_THRESHOLD_PX) return;
            current.active = true;
            setDragCursor("grabbing");
          }
          const strip = TIMELINE_STRIPS[current.strip];
          const height = clamp(current.initialHeight + event.nativeEvent.offsetY - current.startY, strip.minPx, strip.maxPx);
          setViewport((previous) => {
            const next = { ...previous, [strip.height]: height };
            const nextTop = SEQUENCE_CANVAS.audioStripTopPx + next.audioStripHeight + next.markRulerHeight;
            const visibleHeight = Math.max(1, canvasSize.height - nextTop);
            const maxScrollY = Math.max(0, expandedTimelineHeight(layoutRows(previous.rowHeights, revealAutomation)) - visibleHeight);
            return { ...next, scrollY: clamp(previous.scrollY, 0, maxScrollY) };
          });
          return;
        }
        if (current?.kind === "rowResize") {
          if (!current.active) {
            if (Math.abs(event.nativeEvent.offsetY - current.startY) < SEQUENCE_DRAG_THRESHOLD_PX) return;
            current.active = true;
            setDragCursor("grabbing");
          }
          const nextHeight = clamp(
            current.initialHeight + event.nativeEvent.offsetY - current.startY,
            SEQUENCE_CANVAS.minLaneHeightPx,
            SEQUENCE_CANVAS.maxLaneHeightPx
          );
          setViewport((previous) => {
            const target = document.lanes[current.laneIndex]?.target;
            if (target === undefined) return previous;
            const complete = completeRowHeights(previous.rowHeights, document, settings);
            const heights = complete[target.fixture];
            if (heights === undefined) throw new Error("Timeline row heights are missing.");
            const rowHeights = { ...complete, [target.fixture]: { ...heights, [current.rowKind]: nextHeight } };
            const maxScrollY = Math.max(0, expandedTimelineHeight(layoutRows(rowHeights, revealAutomation)) - Math.max(1, canvasSize.height - top));
            return { ...previous, rowHeights, scrollY: clamp(previous.scrollY, 0, maxScrollY) };
          });
          return;
        }
        if (current?.kind === "playheadScrub") {
          seekFromCanvas(event);
          return;
        }
        if (current?.kind === "timeRange") {
          if (!current.active) {
            if (Math.abs(event.nativeEvent.offsetX - current.startX) < SEQUENCE_DRAG_THRESHOLD_PX) return;
            current.active = true;
          }
          setRangeDraft(rangeFromDrag(current.anchorSeconds, rangeTimeFromCanvasX(event.nativeEvent.offsetX, event.altKey)));
          return;
        }
        if (current?.kind === "marquee") {
          const next = {
            ...current.state,
            x: event.nativeEvent.offsetX,
            y: event.nativeEvent.offsetY,
            active: current.state.active || Math.hypot(event.nativeEvent.offsetX - current.state.startX, event.nativeEvent.offsetY - current.state.startY) >= SEQUENCE_DRAG_THRESHOLD_PX
          };
          current.state = next;
          setMarquee(next);
          if (next.active) {
            const selectedByBox = next.mode === "clips"
              ? selectionFromMarqueeEffects(visibleClips, visibleAutomationClips, next)
              : selectionFromMarqueeMarks(visibleMarkCollections, next, left, viewport);
            updateSequenceSelection(mergeSequenceSelection(sequenceSelectionRef.current, selectedByBox, next.shift, next.ctrl));
            setSelected(null);
          }
          return;
        }
        if (current?.kind === "mark") {
          if (!current.active) {
            if (Math.hypot(event.nativeEvent.offsetX - current.startX, event.nativeEvent.offsetY - current.startY) < SEQUENCE_DRAG_THRESHOLD_PX) return;
            current.active = true;
          }
          const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
          const timeSeconds = clamp(current.originalTimeSeconds + deltaSeconds, 0, document.durationSeconds);
          setSelected({ type: "mark", collectionKey: current.collectionKey, index: current.index });
          const collection = document.markCollections.find((candidate) => candidate.key === current.collectionKey);
          const committedIndex = collection === undefined ? current.index : markIndexAfterMove(collection, current.index, timeSeconds);
          const activeSelection = sequenceSelectionRef.current;
          if (activeSelection?.type === "marks" && activeSelection.marks.length > 1 && activeSelection.marks.some((mark) => mark.collectionKey === current.collectionKey && mark.index === current.index)) {
            const constrainedDelta = constrainMarkDelta(document, activeSelection.marks, deltaSeconds);
            setMarkDrafts(markMoveDrafts(document, activeSelection.marks, constrainedDelta));
          } else {
            const nextDrafts: MarkDraftLookup = new Map();
            setMarkDraft(nextDrafts, { collectionKey: current.collectionKey, index: current.index }, { collectionKey: current.collectionKey, index: current.index, timeSeconds, committedIndex });
            setMarkDrafts(nextDrafts);
          }
          setAutomationDrafts([]);
          setRevealAutomation(false);
          setGroupDraft([]);
          return;
        }
        if (current?.kind === "automationPoint") {
          const layout = visibleAutomationClips.find((candidate) => candidate.clip.id === current.clipId);
          if (layout === undefined) return;
          dragAutomationPoint(layout, current.index, event.nativeEvent.offsetX, event.nativeEvent.offsetY, event);
          current.active = true;
          return;
        }
        if (!current) {
          const x = event.nativeEvent.offsetX;
          const y = event.nativeEvent.offsetY;
          const stripResize = stripResizeHit(y, audioStripBottom, top);
          setStripResizeHover(stripResize);
          const nextSeekHover = stripResize === null && x >= left && y < audioStripBottom ? seekHoverAt(x, y) : null;
          setSeekHover((previous) =>
            previous?.target === nextSeekHover?.target && previous?.seconds === nextSeekHover?.seconds ? previous : nextSeekHover
          );
          if (stripResize !== null || nextSeekHover !== null) {
            setRowResizeHover(null);
            setHover(null);
            setAutomationHover(null);
            return;
          }
          const resizeHit = x < left && y >= top
            ? rowResizeHit(y, top, viewport.scrollY, rows)
            : null;
          setRowResizeHover(resizeHit === null ? null : { laneIndex: resizeHit.laneIndex, rowKind: resizeHit.rowKind });
          const automationHit = x >= left && y >= top ? hitTimelineClip(visibleAutomationClips, x, y) : null;
          const choosingAutomation = automationClipChooser !== null;
          const hit = x >= left && y >= top && automationHit === null && !choosingAutomation ? hitSequence(visibleClips, x, y) : null;
          const markHit = inMarkRuler(y) && !choosingAutomation ? hitSequenceMark(visibleMarkCollections, targetMarkCollectionKey, x, left, viewport) : null;
          const nextHover: SequenceHover =
            hit !== null
              ? { kind: "effect", effectId: hit.effect.id, resize: hit.resize }
              : markHit !== null
                ? { kind: "mark", collectionKey: markHit.collectionKey, index: markHit.index }
                : null;
          const nextAutomationHover: AutomationHover | null = automationHit === null ? null : { kind: "automation", clipId: automationHit.clip.id, resize: choosingAutomation ? "none" : automationHit.resize };
          setHover((previous) =>
            sequenceHoverEqual(previous, nextHover) ? previous : nextHover
          );
          setAutomationHover((previous) =>
            automationHoverEqual(previous, nextAutomationHover) ? previous : nextAutomationHover
          );
          return;
        }
        if (!current.active) {
          if (Math.hypot(event.nativeEvent.offsetX - current.startX, event.nativeEvent.offsetY - current.startY) < SEQUENCE_DRAG_THRESHOLD_PX) return;
          current.active = true;
          setDragCursor("grabbing");
          const selection = sequenceSelectionRef.current;
          if (current.resize === "none" && selection?.type === "clips" && selection.automationIds.length > 0) {
            const sourceTarget = current.kind === "automation" ? current.rowTarget : targetAtLane(document, current.laneIndex);
            const sourceKind = current.kind === "automation" ? "automation" : "effects";
            const before = rows.find((row) => targetsEqual(row.target, sourceTarget) && row.kind === sourceKind);
            const expanded = layoutRows(viewport.rowHeights, true);
            const after = expanded.find((row) => targetsEqual(row.target, sourceTarget) && row.kind === sourceKind);
            if (before === undefined || after === undefined) throw new Error("Dragged clip row is missing.");
            // Keep the grabbed row under the pointer while exposing empty drop rows.
            setViewport((previous) => ({ ...previous, scrollY: previous.scrollY + after.top - before.top }));
            setRevealAutomation(true);
            return;
          }
        }
        const selection = sequenceSelectionRef.current;
        if (selection?.type !== "clips") return;
        const sourceLane = current.kind === "automation" ? document.lanes.findIndex((lane) => targetsEqual(lane.target, current.rowTarget)) : current.laneIndex;
        const destinationLane = laneIndexFromCanvasY(event.nativeEvent.offsetY, top, viewport.scrollY, document.lanes.length, rows);
        const gesture = clipSelectionGesture(document, selection, current.resize, clipGestureDelta(current, event), destinationLane - sourceLane, hasPrimaryModifier(event) ? "stretch" : "crop");
        setGroupDraft(gesture.effects);
        setAutomationDrafts(gesture.automation);
      }}
      onPointerUp={(event) => {
        const current = drag.current;
        drag.current = null;
        setDragCursor(null);
        setMarquee(null);
        if (current?.kind === "stripResize") {
          setStripResizeHover(null);
          return;
        }
        if (current?.kind === "rowResize") {
          setRowResizeHover(null);
          return;
        }
        if (current?.kind === "timeRange") {
          if (current.active) {
            const range = rangeFromDrag(current.anchorSeconds, rangeTimeFromCanvasX(event.nativeEvent.offsetX, event.altKey));
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
        if (current?.kind === "marquee") return;
        if (current?.kind === "mark") {
          if (!current.active) {
            setMarkDrafts(new Map());
              setGroupDraft([]);
            return;
          }
          const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
          const activeSelection = sequenceSelectionRef.current;
          if (activeSelection?.type === "marks" && activeSelection.marks.some((mark) => mark.collectionKey === current.collectionKey && mark.index === current.index)) {
            const constrainedDelta = constrainMarkDelta(document, activeSelection.marks, deltaSeconds);
            if (constrainedDelta === 0) {
              setMarkDrafts(new Map());
              return;
            }
            void runGuiEditCommand((request) => commands.applySequenceSelectionEdit(request, {
              type: "moveMarks",
              marks: activeSelection.marks,
              timeDeltaSeconds: constrainedDelta
            }), gestureRequest.current).then((result) => {
              updateSequenceSelection(result.selection);
              setSelected(null);
              setMarkDrafts(new Map());
              return result;
            });
            return;
          }
          const timeSeconds = clamp(current.originalTimeSeconds + deltaSeconds, 0, document.durationSeconds);
          if (timeSeconds === current.originalTimeSeconds) {
            setMarkDrafts(new Map());
            return;
          }
          const collection = document.markCollections.find((candidate) => candidate.key === current.collectionKey);
          const nextIndex = collection === undefined ? current.index : markIndexAfterMove(collection, current.index, timeSeconds);
          void runGuiEditCommand((request) =>
            commands.applySequenceGuiEdit(request, {
              type: "moveMark",
              collectionKey: current.collectionKey,
              index: current.index,
              timeSeconds
            }), gestureRequest.current
          ).then(() => {
            setSelected({ type: "mark", collectionKey: current.collectionKey, index: nextIndex });
            setMarkDrafts(new Map());
          });
          return;
        }
        if (current?.kind === "automationPoint") {
          const committedDraft = automationCurveDraft;
          if (!(current.active || current.inserted) || committedDraft === null) {
            setAutomationCurveDraft(null);
            return;
          }
          commitAutomationCurve(committedDraft.id, committedDraft.curve, gestureRequest.current);
          return;
        }
        if (current?.kind !== "sequence" && current?.kind !== "automation") return;
        const clearDrafts = () => { setGroupDraft([]); setAutomationDrafts([]); setRevealAutomation(false); };
        const selection = sequenceSelectionRef.current;
        if (!current.active || selection?.type !== "clips") { clearDrafts(); return; }
        const sourceLane = current.kind === "automation" ? document.lanes.findIndex((lane) => targetsEqual(lane.target, current.rowTarget)) : current.laneIndex;
        const destinationLane = laneIndexFromCanvasY(event.nativeEvent.offsetY, top, viewport.scrollY, document.lanes.length, rows);
        const gesture = clipSelectionGesture(document, selection, current.resize, clipGestureDelta(current, event), destinationLane - sourceLane, hasPrimaryModifier(event) ? "stretch" : "crop");
        if (!gesture.changed) { clearDrafts(); return; }
        void runGuiEditCommand((request) => commands.applySequenceSelectionEdit(request, gesture.edit), gestureRequest.current).then((result) => {
          updateSequenceSelection(result.selection);
          setSelected(singleSelectionFocus(result.selection));
          const laneDelta = gesture.edit.type === "moveClips" ? gesture.edit.laneDelta : 0;
          setSelectedTarget(targetAtLane(document, sourceLane + laneDelta));
          const grabbed = current.kind === "automation" ? gesture.automation.find((clip) => clip.id === current.id) : gesture.effects.find((clip) => clip.id === current.id);
          if (grabbed !== undefined) setSelectedTimeSeconds(grabbed.startSeconds);
        }).finally(clearDrafts);
      }}
      onPointerCancel={() => {
        drag.current = null;
        gestureRequest.current = null;
        setGroupDraft([]);
        setAutomationDrafts([]);
        setAutomationCurveDraft(null);
        setMarkDrafts(new Map());
        setRevealAutomation(false);
        setDragCursor(null);
        setMarquee(null);
        setRangeDraft(null);
      }}
      onPointerLeave={() => {
        if (drag.current === null) {
          setSeekHover(null);
          setHover(null);
          setAutomationHover(null);
          setRowResizeHover(null);
          setStripResizeHover(null);
        }
      }}
          />
        </ContextMenu.Trigger>
        {sequenceContextMenu !== null && (
          <ContextMenu.Portal container={overlayContainer}>
            <ContextMenu.Content className="menu-content">
              {laneContextMenu !== null && (
                <ContextMenu.Sub>
                  <ContextMenu.SubTrigger className="menu-item">
                    Add Effect <ChevronRight size={THEME_METRICS.iconSizeSmall} aria-hidden />
                  </ContextMenu.SubTrigger>
                  <ContextMenu.Portal container={overlayContainer}>
                    <ContextMenu.SubContent className="menu-content">
                      <DefinitionMenuItems
                        menu={ContextMenu}
                        tree={effectTree}
                        label={(definition) => definition.name}
                        itemKey={(definition) => `${definition.effect.moduleId}:${definition.effect.path}:${definition.effect.effectName}`}
                        onSelect={(definition) => void addEffectFromContextMenu(definition, laneContextMenu)}
                        empty="No effects"
                      />
                    </ContextMenu.SubContent>
                  </ContextMenu.Portal>
                </ContextMenu.Sub>
              )}
              <ContextMenu.Item
                className="menu-item"
                onSelect={() => {
                  void runSnapshotCommand(() => commands.audioSeek(sequenceContextMenu.startSeconds));
                }}
              >
                Set Playhead Here
              </ContextMenu.Item>
              <ContextMenu.Sub>
                <ContextMenu.SubTrigger className="menu-item">
                  Add Mark <ChevronRight size={THEME_METRICS.iconSizeSmall} aria-hidden />
                </ContextMenu.SubTrigger>
                <ContextMenu.Portal container={overlayContainer}>
                  <ContextMenu.SubContent className="menu-content">
                    {document.markCollections.length === 0 ? (
                      <ContextMenu.Item className="menu-item" onSelect={() => void addMark(null, sequenceContextMenu.startSeconds)}>
                        Marks
                      </ContextMenu.Item>
                    ) : (
                      markCollectionsForMenu().map((collection) => (
                        <ContextMenu.Item
                          key={collection.key}
                          className="menu-item"
                          onSelect={() => void addMark(collection.key, sequenceContextMenu.startSeconds)}
                        >
                          <span style={{ color: collection.color }}>{collection.key}</span>
                        </ContextMenu.Item>
                      ))
                    )}
                  </ContextMenu.SubContent>
                </ContextMenu.Portal>
              </ContextMenu.Sub>
              {laneContextMenu !== null && (
                <ContextMenu.Item className="menu-item" onSelect={() => void addAutomationClipFromContextMenu(laneContextMenu)}>
                  Add Automation Clip
                </ContextMenu.Item>
              )}
              {sequenceContextMenu.kind === "effect" && (
                <>
                  <ContextMenu.Separator className="menu-separator" />
                  <ContextMenu.Sub>
                    <ContextMenu.SubTrigger className="menu-item">
                      Retarget Effect <span className="shortcut"><ArrowRight size={THEME_METRICS.iconSizeExtraSmall} aria-hidden="true" /></span>
                    </ContextMenu.SubTrigger>
                    <ContextMenu.Portal container={overlayContainer}>
                      <ContextMenu.SubContent className="menu-content">
                        {document.lanes.map((lane) => (
                          <ContextMenu.Item
                            key={lane.target.fixture}
                            className="menu-item"
                            onSelect={() => void retargetContextEffect(sequenceContextMenu.effectId, lane.target)}
                          >
                            {lane.label}
                          </ContextMenu.Item>
                        ))}
                      </ContextMenu.SubContent>
                    </ContextMenu.Portal>
                  </ContextMenu.Sub>
                  <ContextMenu.Item className="menu-item danger" onSelect={() => void deleteSelectedEffect(sequenceContextMenu.effectId)}>
                    <Trash2 size={THEME_METRICS.iconSizeSmall} /> Delete Effect
                  </ContextMenu.Item>
                </>
              )}
              {sequenceContextMenu.kind === "automation" && (
                <>
                  <ContextMenu.Separator className="menu-separator" />
                  <ContextMenu.Item className="menu-item" onSelect={() => void splitAutomationClip(sequenceContextMenu.clipId, sequenceContextMenu.startSeconds)}>
                    <Scissors size={THEME_METRICS.iconSizeSmall} /> Split Automation Clip Here
                  </ContextMenu.Item>
                  <ContextMenu.Item className="menu-item danger" onSelect={() => void deleteAutomationClip(sequenceContextMenu.clipId)}>
                    <Trash2 size={THEME_METRICS.iconSizeSmall} /> Delete Automation Clip
                  </ContextMenu.Item>
                </>
              )}
              {sequenceContextMenu.kind === "mark" && (
                <>
                  <ContextMenu.Separator className="menu-separator" />
                  <ContextMenu.Item className="menu-item danger" onSelect={() => void deleteContextMark(sequenceContextMenu)}>
                    <Trash2 size={THEME_METRICS.iconSizeSmall} /> Delete Mark
                  </ContextMenu.Item>
                </>
              )}
            </ContextMenu.Content>
          </ContextMenu.Portal>
        )}
      </ContextMenu.Root>
      <div className="sequence-gutter-scrollbar-divider" style={{ left }} aria-hidden="true" />
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
        seekHover={seekHover}
        rangeDraft={rangeDraft}
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
  seekHover,
  rangeDraft,
  canvasSize,
  playheadClockRef
}: {
  document: SequenceEditorDocument;
  viewport: SequenceViewport;
  setViewport: Dispatch<SetStateAction<SequenceViewport>>;
  left: number;
  audioStripTop: number;
  seekZoneHeight: number;
  followMode: SequenceFollowMode | null;
  seekHover: SeekHover | null;
  rangeDraft: PlaybackRange | null;
  canvasSize: { width: number; height: number };
  playheadClockRef: RefObject<(() => number) | null>;
}) {
  const host = useSequenceEditorHost();
  const { store: useAppStore } = host;

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
      seekHover={seekHover}
      rangeDraft={rangeDraft}
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
  seekHover,
  rangeDraft,
  canvasSize,
  playheadClockRef
}: {
  document: SequenceEditorDocument;
  transport: AudioTransportViewSnapshot;
  viewport: SequenceViewport;
  setViewport: Dispatch<SetStateAction<SequenceViewport>>;
  left: number;
  audioStripTop: number;
  seekZoneHeight: number;
  followMode: SequenceFollowMode | null;
  seekHover: SeekHover | null;
  rangeDraft: PlaybackRange | null;
  canvasSize: { width: number; height: number };
  playheadClockRef: RefObject<(() => number) | null>;
}) {
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
            {seekHover.target === "home" ? "Home: Stop returns here" : formatSeconds(seekHover.seconds)}
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

function automationClipLabel(document: SequenceEditorDocument, clip: SequenceAutomationClip) {
  const primary = clip.bindings[0];
  const detached = clip.detachedBindings[0];
  if (primary === undefined && detached === undefined) return "Unassigned automation";
  const target = primary?.target ?? detached?.target;
  if (target === undefined) throw new Error("Automation clip label has no target");
  const label = primary === undefined
    ? `Detached: ${detachedAutomationTargetLabel(target)}`
    : automationTargetLabel(document, target);
  const additionalBindingCount = clip.bindings.length + clip.detachedBindings.length - 1;
  return additionalBindingCount > 0 ? `${label} +${additionalBindingCount}` : label;
}

function automationTargetLabel(document: SequenceEditorDocument, target: SequenceAutomationTarget) {
  if (target.type === "effectParam") {
    const effect = document.effects.find((candidate) => candidate.id === target.effectId);
    if (effect === undefined) throw new Error(`Automation target effect ${target.effectId} is missing`);
    return `${effect.effect}: ${target.param}`;
  }
  const node = document.compositionGraph.nodes.find((candidate) => candidate.id === target.nodeId);
  if (node === undefined || node.kind.type !== "operator") {
    throw new Error(`Automation target operator ${target.nodeId} is missing`);
  }
  return `${graphOperatorDefinition(document.compositionGraph.operatorCatalog, node.kind.operator).displayName}: ${target.param}`;
}

function detachedAutomationTargetLabel(target: SequenceAutomationTarget) {
  return target.type === "effectParam"
    ? `Effect ${target.effectId}: ${target.param}`
    : `Operator ${target.nodeId}: ${target.param}`;
}

function scheduleSequenceViewportStateSave(host: SequenceEditorHost, reference: GuiObjectRef, state: PersistedSequenceViewportState) {
  const { commands, store: useAppStore } = host;

  const { path, objectKey, ownedPath } = reference;
  const store = useAppStore.getState();
  if (store.restoreState !== null) {
    store.setRestoreState({ ...store.restoreState, sequenceViewports: { ...store.restoreState.sequenceViewports, [objectViewKey(reference)]: state } });
  }
  scheduleViewStateSave(JSON.stringify(["sequence", objectViewKey(reference)]), () => commands.saveSequenceViewportState({ path, objectKey, ownedPath, state }),
    (error) => { useAppStore.getState().setError(String(error)); });
}

/** A lane label's disclosure triangle and text positions, indented by depth like a file tree. */
function laneLabelLayout(lane: SequenceLane) {
  const disclosureX = THEME_METRICS.sequenceLabelX + lane.depth * THEME_METRICS.sequenceLaneIndent;
  return { disclosureX, textX: disclosureX + THEME_METRICS.sequenceDisclosureSize + THEME_METRICS.sequenceDisclosureGap };
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

function completeRowHeights(heights: SequenceRowHeightMap, document: SequenceEditorDocument, settings: AppSettings | null): SequenceRowHeightMap {
  const missing = document.lanes.filter((lane) => heights[lane.target.fixture] === undefined);
  if (missing.length === 0) return heights;
  const defaultHeight = initialSequenceLaneHeight(settings);
  return { ...heights, ...Object.fromEntries(missing.map((lane) => [lane.target.fixture, { effects: defaultHeight, automation: automationLaneRowHeight(defaultHeight) }])) };
}

function sequenceViewportFromPersisted(state: PersistedSequenceViewportState | undefined, document: SequenceEditorDocument, settings: AppSettings | null): SequenceViewport {
  return {
    pxPerSecond: state === undefined ? settings?.sequenceInitialPxPerSecond ?? SEQUENCE_CANVAS.initialPxPerSecond : clamp(state.pxPerSecond, SEQUENCE_CANVAS.minPxPerSecond, SEQUENCE_CANVAS.maxZoomPxPerSecond),
    audioStripHeight: clamp(state?.audioStripHeightPx ?? SEQUENCE_CANVAS.initialAudioStripHeightPx, TIMELINE_STRIPS.audio.minPx, TIMELINE_STRIPS.audio.maxPx),
    markRulerHeight: clamp(state?.markRulerHeightPx ?? SEQUENCE_CANVAS.initialMarkRulerHeightPx, TIMELINE_STRIPS.marks.minPx, TIMELINE_STRIPS.marks.maxPx),
    rowHeights: restoreRowHeights(document.lanes, state?.rowHeights, initialSequenceLaneHeight(settings)),
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

function drawClipRasterWarning(
  ctx: CanvasRenderingContext2D,
  rect: { x: number; y: number; width: number; height: number }
) {
  const size = Math.min(THEME_METRICS.rasterWarningSizeMax, Math.max(THEME_METRICS.rasterWarningSizeMin, rect.height - THEME_METRICS.rasterWarningSizeMin));
  ctx.fillStyle = THEME_COLORS.rasterWarningOverlay;
  ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
  ctx.fillStyle = SEQUENCE_COLORS.warning;
  ctx.beginPath();
  ctx.moveTo(rect.x + rect.width - size - THEME_METRICS.rasterWarningInset, rect.y + THEME_METRICS.rasterWarningInset);
  ctx.lineTo(rect.x + rect.width - THEME_METRICS.rasterWarningInset, rect.y + THEME_METRICS.rasterWarningInset);
  ctx.lineTo(rect.x + rect.width - THEME_METRICS.rasterWarningInset, rect.y + size + THEME_METRICS.rasterWarningInset);
  ctx.closePath();
  ctx.fill();
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

function isTextEntryElement(target: EventTarget | null) {
  return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement;
}

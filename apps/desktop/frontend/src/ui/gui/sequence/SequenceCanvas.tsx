import * as ContextMenu from "@radix-ui/react-context-menu";
import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent } from "react";

import { ArrowRight, ChevronRight, Trash2 } from "lucide-react";

import { commands } from "../../../api";
import { scheduleViewStateSave } from "../../../viewStatePersistence";

import type { AppSettings, GuiDocumentRequest, FixtureTarget, PersistedSequenceViewportState, SequenceAutomationClip, SequenceAutomationTarget, SequenceEditorDocument, SequenceEffectScope, SequenceEffectDefinition } from "../../../types";

import { runGuiEditCommand, runSnapshotCommand, useAppStore } from "../../../store";

import { clamp, formatSeconds, roundToNanosecond, type AudioTransportViewSnapshot, type AutomationClipChooser, type GuiFocus, type SequenceSelection } from "../shared";

import { defaultMarkColor, drawSequenceMarks, committedMarkDrafts, markIndexAfterMove, nextCollectionKey, useMarkDisplayMode } from "./marks";

import { graphOperatorDefinition } from "./graphOperator";
import { targetsEqual } from "./sequenceTargets";
import { drawWaveformStrip, useSequenceWaveform } from "./sequenceWaveform";
import { drawClipRaster, useSequenceClipRasters } from "./sequenceClipRasters";
import { useSequenceTransport } from "./SequenceTransportControls";
import {
  automationClipsWithDrafts,
  automationCurvePointFromCanvas,
  automationHoverEqual,
  automationLaneRowHeight,
  rowHeightAt,
  automationRowCounts,
  buildAutomationClipLayout,
  drawAutomationClip,
  expandedTimelineHeight,
  fitCanvasLabel,
  hitTimelineClip,
  hitAutomationCurvePoint,
  laneIndexFromCanvasY,
  rowFromCanvasY,
  sequenceRowLayout,
  type SequenceRowLayout,
  removeAutomationCurvePoint,
  replaceAutomationCurvePointByIdentity,
  sortAutomationCurve,
  type AutomationCurveDraft,
  type AutomationDraft,
  type AutomationHover
} from "./sequenceAutomationLayout";
import { THEME_COLORS, THEME_METRICS, THEME_TYPOGRAPHY } from "../../../theme";

import { buildSequenceClipLayout, constrainEffectLaneDelta, constrainEffectMoveDelta, constrainEffectResizeDelta, constrainMarkDelta, effectMoveDrafts, effectResizeDrafts, hitSequence, hitSequenceMark, markMoveDrafts, markRefLookup, mergeSequenceSelection, MIN_EFFECT_DURATION_SECONDS, nextEffectSelection, nextMarkSelection, normalizedRect, selectedEffectId, selectionCount, selectionFromMarqueeEffects, selectionFromMarqueeMarks, sequenceHoverEqual, setMarkDraft, singleEffectSelectionFocus, singleSelectionFocus, selectionFromSingle, type MarkDraftLookup, type SequenceContextMenu, type SequenceHover, type SequenceMarquee, type SequenceDraft, type SequenceViewport } from "./sequenceSelection";

const SEQUENCE_CANVAS = {
  leftGutterPx: THEME_METRICS.sequenceLeftGutter,
  topPx: THEME_METRICS.sequenceTop,
  audioStripTopPx: THEME_METRICS.sequenceAudioStripTop,
  initialPxPerSecond: THEME_METRICS.sequenceInitialPixelsPerSecond,
  initialLaneHeightPx: THEME_METRICS.sequenceInitialLaneHeight,
  minPxPerSecond: THEME_METRICS.sequenceMinPixelsPerSecond,
  maxPxPerSecond: THEME_METRICS.sequenceMaxPixelsPerSecond,
  maxZoomPxPerSecond: THEME_METRICS.sequenceMaxZoomPixelsPerSecond,
  minLaneHeightPx: THEME_METRICS.sequenceMinLaneHeight,
  maxLaneHeightPx: THEME_METRICS.sequenceMaxLaneHeight,
  wheelZoomScale: THEME_METRICS.sequenceWheelZoomScale,
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
  automationFill: THEME_COLORS.automationFill,
  automationGraph: THEME_COLORS.automationGraph,
  automationGraphGrid: THEME_COLORS.automationGraphGrid,
  automationGraphGridMajor: THEME_COLORS.automationGraphMajorGrid,
  accent: THEME_COLORS.accent,
  accentSubtle: THEME_COLORS.accentSubtle,
  warning: THEME_COLORS.warning,
  markMarquee: THEME_COLORS.markMarquee,
  markMarqueeFill: THEME_COLORS.markMarqueeFill,
  effectMarqueeFill: THEME_COLORS.effectMarqueeFill,
  playhead: THEME_COLORS.playhead
} as const;

const SEQUENCE_DRAG_THRESHOLD_PX = THEME_METRICS.sequenceDragThreshold;

type SequenceDragState =
  | null
  | { kind: "rowResize"; laneIndex: number; rowIndex: number; startY: number; initialHeight: number; active: boolean }
  | { kind: "sequence"; id: number; startX: number; startY: number; active: boolean; originalStartSeconds: number; laneIndex: number; resize: "none" | "left" | "right" }
  | { kind: "automation"; id: number; startX: number; startY: number; active: boolean; originalStartSeconds: number; anchorLaneIndex: number; laneIndex: number; resize: "none" | "left" | "right" }
  | { kind: "automationPoint"; clipId: number; pointTime: number; pointValue: number; pointOccurrence: number; active: boolean }
  | { kind: "mark"; collectionKey: string; index: number; startX: number; startY: number; active: boolean; originalTimeSeconds: number }
  | { kind: "marquee"; state: SequenceMarquee }
  | { kind: "sequenceScrub" };

function automationCurvePointIdentity(
  curve: Array<{ time: number; value: number }>,
  index: number
) {
  const point = curve[index];
  if (point === undefined) throw new Error("Automation curve point is missing");
  const pointOccurrence = curve
    .slice(0, index)
    .filter((candidate) => candidate.time === point.time && candidate.value === point.value)
    .length;
  return { pointTime: point.time, pointValue: point.value, pointOccurrence };
}

function automationCurvePointIndex(
  curve: Array<{ time: number; value: number }>,
  identity: { pointTime: number; pointValue: number; pointOccurrence: number }
) {
  let occurrence = 0;
  const sorted = sortAutomationCurve(curve);
  for (let index = 0; index < sorted.length; index += 1) {
    const point = sorted[index];
    if (point?.time !== identity.pointTime || point.value !== identity.pointValue) continue;
    if (occurrence === identity.pointOccurrence) return index;
    occurrence += 1;
  }
  return null;
}

function rowResizeHit(
  y: number,
  top: number,
  scrollY: number,
  rows: SequenceRowLayout[]
): { laneIndex: number; rowIndex: number; edgeY: number } | null {
  const contentY = y - top + scrollY;
  for (const row of rows) {
    if (Math.abs(contentY - row.bottom) <= THEME_METRICS.sequenceLaneResizeHitHeight) {
      return { laneIndex: row.laneIndex, rowIndex: row.rowIndex, edgeY: row.bottom };
    }
  }
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
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<SequenceDragState>(null);
  const sequenceSelectionRef = useRef<SequenceSelection>(sequenceSelection);
  const [draft, setDraft] = useState<SequenceDraft | null>(null);
  const [automationDraft, setAutomationDraft] = useState<AutomationDraft | null>(null);
  const [automationCurveDraft, setAutomationCurveDraft] = useState<AutomationCurveDraft | null>(null);
  const [groupDraft, setGroupDraft] = useState<SequenceDraft[]>([]);
  const [markDrafts, setMarkDrafts] = useState<MarkDraftLookup>(() => new Map());
  const [sequenceContextMenu, setSequenceContextMenu] = useState<SequenceContextMenu | null>(null);
  const [hover, setHover] = useState<SequenceHover>(null);
  const [rowResizeHover, setRowResizeHover] = useState<{ laneIndex: number; rowIndex: number } | null>(null);
  const [dragCursor, setDragCursor] = useState<"grabbing" | null>(null);
  const [selectedLaneIndex, setSelectedLaneIndex] = useState<number | null>(null);
  const [selectedTimeSeconds, setSelectedTimeSeconds] = useState<number | null>(null);
  const [marquee, setMarquee] = useState<SequenceMarquee | null>(null);
  const [canvasSize, setCanvasSize] = useState({ width: 0, height: 0 });
  const restoreState = useAppStore((store) => store.restoreState);
  const gestureRequest = useRef<GuiDocumentRequest | null>(null);
  const settings = useAppStore((store) => store.snapshot?.settings ?? null);
  const restoreKey = `${document.path}::${document.objectKey}`;
  const restoredViewport = restoreState?.sequenceViewports[restoreKey];
  const [viewport, setViewport] = useState<SequenceViewport>(() => sequenceViewportFromPersisted(restoredViewport, document, settings));
  const viewportInitialized = useRef(false);
  const restoredViewportKey = useRef<string | null>(restoredViewport === undefined ? null : restoreKey);
  const left = SEQUENCE_CANVAS.leftGutterPx;
  const top = SEQUENCE_CANVAS.topPx;
  const audioStripTop = SEQUENCE_CANVAS.audioStripTopPx;
  const audioStripHeight = top - audioStripTop;
  const waveform = useSequenceWaveform(document.audio);
  const [mode] = useMarkDisplayMode();
  const automationRowHeight = automationLaneRowHeight(initialSequenceLaneHeight(settings));
  const automationClipsForLayout = useMemo(
    () => automationClipsWithDrafts(document.automationClips, automationDraft, automationCurveDraft),
    [automationCurveDraft, automationDraft, document.automationClips]
  );
  const automationRowsByLane = useMemo(
    () => automationRowCounts(automationClipsForLayout, document.lanes.length),
    [automationClipsForLayout, document.lanes.length]
  );
  const rows = useMemo(
    () => sequenceRowLayout(automationRowsByLane, viewport.rowHeights, initialSequenceLaneHeight(settings), automationRowHeight),
    [automationRowsByLane, viewport.rowHeights, settings, automationRowHeight]
  );
  const visibleMarkCollections = useMemo(
    () => document.markCollections.filter((collection) => visibleMarkCollectionKeys.has(collection.key)),
    [document.markCollections, visibleMarkCollectionKeys]
  );
  const [automationHover, setAutomationHover] = useState<AutomationHover | null>(null);
  const canvasCursor =
    dragCursor ??
    (rowResizeHover !== null ? "ns-resize" :
    (automationClipChooser !== null && automationHover !== null
      ? "pointer"
      : automationHover !== null
      ? automationHover.resize === "none" ? "grab" : "ew-resize"
      : hover === null ? undefined : hover.kind === "mark" ? "pointer" : hover.resize === "none" ? "grab" : "ew-resize"));

  const updateSequenceSelection = useCallback((selection: SequenceSelection) => {
    sequenceSelectionRef.current = selection;
    setSequenceSelection(selection);
  }, [setSequenceSelection]);

  useEffect(() => {
    sequenceSelectionRef.current = sequenceSelection;
  }, [sequenceSelection]);

  useEffect(() => {
    const target = canvas.current;
    if (!target) return;
    const updateSize = () => {
      const rect = target.getBoundingClientRect();
      setCanvasSize({ width: rect.width, height: rect.height });
      const timelineWidth = Math.max(1, rect.width - left);
      if (rect.width > 0 && !viewportInitialized.current) {
        viewportInitialized.current = true;
        if (restoredViewport === undefined) {
          setViewport({
            pxPerSecond: initialSequencePxPerSecond(settings, timelineWidth, document.durationSeconds),
            rowHeights: automationRowsByLane.map((rowCount) => [initialSequenceLaneHeight(settings), ...Array.from({ length: rowCount }, () => automationRowHeight)]),
            scrollXSeconds: 0,
            scrollY: 0
          });
        }
      }
      setViewport((current) => {
        const minPxPerSecond = minSequencePxPerSecond(timelineWidth, document.durationSeconds);
        const pxPerSecond = Math.max(current.pxPerSecond, minPxPerSecond);
        const scrollXSeconds = clamp(current.scrollXSeconds, 0, Math.max(0, document.durationSeconds - timelineWidth / pxPerSecond));
        const rowHeights = automationRowsByLane.map((rowCount, laneIndex) => {
          const rows = [...(current.rowHeights[laneIndex] ?? [])].slice(0, rowCount + 1);
          while (rows.length <= rowCount) rows.push(rows.length === 0 ? initialSequenceLaneHeight(settings) : automationRowHeight);
          return rows;
        });
        const rowsChanged = rowHeights.some((rows, laneIndex) => {
          const currentRows = current.rowHeights[laneIndex] ?? [];
          return rows.length !== currentRows.length || rows.some((height, rowIndex) => height !== currentRows[rowIndex]);
        });
        const maxScrollY = Math.max(0, expandedTimelineHeight(sequenceRowLayout(automationRowsByLane, rowHeights, initialSequenceLaneHeight(settings), automationRowHeight)) - Math.max(1, rect.height - top));
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
  }, [automationRowHeight, automationRowsByLane, document, left, restoredViewport, settings, top]);

  useEffect(() => {
    if (restoredViewport === undefined || restoredViewportKey.current === restoreKey) return;
    restoredViewportKey.current = restoreKey;
    setViewport(sequenceViewportFromPersisted(restoredViewport, document, settings));
  }, [document, restoreKey, restoredViewport, settings]);

  useEffect(() => {
    const state: PersistedSequenceViewportState = {
      pxPerSecond: viewport.pxPerSecond,
      rowHeights: Object.fromEntries(document.lanes.flatMap((lane, laneIndex) => Array.from({ length: (automationRowsByLane[laneIndex] ?? 0) + 1 }, (_, rowIndex) => [rowKey(lane.target, rowIndex), rowHeightAt(viewport.rowHeights, laneIndex, rowIndex, rowIndex === 0 ? initialSequenceLaneHeight(settings) : automationRowHeight)]))),
      scrollXSeconds: viewport.scrollXSeconds,
      scrollY: viewport.scrollY,
      activeMarkCollectionKey,
      visibleMarkCollectionKeys: [...visibleMarkCollectionKeys]
    };
    scheduleSequenceViewportStateSave(document.path, document.objectKey, state);
  }, [activeMarkCollectionKey, automationRowHeight, automationRowsByLane, document, settings, viewport, visibleMarkCollectionKeys]);

  const visibleClips = useMemo(
    () => buildSequenceClipLayout(
      document,
      groupDraft.length > 0 ? groupDraft : draft === null ? [] : [draft],
      viewport,
      left,
      top,
      canvasSize,
      rows
    ),
    [canvasSize, document, groupDraft, left, draft, top, viewport, rows]
  );
  const visibleRasterClips = useMemo(() => {
    return visibleClips
      .filter((clip) => clip.rect.x + clip.rect.width >= left && clip.rect.x <= canvasSize.width && clip.rect.y + clip.rect.height >= top && clip.rect.y <= canvasSize.height);
  }, [canvasSize.height, canvasSize.width, left, top, visibleClips]);
  const visibleAutomationClips = useMemo(
    () => buildAutomationClipLayout(automationClipsForLayout, rows, viewport, left, top, canvasSize),
    [automationClipsForLayout, rows, canvasSize, left, top, viewport]
  );
  const clipRasters = useSequenceClipRasters(document, visibleRasterClips, Math.max(...viewport.rowHeights.map((rows) => rows[0] ?? 0), SEQUENCE_CANVAS.minLaneHeightPx), settings);
  const selectedEffectIds = useMemo(() => new Set<number>(sequenceSelection?.type === "effects" ? sequenceSelection.ids : []), [sequenceSelection]);
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
    const maxScrollY = Math.max(0, totalLaneHeight - Math.max(1, rect.height - top));
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
      const y = top + row.top - scrollY;
      if (y > rect.height || y + row.height < top) return;
      ctx.fillStyle = index % 2 === 0 ? SEQUENCE_COLORS.page : SEQUENCE_COLORS.laneAlt;
      ctx.fillRect(left, y, timelineWidth, row.height);
      if (row.rowIndex === 0 && selectedLaneIndex === row.laneIndex) {
        ctx.fillStyle = SEQUENCE_COLORS.laneSelected;
        ctx.fillRect(left, y, timelineWidth, row.height);
      }
      ctx.strokeStyle = SEQUENCE_COLORS.grid;
      ctx.beginPath();
      ctx.moveTo(left, y + row.height + THEME_METRICS.visualHairlineOffset);
      ctx.lineTo(rect.width, y + row.height + THEME_METRICS.visualHairlineOffset);
      ctx.stroke();
      ctx.fillStyle = SEQUENCE_COLORS.panel;
      ctx.fillRect(0, y, left, row.height);
      ctx.fillStyle = row.rowIndex === 0 ? SEQUENCE_COLORS.textMuted : SEQUENCE_COLORS.automation;
      const lane = document.lanes[row.laneIndex];
      if (lane === undefined) throw new Error("Timeline row has no lane.");
      const label = row.rowIndex === 0 ? lane.label : `Automation ${row.rowIndex}`;
      ctx.fillText(fitCanvasLabel(ctx, label, left - THEME_METRICS.sequenceLabelX * 2), THEME_METRICS.sequenceLabelX, y + row.height / 2 + THEME_METRICS.sequenceLabelYOffset);
      if (rowResizeHover?.laneIndex === row.laneIndex && rowResizeHover.rowIndex === row.rowIndex) {
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
    ctx.fillStyle = SEQUENCE_COLORS.page;
    ctx.fillRect(left, audioStripTop, timelineWidth, audioStripHeight);
    ctx.strokeStyle = SEQUENCE_COLORS.gridFaint;
    ctx.beginPath();
    ctx.moveTo(0, top + THEME_METRICS.visualHairlineOffset);
    ctx.lineTo(rect.width, top + THEME_METRICS.visualHairlineOffset);
    ctx.stroke();

    drawWaveformStrip(
      ctx,
      waveform.audio,
      left,
      audioStripTop,
      timelineWidth,
      audioStripHeight,
      document.durationSeconds,
      viewport.pxPerSecond,
      scrollXSeconds,
      SEQUENCE_COLORS
    );
    drawTimelineGrid(ctx, left, top, rect.width, rect.height, viewport.pxPerSecond, scrollXSeconds, document.frameRate);
    drawSequenceMarks(
      ctx,
      visibleMarkCollections,
      selected,
      selectedMarks,
      mode,
      left,
      audioStripTop,
      audioStripHeight,
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
    for (const clip of visibleAutomationClips) {
      const selectedClip = selected?.type === "automationClip" && selected.id === clip.clip.id;
      const hoverResize = automationHover?.clipId === clip.clip.id ? automationHover.resize : null;
      const choosingCandidate = automationClipChooser !== null;
      const activePointIndex = drag.current?.kind === "automationPoint" && drag.current.clipId === clip.clip.id
        ? automationCurvePointIndex(clip.clip.curve, drag.current)
        : null;
      drawAutomationClip(ctx, clip.clip, clip.rect, {
        label: automationClipLabel(document, clip.clip),
        selected: selectedClip,
        hovered: hoverResize !== null,
        choosing: choosingCandidate,
        resize: hoverResize ?? "none",
        activePointIndex
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

    ctx.strokeStyle = SEQUENCE_COLORS.playhead;
    ctx.lineWidth = THEME_METRICS.visualLineWidth;
    ctx.beginPath();
    ctx.moveTo(left + THEME_METRICS.visualHairlineOffset, top);
    ctx.lineTo(left, rect.height);
    ctx.stroke();
  }, [activeAutomationTargetEffectIds, automationClipChooser, automationHover, rows, document, rowResizeHover, left, top, audioStripTop, audioStripHeight, settings, viewport, visibleClips, visibleAutomationClips, selected, selectedEffectIds, selectedMarks, selectedLaneIndex, selectedTimeSeconds, marquee, waveform.audio, visibleMarkCollections, mode, markDrafts, hover, clipRasters]);

  const seekFromCanvas = (event: MouseEvent<HTMLCanvasElement>) => {
    const x = event.nativeEvent.offsetX;
    if (x < left) return;
    const positionSeconds = clamp(Math.round((viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond) / SEQUENCE_CANVAS.scrubStepSeconds) * SEQUENCE_CANVAS.scrubStepSeconds, 0, document.durationSeconds);
    void runSnapshotCommand(() => commands.audioSeek(positionSeconds));
  };
  const timeFromCanvasX = (x: number) => clamp(roundToNanosecond(viewport.scrollXSeconds + (x - left) / viewport.pxPerSecond), 0, document.durationSeconds);
  const addEffectFromContextMenu = async (definition: SequenceEffectDefinition, menu: SequenceContextMenu) => {
    const hasMarksParams = definition.params.some((param) => param.kind === "marks");
    let markCollectionKey = hasMarksParams ? activeMarkCollectionKey ?? document.markCollections[0]?.key ?? null : null;
    if (hasMarksParams && markCollectionKey === null) {
      const newCollectionKey = nextCollectionKey("Marks", document.markCollections);
      await runGuiEditCommand((request) =>
        commands.applySequenceGuiEdit(request, {
          type: "createMarkCollection",
          key: newCollectionKey,
          name: "Marks",
          color: defaultMarkColor(document.markCollections.length)
        })
      );
      markCollectionKey = newCollectionKey;
      setActiveMarkCollectionKey(newCollectionKey);
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, newCollectionKey]));
    }
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
  const addMarkFromContextMenu = async (collectionKey: string | null, menu: SequenceContextMenu) => {
    let targetCollectionKey = collectionKey;
    if (targetCollectionKey === null) {
      const newCollectionKey = nextCollectionKey("Marks", document.markCollections);
      await runGuiEditCommand((request) =>
        commands.applySequenceGuiEdit(request, {
          type: "createMarkCollection",
          key: newCollectionKey,
          name: "Marks",
          color: defaultMarkColor(document.markCollections.length)
        })
      );
      targetCollectionKey = newCollectionKey;
      setActiveMarkCollectionKey(targetCollectionKey);
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, targetCollectionKey]));
    }
    await runGuiEditCommand((request) =>
      commands.applySequenceGuiEdit(request, {
        type: "addMark",
        collectionKey: targetCollectionKey,
        timeSeconds: menu.startSeconds
      })
    );
  };
  const addMarkAtTime = async (timeSeconds: number) => {
    let collectionKey = activeMarkCollectionKey ?? document.markCollections[0]?.key ?? null;
    if (collectionKey === null) {
      const newCollectionKey = nextCollectionKey("Marks", document.markCollections);
      await runGuiEditCommand((request) =>
        commands.applySequenceGuiEdit(request, {
          type: "createMarkCollection",
          key: newCollectionKey,
          name: "Marks",
          color: defaultMarkColor(document.markCollections.length)
        })
      );
      collectionKey = newCollectionKey;
      setActiveMarkCollectionKey(newCollectionKey);
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, newCollectionKey]));
    }
    await runGuiEditCommand((request) =>
      commands.applySequenceGuiEdit(request, {
        type: "addMark",
        collectionKey,
        timeSeconds
      })
    );
    const nextIndex = [...(document.markCollections.find((collection) => collection.key === collectionKey)?.marksSeconds ?? []), timeSeconds]
      .map((markTimeSeconds, index) => ({ markTimeSeconds, index }))
      .sort((leftMark, rightMark) => leftMark.markTimeSeconds - rightMark.markTimeSeconds || leftMark.index - rightMark.index)
      .findIndex((mark) => mark.index === (document.markCollections.find((collection) => collection.key === collectionKey)?.marksSeconds.length ?? 0));
    updateSequenceSelection({ type: "marks", marks: [{ collectionKey, index: Math.max(0, nextIndex) }] });
    setSelected({ type: "mark", collectionKey, index: Math.max(0, nextIndex) });
  };
  const addAutomationClipFromContextMenu = async (menu: SequenceContextMenu) => {
    await runGuiEditCommand((request) =>
      commands.applySequenceGuiEdit(request, {
        type: "addAutomationClip",
        startSeconds: menu.startSeconds,
        durationSeconds: Math.min(2, Math.max(0.000000001, document.durationSeconds - menu.startSeconds)),
        anchorLaneIndex: menu.laneIndex,
        laneIndex: 0
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
        target: chooser.target,
        mapping: chooser.mapping
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
            style={canvasCursor === undefined ? undefined : { cursor: canvasCursor }}
            tabIndex={0}
      onKeyDown={(event) => {
        if (event.key === "Escape" && automationClipChooser !== null) {
          event.preventDefault();
          setAutomationClipChooser(null);
          return;
        }
        const selectedMark = selected?.type === "mark" ? { collectionKey: selected.collectionKey, index: selected.index } : null;
        const focusedEffectId = selectedEffectId(selected);
        const activeSelection = sequenceSelection ?? selectionFromSingle(selected);
        if ((event.ctrlKey || event.metaKey) && !isTextEntryElement(event.target)) {
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
            void runGuiEditCommand((request) => commands.applySequenceSelectionEdit(request, {
              type: "paste",
              anchor: { laneIndex: selectedLaneIndex as never, timeSeconds: selectedTimeSeconds as never }
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
        if (x < left || y < top || document.lanes.length === 0) {
          event.preventDefault();
          setSequenceContextMenu(null);
          return;
        }
        const laneIndex = laneIndexFromCanvasY(y, top, viewport.scrollY, document.lanes.length, rows);
        const startSeconds = timeFromCanvasX(x);
        const automationHit = hitTimelineClip(visibleAutomationClips, x, y);
        if (automationHit !== null) {
          if (event.ctrlKey) {
            event.preventDefault();
            const pointHit = hitAutomationCurvePoint(automationHit, x, y);
            if (pointHit !== null && automationHit.clip.curve.length > 1) {
              const curve = removeAutomationCurvePoint(automationHit.clip.curve, pointHit);
              setAutomationCurveDraft({ id: automationHit.clip.id, curve });
              void runGuiEditCommand((request) =>
                commands.applySequenceGuiEdit(request, {
                  type: "updateAutomationCurve",
                  id: automationHit.clip.id,
                  curve
                })
              ).finally(() => {
                setAutomationCurveDraft(null);
              });
            }
            return;
          }
          setSelected({ type: "automationClip", id: automationHit.clip.id });
          updateSequenceSelection(null);
          setSequenceContextMenu({
            kind: "automation",
            laneIndex: automationHit.clip.anchorLaneIndex,
            startSeconds,
            clipId: automationHit.clip.id
          });
          return;
        }
        const hit = hitSequence(visibleClips, x, y);
        if (hit !== null) {
          setSelected({ type: "effect", id: hit.effect.id });
          updateSequenceSelection({ type: "effects", ids: [hit.effect.id] });
          setSequenceContextMenu({ kind: "effect", laneIndex: hit.laneIndex, startSeconds, effectId: hit.effect.id });
          return;
        }
        const markHit = hitSequenceMark(visibleMarkCollections, mode, x, y, left, audioStripTop, audioStripHeight, canvasSize.height, viewport);
        if (markHit !== null) {
          setSelected({ type: "mark", collectionKey: markHit.collectionKey, index: markHit.index });
          updateSequenceSelection({ type: "marks", marks: [{ collectionKey: markHit.collectionKey, index: markHit.index }] });
          setActiveMarkCollectionKey(markHit.collectionKey);
          setSequenceContextMenu({ kind: "mark", laneIndex, startSeconds, collectionKey: markHit.collectionKey, index: markHit.index });
          return;
        }
        setSelected(null);
        updateSequenceSelection(null);
        setSequenceContextMenu({ kind: "blank", laneIndex, startSeconds });
      }}
      onMouseDown={(event) => {
        gestureRequest.current = useAppStore.getState().guiRequest;
        if (event.button !== 0) return;
        event.currentTarget.focus();
        const x = event.nativeEvent.offsetX;
        const y = event.nativeEvent.offsetY;
        setMarkDrafts(new Map());
        if (automationClipChooser !== null) {
          const automationHit = x >= left ? hitTimelineClip(visibleAutomationClips, x, y) : null;
          if (automationHit !== null) {
            event.preventDefault();
            event.stopPropagation();
            chooseAutomationClip(automationHit.clip.id);
          }
          return;
        }
        if (x >= left && y < top) {
          drag.current = { kind: "sequenceScrub" };
          seekFromCanvas(event);
          return;
        }
        if (x < left && y >= top && document.lanes.length > 0) {
          const resizeHit = rowResizeHit(y, top, viewport.scrollY, rows);
          if (resizeHit !== null) {
            event.preventDefault();
            drag.current = {
              kind: "rowResize",
              laneIndex: resizeHit.laneIndex,
              rowIndex: resizeHit.rowIndex,
              startY: y,
              initialHeight: rowHeightAt(viewport.rowHeights, resizeHit.laneIndex, resizeHit.rowIndex, resizeHit.rowIndex === 0 ? initialSequenceLaneHeight(settings) : automationRowHeight),
              active: false
            };
            setRowResizeHover({ laneIndex: resizeHit.laneIndex, rowIndex: resizeHit.rowIndex });
            return;
          }
          const laneIndex = laneIndexFromCanvasY(y, top, viewport.scrollY, document.lanes.length, rows);
          const lane = document.lanes[laneIndex];
          if (lane === undefined) return;
          const ids = document.effects.filter((effect) => targetsEqual(effect.target, lane.target)).map((effect) => effect.id);
          setSelectedLaneIndex(laneIndex);
          updateSequenceSelection(ids.length > 0 ? { type: "effects", ids } : null);
          setSelected(singleEffectSelectionFocus(ids));
          return;
        }
        const automationHit = x >= left ? hitTimelineClip(visibleAutomationClips, x, y) : null;
        if (automationHit !== null) {
          if (event.ctrlKey) {
            event.preventDefault();
            event.stopPropagation();
            const point = automationCurvePointFromCanvas(automationHit.rect, x, y);
            const curve = [...automationHit.clip.curve, point]
              .filter((candidate) => Number.isFinite(candidate.time) && Number.isFinite(candidate.value))
              .sort((leftPoint, rightPoint) => leftPoint.time - rightPoint.time);
            setSelected({ type: "automationClip", id: automationHit.clip.id });
            updateSequenceSelection(null);
            setAutomationCurveDraft({ id: automationHit.clip.id, curve });
            void runGuiEditCommand((request) =>
              commands.applySequenceGuiEdit(request, {
                type: "updateAutomationCurve",
                id: automationHit.clip.id,
                curve
              })
            ).finally(() => {
              setAutomationCurveDraft(null);
            });
            return;
          }
          const pointHit = hitAutomationCurvePoint(automationHit, x, y);
          if (pointHit !== null) {
            setSelected({ type: "automationClip", id: automationHit.clip.id });
            updateSequenceSelection(null);
            drag.current = {
              kind: "automationPoint",
              clipId: automationHit.clip.id,
              ...automationCurvePointIdentity(sortAutomationCurve(automationHit.clip.curve), pointHit),
              active: false
            };
            return;
          }
          setSelected({ type: "automationClip", id: automationHit.clip.id });
          updateSequenceSelection(null);
          setSelectedLaneIndex(automationHit.clip.anchorLaneIndex);
          drag.current = {
            kind: "automation",
            id: automationHit.clip.id,
            startX: x,
            startY: y,
            active: false,
            originalStartSeconds: automationHit.clip.startSeconds,
            anchorLaneIndex: automationHit.clip.anchorLaneIndex,
            laneIndex: automationHit.clip.laneIndex,
            resize: automationHit.resize
          };
          return;
        }
        const hit = x >= left ? hitSequence(visibleClips, x, y) : null;
        if (hit !== null) {
          const activeSelection = sequenceSelectionRef.current;
          const wasAlreadySelected = activeSelection?.type === "effects" && activeSelection.ids.includes(hit.effect.id);
          const nextSelection = wasAlreadySelected && !event.shiftKey && !event.ctrlKey && !event.metaKey
            ? activeSelection
            : nextEffectSelection(activeSelection?.type === "effects" ? activeSelection : null, hit.effect.id, event.shiftKey, event.ctrlKey || event.metaKey);
          updateSequenceSelection(nextSelection);
          setSelected(nextSelection.type === "effects" ? singleEffectSelectionFocus(nextSelection.ids) ?? { type: "effect", id: hit.effect.id } : { type: "effect", id: hit.effect.id });
          setSelectedLaneIndex(hit.laneIndex);
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
        const markHit = hitSequenceMark(visibleMarkCollections, mode, x, y, left, audioStripTop, audioStripHeight, canvasSize.height, viewport);
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
        if (x >= left && y >= top) {
        const laneIndex = laneIndexFromCanvasY(y, top, viewport.scrollY, document.lanes.length, rows);
          const timeSeconds = timeFromCanvasX(x);
          setSelectedLaneIndex(laneIndex);
          setSelectedTimeSeconds(timeSeconds);
          setSelected(null);
          updateSequenceSelection(null);
          const state = { mode: event.altKey ? "marks" as const : "effects" as const, startX: x, startY: y, x, y, active: false, shift: event.shiftKey, ctrl: event.ctrlKey || event.metaKey };
          drag.current = { kind: "marquee", state };
          setMarquee(state);
        }
      }}
      onMouseMove={(event) => {
        const current = drag.current;
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
            const rowHeights = previous.rowHeights.map((rows, laneIndex) => {
              if (laneIndex !== current.laneIndex) return rows;
              const nextRows = [...rows];
              while (nextRows.length <= current.rowIndex) {
                nextRows.push(nextRows.length === 0 ? initialSequenceLaneHeight(settings) : automationRowHeight);
              }
              nextRows[current.rowIndex] = nextHeight;
              return nextRows;
            });
            const maxScrollY = Math.max(0, expandedTimelineHeight(sequenceRowLayout(automationRowsByLane, rowHeights, initialSequenceLaneHeight(settings), automationRowHeight)) - Math.max(1, canvasSize.height - top));
            return { ...previous, rowHeights, scrollY: clamp(previous.scrollY, 0, maxScrollY) };
          });
          return;
        }
        if (current?.kind === "sequenceScrub") {
          seekFromCanvas(event);
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
            const selectedByBox = next.mode === "effects"
              ? selectionFromMarqueeEffects(visibleClips, next)
              : selectionFromMarqueeMarks(visibleMarkCollections, mode, next, left, audioStripTop, audioStripHeight, canvasSize.height, viewport);
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
          setDraft(null);
          setAutomationDraft(null);
          setGroupDraft([]);
          return;
        }
        if (current?.kind === "automationPoint") {
          const layout = visibleAutomationClips.find((candidate) => candidate.clip.id === current.clipId);
          if (layout === undefined) return;
          const point = automationCurvePointFromCanvas(layout.rect, event.nativeEvent.offsetX, event.nativeEvent.offsetY);
          const curve = replaceAutomationCurvePointByIdentity(sortAutomationCurve(layout.clip.curve), current, point);
          setAutomationCurveDraft({ id: current.clipId, curve });
          const pointIndex = curve.indexOf(point);
          if (pointIndex >= 0) Object.assign(current, automationCurvePointIdentity(curve, pointIndex));
          current.active = true;
          return;
        }
        if (!current) {
          const x = event.nativeEvent.offsetX;
          const y = event.nativeEvent.offsetY;
          const resizeHit = x < left && y >= top
            ? rowResizeHit(y, top, viewport.scrollY, rows)
            : null;
          setRowResizeHover(resizeHit === null ? null : { laneIndex: resizeHit.laneIndex, rowIndex: resizeHit.rowIndex });
          const automationHit = x >= left ? hitTimelineClip(visibleAutomationClips, x, y) : null;
          const choosingAutomation = automationClipChooser !== null;
          const hit = x >= left && automationHit === null && !choosingAutomation ? hitSequence(visibleClips, x, y) : null;
          const markHit =
            hit === null && automationHit === null && !choosingAutomation
              ? hitSequenceMark(visibleMarkCollections, mode, x, y, left, audioStripTop, audioStripHeight, canvasSize.height, viewport)
              : null;
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
        }
        if (current.kind === "automation") {
          const clip = document.automationClips.find((candidate) => candidate.id === current.id);
          if (clip === undefined) return;
          const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
          if (current.resize === "left") {
            const endSeconds = clip.startSeconds + clip.durationSeconds;
            const startSeconds = clamp(current.originalStartSeconds + deltaSeconds, 0, endSeconds - MIN_EFFECT_DURATION_SECONDS);
            setAutomationDraft({ id: clip.id, startSeconds, durationSeconds: endSeconds - startSeconds, anchorLaneIndex: current.anchorLaneIndex, laneIndex: current.laneIndex });
          } else if (current.resize === "right") {
            setAutomationDraft({
              id: clip.id,
              startSeconds: clip.startSeconds,
              durationSeconds: clamp(clip.durationSeconds + deltaSeconds, MIN_EFFECT_DURATION_SECONDS, document.durationSeconds - clip.startSeconds),
              anchorLaneIndex: current.anchorLaneIndex,
              laneIndex: current.laneIndex
            });
          } else {
            const targetRow = rowFromCanvasY(event.nativeEvent.offsetY, top, viewport.scrollY, rows);
            const laneIndex = targetRow?.laneIndex ?? laneIndexFromCanvasY(event.nativeEvent.offsetY, top, viewport.scrollY, document.lanes.length, rows);
            const automationLaneIndex = targetRow === null || targetRow.rowIndex <= 0 ? 0 : targetRow.rowIndex - 1;
            setAutomationDraft({
              id: clip.id,
              startSeconds: clamp(current.originalStartSeconds + deltaSeconds, 0, Math.max(0, document.durationSeconds - clip.durationSeconds)),
              durationSeconds: clip.durationSeconds,
              anchorLaneIndex: laneIndex,
              laneIndex: automationLaneIndex
            });
          }
          setDraft(null);
          setGroupDraft([]);
          return;
        }
        const effect = document.effects.find((candidate) => candidate.id === current.id);
        if (effect === undefined) return;
        const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
        const laneIndex =
          current.resize === "none"
            ? laneIndexFromCanvasY(event.nativeEvent.offsetY, top, viewport.scrollY, document.lanes.length, rows)
            : current.laneIndex;
        const activeEffectSelection = sequenceSelectionRef.current;
        if (activeEffectSelection?.type === "effects" && activeEffectSelection.ids.includes(current.id) && activeEffectSelection.ids.length > 1) {
          if (current.resize === "none") {
            const constrainedDelta = constrainEffectMoveDelta(document, activeEffectSelection.ids, deltaSeconds);
            const laneDelta = constrainEffectLaneDelta(document, activeEffectSelection.ids, laneIndex - current.laneIndex);
            setGroupDraft(effectMoveDrafts(document, activeEffectSelection.ids, constrainedDelta, laneDelta));
          } else {
            const constrainedDelta = constrainEffectResizeDelta(document, activeEffectSelection.ids, current.resize, deltaSeconds);
            setGroupDraft(effectResizeDrafts(document, activeEffectSelection.ids, current.resize, constrainedDelta));
          }
          setDraft(null);
          return;
        }
        setGroupDraft([]);
        if (current.resize === "left") {
          const startSeconds = clamp(current.originalStartSeconds + deltaSeconds, 0, effect.startSeconds + effect.durationSeconds - MIN_EFFECT_DURATION_SECONDS);
          setDraft({ id: effect.id, startSeconds, durationSeconds: effect.startSeconds + effect.durationSeconds - startSeconds, laneIndex });
        } else if (current.resize === "right") {
          setDraft({ id: effect.id, startSeconds: effect.startSeconds, durationSeconds: clamp(effect.durationSeconds + deltaSeconds, MIN_EFFECT_DURATION_SECONDS, document.durationSeconds - effect.startSeconds), laneIndex });
        } else {
          setDraft({ id: effect.id, startSeconds: clamp(current.originalStartSeconds + deltaSeconds, 0, Math.max(0, document.durationSeconds - effect.durationSeconds)), durationSeconds: effect.durationSeconds, laneIndex });
        }
      }}
      onMouseUp={(event) => {
        const current = drag.current;
        drag.current = null;
        setDragCursor(null);
        setMarquee(null);
        if (current?.kind === "rowResize") {
          setRowResizeHover(null);
          return;
        }
        if (current?.kind === "marquee") {
          if (!current.state.active && current.state.mode === "marks") {
            void addMarkAtTime(timeFromCanvasX(current.state.startX));
          }
          return;
        }
        if (current?.kind === "mark") {
          if (!current.active) {
            setMarkDrafts(new Map());
            setDraft(null);
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
        if (current?.kind === "automation") {
          const committedDraft = automationDraft;
          if (!current.active || committedDraft === null) {
            setAutomationDraft(null);
            return;
          }
          const originalClip = document.automationClips.find((candidate) => candidate.id === committedDraft.id);
          const isNoOp =
            originalClip !== undefined &&
            committedDraft.startSeconds === originalClip.startSeconds &&
            committedDraft.durationSeconds === originalClip.durationSeconds &&
            committedDraft.anchorLaneIndex === originalClip.anchorLaneIndex &&
            committedDraft.laneIndex === originalClip.laneIndex;
          if (isNoOp) {
            setAutomationDraft(null);
            return;
          }
            void runGuiEditCommand((request) =>
              current.resize === "none"
                ? commands.applySequenceGuiEdit(request, {
                    type: "moveAutomationClip",
                    id: committedDraft.id,
                    startSeconds: committedDraft.startSeconds,
                    anchorLaneIndex: committedDraft.anchorLaneIndex,
                    laneIndex: committedDraft.laneIndex
                  })
                : commands.applySequenceGuiEdit(request, {
                    type: "resizeAutomationClip",
                    id: committedDraft.id,
                    startSeconds: committedDraft.startSeconds,
                    durationSeconds: committedDraft.durationSeconds
                  })
            , gestureRequest.current).finally(() => {
              setAutomationDraft(null);
            });
          return;
        }
        if (current?.kind === "automationPoint") {
          const committedDraft = automationCurveDraft;
          if (!current.active || committedDraft === null) {
            setAutomationCurveDraft(null);
            return;
          }
          void runGuiEditCommand((request) =>
            commands.applySequenceGuiEdit(request, {
              type: "updateAutomationCurve",
              id: committedDraft.id,
              curve: committedDraft.curve
            }), gestureRequest.current
          ).finally(() => {
            setAutomationCurveDraft(null);
          });
          return;
        }
        if (!current || current.kind !== "sequence") return;
        if (!current.active) {
          setDraft(null);
          setAutomationDraft(null);
          setGroupDraft([]);
          return;
        }
        const activeSelection = sequenceSelectionRef.current;
        if (activeSelection?.type === "effects" && activeSelection.ids.length > 1 && activeSelection.ids.includes(current.id)) {
          const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
          const rawLaneIndex = laneIndexFromCanvasY(event.nativeEvent.offsetY, top, viewport.scrollY, document.lanes.length, rows);
          const laneDelta = current.resize === "none" ? constrainEffectLaneDelta(document, activeSelection.ids, rawLaneIndex - current.laneIndex) : 0;
          const edit = current.resize === "none"
            ? { type: "moveEffects" as const, ids: activeSelection.ids, timeDeltaSeconds: constrainEffectMoveDelta(document, activeSelection.ids, deltaSeconds), laneDelta }
            : { type: "resizeEffects" as const, ids: activeSelection.ids, edge: current.resize, timeDeltaSeconds: constrainEffectResizeDelta(document, activeSelection.ids, current.resize, deltaSeconds) };
          if ((edit.type === "moveEffects" && edit.timeDeltaSeconds === 0 && edit.laneDelta === 0) || (edit.type === "resizeEffects" && edit.timeDeltaSeconds === 0)) {
            setDraft(null);
            setGroupDraft([]);
            return;
          }
          void runGuiEditCommand((request) => commands.applySequenceSelectionEdit(request, edit), gestureRequest.current).then((result) => {
            updateSequenceSelection(result.selection);
            setSelected(null);
            setDraft(null);
            setGroupDraft([]);
            return result;
          });
          return;
        }
        const effect = document.effects.find((candidate) => candidate.id === current.id);
        if (effect === undefined) {
          setDraft(null);
          setGroupDraft([]);
          return;
        }
        const deltaSeconds = roundToNanosecond((event.nativeEvent.offsetX - current.startX) / viewport.pxPerSecond);
        const laneIndex =
          current.resize === "none"
            ? laneIndexFromCanvasY(event.nativeEvent.offsetY, top, viewport.scrollY, document.lanes.length, rows)
            : current.laneIndex;
        const committedDraft =
          current.resize === "left"
            ? {
                id: effect.id,
                startSeconds: clamp(current.originalStartSeconds + deltaSeconds, 0, effect.startSeconds + effect.durationSeconds - MIN_EFFECT_DURATION_SECONDS),
                durationSeconds: effect.startSeconds + effect.durationSeconds - clamp(current.originalStartSeconds + deltaSeconds, 0, effect.startSeconds + effect.durationSeconds - MIN_EFFECT_DURATION_SECONDS),
                laneIndex
              }
            : current.resize === "right"
              ? {
                  id: effect.id,
                  startSeconds: effect.startSeconds,
                  durationSeconds: clamp(effect.durationSeconds + deltaSeconds, MIN_EFFECT_DURATION_SECONDS, document.durationSeconds - effect.startSeconds),
                  laneIndex
                }
              : {
                  id: effect.id,
                  startSeconds: clamp(current.originalStartSeconds + deltaSeconds, 0, Math.max(0, document.durationSeconds - effect.durationSeconds)),
                  durationSeconds: effect.durationSeconds,
                  laneIndex
                };
        const target = document.lanes[committedDraft.laneIndex]?.target ?? null;
        const isNoOp =
          current.resize === "none"
            ? committedDraft.startSeconds === effect.startSeconds && (target === null || targetsEqual(effect.target, target))
            : committedDraft.startSeconds === effect.startSeconds && committedDraft.durationSeconds === effect.durationSeconds;
        if (isNoOp) {
          setDraft(null);
          setGroupDraft([]);
          return;
        }
        const edit = (request: GuiDocumentRequest) =>
          current.resize === "none"
            ? commands.applySequenceGuiEdit(request, {
                type: "moveEffect",
                id: committedDraft.id,
                startSeconds: committedDraft.startSeconds,
                target
              })
            : commands.applySequenceGuiEdit(request, {
                type: "resizeEffect",
                id: committedDraft.id,
                startSeconds: committedDraft.startSeconds,
                durationSeconds: committedDraft.durationSeconds
              });
        void runGuiEditCommand(edit, gestureRequest.current).finally(() => {
          setDraft(null);
          setGroupDraft([]);
        });
      }}
      onMouseLeave={() => {
        if (drag.current === null) {
          setHover(null);
          setAutomationHover(null);
          setRowResizeHover(null);
        }
      }}
      onWheel={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        const offsetX = event.clientX - rect.left;
        const timelineWidth = Math.max(1, rect.width - left);
        const visibleHeight = Math.max(1, rect.height - top);

        event.preventDefault();
        setViewport((current) => {
          const maxScrollXSeconds = Math.max(0, document.durationSeconds - timelineWidth / current.pxPerSecond);
          const maxScrollY = Math.max(0, expandedTimelineHeight(sequenceRowLayout(automationRowsByLane, current.rowHeights, initialSequenceLaneHeight(settings), automationRowHeight)) - visibleHeight);
          if (event.ctrlKey && event.shiftKey) {
            const scale = Math.exp(-event.deltaY * SEQUENCE_CANVAS.wheelZoomScale);
            const rowHeights = current.rowHeights.map((rows) => rows.map((height) => clamp(height * scale, SEQUENCE_CANVAS.minLaneHeightPx, SEQUENCE_CANVAS.maxLaneHeightPx)));
            return {
              ...current,
              rowHeights,
              scrollY: clamp(current.scrollY, 0, Math.max(0, expandedTimelineHeight(sequenceRowLayout(automationRowsByLane, rowHeights, initialSequenceLaneHeight(settings), automationRowHeight)) - visibleHeight))
            };
          }
          if (event.ctrlKey) {
            const anchorX = clamp(offsetX - left, 0, timelineWidth);
            const anchorTime = current.scrollXSeconds + anchorX / current.pxPerSecond;
            const nextPxPerSecond = clamp(
              current.pxPerSecond * Math.exp(-event.deltaY * SEQUENCE_CANVAS.wheelZoomScale),
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
            return {
              ...current,
              scrollXSeconds: clamp(current.scrollXSeconds + event.deltaY / current.pxPerSecond, 0, maxScrollXSeconds)
            };
          }
          return {
            ...current,
            scrollY: clamp(current.scrollY + event.deltaY, 0, maxScrollY)
          };
        });
      }}
          />
        </ContextMenu.Trigger>
        {sequenceContextMenu !== null && (
          <ContextMenu.Portal>
            <ContextMenu.Content className="menu-content">
              <ContextMenu.Sub>
                <ContextMenu.SubTrigger className="menu-item">
                  Add Effect <ChevronRight size={THEME_METRICS.iconSizeSmall} aria-hidden />
                </ContextMenu.SubTrigger>
                <ContextMenu.Portal>
                  <ContextMenu.SubContent className="menu-content">
                    {document.effectDefinitions.length === 0 ? (
                      <ContextMenu.Item className="menu-item" disabled>
                        No effects
                      </ContextMenu.Item>
                    ) : (
                      document.effectDefinitions.map((definition) => (
                        <ContextMenu.Item
                          key={definition.effect.type === "builtin" ? `builtin:${definition.effect.effect}` : `${definition.effect.path}:${definition.effect.effectName}`}
                          className="menu-item"
                          onSelect={() => void addEffectFromContextMenu(definition, sequenceContextMenu)}
                        >
                          {definition.name}
                        </ContextMenu.Item>
                      ))
                    )}
                  </ContextMenu.SubContent>
                </ContextMenu.Portal>
              </ContextMenu.Sub>
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
                <ContextMenu.Portal>
                  <ContextMenu.SubContent className="menu-content">
                    {document.markCollections.length === 0 ? (
                      <ContextMenu.Item className="menu-item" onSelect={() => void addMarkFromContextMenu(null, sequenceContextMenu)}>
                        Marks
                      </ContextMenu.Item>
                    ) : (
                      markCollectionsForMenu().map((collection) => (
                        <ContextMenu.Item
                          key={collection.key}
                          className="menu-item"
                          onSelect={() => void addMarkFromContextMenu(collection.key, sequenceContextMenu)}
                        >
                          <span style={{ color: collection.color }}>{collection.name}</span>
                        </ContextMenu.Item>
                      ))
                    )}
                  </ContextMenu.SubContent>
                </ContextMenu.Portal>
              </ContextMenu.Sub>
              <ContextMenu.Item className="menu-item" onSelect={() => void addAutomationClipFromContextMenu(sequenceContextMenu)}>
                Add Automation Clip
              </ContextMenu.Item>
              {sequenceContextMenu.kind === "effect" && (
                <>
                  <ContextMenu.Separator className="menu-separator" />
                  <ContextMenu.Sub>
                    <ContextMenu.SubTrigger className="menu-item">
                      Retarget Effect <span className="shortcut"><ArrowRight size={THEME_METRICS.iconSizeExtraSmall} aria-hidden="true" /></span>
                    </ContextMenu.SubTrigger>
                    <ContextMenu.Portal>
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
      <SequenceTransportOverlay
        document={document}
        viewport={viewport}
        left={left}
        top={top}
        canvasSize={canvasSize}
      />
    </div>
  );
}

function SequenceTransportOverlay({
  document,
  viewport,
  left,
  top,
  canvasSize
}: {
  document: SequenceEditorDocument;
  viewport: SequenceViewport;
  left: number;
  top: number;
  canvasSize: { width: number; height: number };
}) {
  const transport = useAppStore((store) => store.snapshot?.audioTransport ?? null);
  if (transport === null) return null;
  return (
    <SequenceTransportMarkers
      document={document}
      transport={transport}
      viewport={viewport}
      left={left}
      top={top}
      canvasSize={canvasSize}
    />
  );
}

function SequenceTransportMarkers({
  document,
  transport,
  viewport,
  left,
  top,
  canvasSize
}: {
  document: SequenceEditorDocument;
  transport: AudioTransportViewSnapshot;
  viewport: SequenceViewport;
  left: number;
  top: number;
  canvasSize: { width: number; height: number };
}) {
  const liveTransport = useSequenceTransport(transport);
  const markerHeight = Math.max(0, canvasSize.height - top);
  const markerLeft = (seconds: number) =>
    left + (clamp(seconds, 0, document.durationSeconds) - viewport.scrollXSeconds) * viewport.pxPerSecond;
  const playheadLeft = markerLeft(liveTransport.positionSeconds);
  const homeLeft = markerLeft(liveTransport.homeSeconds);
  const visible = (x: number) => x >= left && x <= canvasSize.width;

  return (
    <>
      {visible(homeLeft) && (
        <div
          className="sequence-transport-marker home"
          style={{ left: homeLeft, top, height: markerHeight }}
        />
      )}
      {visible(playheadLeft) && (
        <div
          className="sequence-transport-marker playhead"
          style={{ left: playheadLeft, top, height: markerHeight }}
        />
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

function scheduleSequenceViewportStateSave(path: string, objectKey: string, state: PersistedSequenceViewportState) {
  scheduleViewStateSave(JSON.stringify(["sequence", path, objectKey]), () => commands.saveSequenceViewportState({ path, objectKey, state }),
    (error) => { useAppStore.getState().setError(String(error)); });
}

function rowKey(target: FixtureTarget, rowIndex: number): string {
  return `${target.fixture}:row:${rowIndex}`;
}

function sequenceViewportFromPersisted(state: PersistedSequenceViewportState | undefined, document: SequenceEditorDocument, settings: AppSettings | null): SequenceViewport {
  const defaultHeight = initialSequenceLaneHeight(settings);
  if (state === undefined) {
    return {
      pxPerSecond: settings?.sequenceInitialPxPerSecond ?? SEQUENCE_CANVAS.initialPxPerSecond,
      rowHeights: document.lanes.map((_lane, laneIndex) => {
        const rowCount = document.automationClips.filter((clip) => clip.anchorLaneIndex === laneIndex).reduce((count, clip) => Math.max(count, clip.laneIndex + 1), 0);
        return [defaultHeight, ...Array.from({ length: rowCount }, () => automationLaneRowHeight(defaultHeight))];
      }),
      scrollXSeconds: 0,
      scrollY: 0
    };
  }
  return {
    pxPerSecond: clamp(state.pxPerSecond, SEQUENCE_CANVAS.minPxPerSecond, SEQUENCE_CANVAS.maxZoomPxPerSecond),
    rowHeights: document.lanes.map((lane, laneIndex) => {
      const rowCount = document.automationClips.filter((clip) => clip.anchorLaneIndex === laneIndex).reduce((count, clip) => Math.max(count, clip.laneIndex + 1), 0);
      return Array.from({ length: rowCount + 1 }, (_, rowIndex) => clamp(state.rowHeights[rowKey(lane.target, rowIndex)] ?? (rowIndex === 0 ? defaultHeight : automationLaneRowHeight(defaultHeight)), SEQUENCE_CANVAS.minLaneHeightPx, SEQUENCE_CANVAS.maxLaneHeightPx));
    }),
    scrollXSeconds: Math.max(0, state.scrollXSeconds),
    scrollY: Math.max(0, state.scrollY)
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

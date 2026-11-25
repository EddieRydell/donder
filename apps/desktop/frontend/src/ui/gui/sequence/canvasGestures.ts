import type { PlaybackRange, SequenceGuiDocument, SequenceGuiEdit, SequenceLane, SequenceMarkCollection, SequenceSelection } from "../../../types";
import { THEME_METRICS } from "../../../theme";
import { roundToNanosecond } from "../shared";
import { hitAutomationCurvePoint, hitAutomationPoint } from "./sequenceAutomationLayout";
import { laneIndexFromCanvasY, rowFromCanvasY, type SequenceRowKind, type SequenceRowLayout } from "./sequenceRows";
import { clipSelectionGesture, constrainMarkDelta, hitSequence, hitSequenceMark, type SequenceClipLayout, type SequenceDraft, type SequenceHit, type SequenceMarkHit, type SequenceMarquee, type SequenceViewport } from "./sequenceSelection";

/** The resizable strips above the lanes, each with its viewport height and limits. */
export type TimelineStrip = "audio" | "marks";
export const TIMELINE_STRIPS = {
  audio: { height: "audioStripHeight", minPx: THEME_METRICS.sequenceMinAudioStripHeight, maxPx: THEME_METRICS.sequenceMaxAudioStripHeight },
  marks: { height: "markRulerHeight", minPx: THEME_METRICS.sequenceMinMarkRulerHeight, maxPx: THEME_METRICS.sequenceMaxMarkRulerHeight }
} as const satisfies Record<TimelineStrip, { height: keyof SequenceViewport; minPx: number; maxPx: number }>;

/** What a pointer drag on the canvas is doing. A drag with `active` does nothing until it passes the threshold. */
export type GestureState =
  | { kind: "stripResize"; strip: TimelineStrip; startY: number; initialHeight: number; active: boolean }
  | { kind: "gutterResize"; startX: number; initialWidth: number; active: boolean }
  | { kind: "rowResize"; laneIndex: number; rowKind: SequenceRowKind; startY: number; initialHeight: number; active: boolean }
  | { kind: "sequence"; id: number; startX: number; startY: number; active: boolean; originalStartSeconds: number; laneIndex: number; resize: "none" | "left" | "right" }
  | { kind: "automationPoint"; clipId: number; laneIndex: number; index: number; active: boolean; inserted: boolean }
  | { kind: "automationErase"; clipId: number; laneIndex: number }
  | { kind: "mark"; collectionId: number; index: number; startX: number; startY: number; active: boolean; originalTimeSeconds: number }
  | { kind: "marquee"; state: SequenceMarquee }
  | { kind: "playheadScrub" }
  | { kind: "slice"; seconds: number; startY: number }
  // Dragging a range edge anchors the new range at the opposite edge.
  | { kind: "timeRange"; startX: number; anchorSeconds: number; fromEdge: boolean; active: boolean };

/** What the canvas draws where: enough to say what a point is over. */
export type CanvasLayout = {
  left: number;
  /** The top of the lanes, below the waveform strip and the mark ruler. */
  top: number;
  audioStripTop: number;
  audioStripBottom: number;
  viewport: SequenceViewport;
  rows: SequenceRowLayout[];
  lanes: SequenceLane[];
  clips: SequenceClipLayout[];
  envelopes: SequenceClipLayout[];
  marks: SequenceMarkCollection[];
  activeMarks: string | null;
  transport: { positionSeconds: number; homeSeconds: number; range: PlaybackRange | null } | null;
};

/** What a point of the canvas is over, the first that applies from the top. */
export type CanvasHit =
  | { type: "stripResize"; strip: TimelineStrip }
  | { type: "gutterResize" }
  /** The ruler and waveform strip: the playhead, a range edge, the home marker or a time. */
  | { type: "seek"; target: "playhead" | "rangeEdge" | "home" | "time" }
  | { type: "mark"; mark: SequenceMarkHit }
  | { type: "markRuler" }
  | { type: "rowResize"; laneIndex: number; rowKind: SequenceRowKind }
  /** A shared lane's "×N" badge, which visits the target's next lane. */
  | { type: "sharedBadge"; laneIndex: number }
  | { type: "disclosure"; laneIndex: number }
  | { type: "rowLabel"; laneIndex: number; row: SequenceRowLayout | null }
  | { type: "curvePoint"; layout: SequenceClipLayout; index: number }
  /** An envelope's body below its header, where a click adds a point. */
  | { type: "curve"; hit: SequenceHit }
  | { type: "clip"; hit: SequenceHit }
  | { type: "lane"; laneIndex: number }
  | { type: "none" };

/** How a gesture reads the canvas: whether it edits curves, and whether only envelopes answer, as while choosing one to bind. */
export type HitMode = { curves: boolean; envelopesOnly: boolean };

/**
 * Each strip resizes from its bottom edge. The audio strip's grip lies inside the strip, so it
 * never covers the Marks lane below; the Marks lane's grip straddles its border with the lanes.
 */
function stripResizeHit(y: number, audioStripBottom: number, markRulerBottom: number): TimelineStrip | null {
  const grip = THEME_METRICS.sequenceAudioResizeHitHeight;
  if (Math.abs(y - markRulerBottom) <= grip / 2) return "marks";
  if (y <= audioStripBottom && audioStripBottom - y <= grip) return "audio";
  return null;
}

function rowResizeHit(y: number, top: number, scrollY: number, rows: SequenceRowLayout[]): SequenceRowLayout | null {
  const contentY = y - top + scrollY;
  return rows.find((row) => row.height > 0 && Math.abs(contentY - row.bottom) <= THEME_METRICS.sequenceLaneResizeHitHeight) ?? null;
}

/** Left edge of a shared lane's "×N" badge in the label column. */
export function sharedBadgeX(left: number) {
  return left - THEME_METRICS.sequenceLabelX - THEME_METRICS.sequenceSharedBadgeWidth;
}

/** A lane label's disclosure triangle and text positions, indented by depth like a file tree. */
export function laneLabelLayout(lane: SequenceLane) {
  const disclosureX = THEME_METRICS.sequenceLabelX + lane.depth * THEME_METRICS.sequenceLaneIndent;
  return { disclosureX, textX: disclosureX + THEME_METRICS.sequenceDisclosureSize + THEME_METRICS.sequenceDisclosureGap };
}

/** The playhead's head is in the ruler, so range edges under it stay grabbable in the waveform strip. */
function seekTarget(layout: CanvasLayout, x: number, y: number): Extract<CanvasHit, { type: "seek" }>["target"] {
  const { transport, viewport, left } = layout;
  const near = (seconds: number) => Math.abs(left + (seconds - viewport.scrollXSeconds) * viewport.pxPerSecond - x) <= THEME_METRICS.sequenceTransportHitHalfWidth;
  if (transport === null) return "time";
  if (y < layout.audioStripTop && near(transport.positionSeconds)) return "playhead";
  if (transport.range !== null && (near(transport.range.startSeconds) || near(transport.range.endSeconds))) return "rangeEdge";
  return near(transport.homeSeconds) ? "home" : "time";
}

/** What `point` is over. */
export function hit(layout: CanvasLayout, point: { x: number; y: number }, mode: HitMode): CanvasHit {
  const { x, y } = point;
  const { left, top, viewport, rows } = layout;
  const strip = stripResizeHit(y, layout.audioStripBottom, top);
  if (strip !== null) return { type: "stripResize", strip };
  if (Math.abs(x - left) <= THEME_METRICS.sequenceLeftGutterResizeHitWidth / 2) return { type: "gutterResize" };
  if (y < layout.audioStripBottom) return x >= left ? { type: "seek", target: seekTarget(layout, x, y) } : { type: "none" };
  if (y < top) {
    if (x < left) return { type: "none" };
    const mark = mode.envelopesOnly ? null : hitSequenceMark(layout.marks, layout.activeMarks, x, left, viewport);
    return mark === null ? { type: "markRuler" } : { type: "mark", mark };
  }
  if (layout.lanes.length === 0) return { type: "none" };
  if (x < left) {
    const resize = rowResizeHit(y, top, viewport.scrollY, rows);
    if (resize !== null) return { type: "rowResize", laneIndex: resize.laneIndex, rowKind: resize.kind };
    const laneIndex = laneIndexFromCanvasY(y, top, viewport.scrollY, layout.lanes.length, rows);
    const lane = layout.lanes[laneIndex];
    if (lane === undefined) return { type: "none" };
    const row = rowFromCanvasY(y, top, viewport.scrollY, rows);
    if (row?.kind === "clips" && lane.occurrences > 1 && x >= sharedBadgeX(left)) return { type: "sharedBadge", laneIndex };
    if (row?.kind === "clips" && lane.kind === "group" && x < laneLabelLayout(lane).textX) return { type: "disclosure", laneIndex };
    return { type: "rowLabel", laneIndex, row };
  }
  // A point wins over the envelope edge it sits on.
  const curvePoint = mode.curves && !mode.envelopesOnly ? hitAutomationPoint(layout.envelopes, x, y) : null;
  if (curvePoint !== null) return { type: "curvePoint", layout: curvePoint.layout, index: curvePoint.index };
  const clip = hitSequence(mode.envelopesOnly ? layout.envelopes : layout.clips, x, y);
  if (clip !== null) {
    if (mode.curves && clip.clip.content.type === "envelope") {
      const index = hitAutomationCurvePoint(clip, x, y);
      if (index !== null) return { type: "curvePoint", layout: clip, index };
      if (clip.resize === "none" && y > clip.rect.y + Math.min(THEME_METRICS.envelopeHeaderHeight, clip.rect.height)) return { type: "curve", hit: clip };
    }
    return { type: "clip", hit: clip };
  }
  return { type: "lane", laneIndex: laneIndexFromCanvasY(y, top, viewport.scrollY, layout.lanes.length, rows) };
}

/** A pointer moving with a drag held, or releasing it. */
export type GestureEvent = { type: "move" | "up"; x: number; y: number };

/** What a drag reads of the sequence to say what it drafts and commits. */
export type GestureContext = {
  document: SequenceGuiDocument;
  selection: SequenceSelection | null;
  pxPerSecond: number;
  /** The lane under a canvas y. */
  laneAt: (y: number) => number;
  /** A clip drag's time delta at `x`, snapped as the gesture asks. */
  clipDelta: (state: Extract<GestureState, { kind: "sequence" }>, x: number, destinationLane: number) => number;
  /** How a resized envelope treats its curve. */
  envelopeResize: "crop" | "stretch";
  /** The clips the slice tool's line cuts. */
  sliced: () => number[];
};

export type GestureResult = {
  /** The drag after the event; none once released. */
  state: GestureState | null;
  /** Whether this event carried the drag past its threshold. */
  started: boolean;
  /** The clips' timing and lanes while a clip drag is drafted, and as it commits. */
  clips?: SequenceDraft[];
  /** What a released drag commits. */
  edit?: SequenceGuiEdit;
};

const THRESHOLD = THEME_METRICS.sequenceDragThreshold;

/** Whether `event` carries a drag that waits for the threshold past it. */
function passes(state: GestureState, event: GestureEvent): boolean {
  switch (state.kind) {
    case "stripResize":
    case "rowResize": return Math.abs(event.y - state.startY) >= THRESHOLD;
    case "gutterResize":
    case "timeRange": return Math.abs(event.x - state.startX) >= THRESHOLD;
    case "sequence":
    case "mark": return Math.hypot(event.x - state.startX, event.y - state.startY) >= THRESHOLD;
    // A curve point moves with the first move.
    case "automationPoint": return event.type === "move";
    case "automationErase":
    case "marquee":
    case "playheadScrub":
    case "slice": return false;
  }
}

/** The drag state machine: a drag activates past its threshold; released, a clip, mark or slice drag commits its edit. */
export function reduce(state: GestureState, event: GestureEvent, context: GestureContext): GestureResult {
  const started = "active" in state && !state.active && passes(state, event);
  const next: GestureState = started ? { ...state, active: true } : state;
  const clipGesture = (current: Extract<GestureState, { kind: "sequence" }>) => {
    const { selection } = context;
    if (selection?.type !== "clips") return null;
    const destinationLane = context.laneAt(event.y);
    return clipSelectionGesture(context.document, selection, current.resize, context.clipDelta(current, event.x, destinationLane), current.laneIndex, destinationLane - current.laneIndex, context.envelopeResize);
  };
  if (event.type === "move") {
    const gesture = next.kind === "sequence" && next.active ? clipGesture(next) : null;
    return gesture === null ? { state: next, started } : { state: next, started, clips: gesture.clips };
  }
  switch (next.kind) {
    case "sequence": {
      const gesture = next.active ? clipGesture(next) : null;
      return gesture === null || !gesture.changed ? { state: null, started } : { state: null, started, clips: gesture.clips, edit: gesture.edit };
    }
    case "mark": {
      if (!next.active) return { state: null, started };
      const { selection } = context;
      // A selected mark moves the whole mark selection; another mark moves alone.
      const marks = selection?.type === "marks" && selection.marks.some((mark) => mark.collectionId === next.collectionId && mark.index === next.index)
        ? selection.marks
        : [{ collectionId: next.collectionId, index: next.index }];
      const timeDeltaSeconds = constrainMarkDelta(context.document, marks, roundToNanosecond((event.x - next.startX) / context.pxPerSecond));
      return timeDeltaSeconds === 0 ? { state: null, started } : { state: null, started, edit: { type: "moveMarks", marks, timeDeltaSeconds } };
    }
    case "slice": {
      const ids = context.sliced();
      return ids.length === 0 ? { state: null, started } : { state: null, started, edit: { type: "splitClips", ids, timeSeconds: next.seconds } };
    }
    case "stripResize":
    case "gutterResize":
    case "rowResize":
    case "automationPoint":
    case "automationErase":
    case "marquee":
    case "playheadScrub":
    case "timeRange": return { state: null, started };
  }
}

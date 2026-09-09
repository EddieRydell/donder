import type { SequenceControlClip } from "../../../types";
import { THEME_COLORS, THEME_METRICS, THEME_TYPOGRAPHY } from "../../../theme";
import { clamp, roundToNanosecond } from "../shared";
import { assignOverlapSlots, groupOverlappingClips, type SequenceRowLayout } from "./sequenceAutomationLayout";
import { MIN_EFFECT_DURATION_SECONDS, type SequenceViewport } from "./sequenceSelection";
import { sameControlChannel } from "./sequenceTargets";
import { controlValueLabel } from "./ControlValueInput";

export type ControlClipLayout = { clip: SequenceControlClip; rect: { x: number; y: number; width: number; height: number } };
export type ControlDrag = { kind: "control"; clip: SequenceControlClip; startX: number; active: boolean; resize: "none" | "left" | "right" };

export function controlTiming(drag: ControlDrag, x: number, viewport: SequenceViewport, duration: number): SequenceControlClip {
  const delta = roundToNanosecond((x - drag.startX) / viewport.pxPerSecond);
  const clip = drag.clip;
  const end = clip.startSeconds + clip.durationSeconds;
  const startSeconds = drag.resize === "right" ? clip.startSeconds : clamp(clip.startSeconds + delta, 0, drag.resize === "left" ? end - MIN_EFFECT_DURATION_SECONDS : duration - clip.durationSeconds);
  const durationSeconds = drag.resize === "left" ? end - startSeconds : drag.resize === "right" ? clamp(clip.durationSeconds + delta, MIN_EFFECT_DURATION_SECONDS, duration - startSeconds) : clip.durationSeconds;
  return { ...clip, startSeconds: roundToNanosecond(startSeconds), durationSeconds: roundToNanosecond(durationSeconds) };
}

export function buildControlClipLayout(clips: SequenceControlClip[], rows: SequenceRowLayout[], viewport: SequenceViewport, left: number, top: number): ControlClipLayout[] {
  const layouts: ControlClipLayout[] = [];
  for (const row of rows) {
    const channel = row.controlChannel;
    if (channel === undefined) continue;
    for (const group of groupOverlappingClips(clips.filter((clip) => sameControlChannel(clip.target, channel.target)))) {
      const assigned = assignOverlapSlots(group);
      const count = Math.max(...assigned.map((clip) => clip.slot)) + 1;
      for (const clip of assigned) {
        const slotHeight = row.height / count;
        layouts.push({ clip, rect: {
          x: left + (clip.startSeconds - viewport.scrollXSeconds) * viewport.pxPerSecond,
          y: top + row.top - viewport.scrollY + clip.slot * slotHeight + THEME_METRICS.sequenceClipSlotOffset,
          width: Math.max(THEME_METRICS.sequenceClipMinWidth, clip.durationSeconds * viewport.pxPerSecond),
          height: Math.max(THEME_METRICS.sequenceClipMinHeight, slotHeight - THEME_METRICS.sequenceClipHandleInset)
        } });
      }
    }
  }
  return layouts;
}

export function drawControlClip(ctx: CanvasRenderingContext2D, layout: ControlClipLayout, selected: boolean) {
  const { clip, rect } = layout;
  ctx.save();
  ctx.beginPath();
  ctx.rect(rect.x, rect.y, rect.width, rect.height);
  ctx.clip();
  ctx.fillStyle = selected ? THEME_COLORS.automationClipHeaderSelected : THEME_COLORS.automationClipHeader;
  ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
  ctx.fillStyle = THEME_COLORS.automationClipLabel;
  ctx.font = THEME_TYPOGRAPHY.canvasLabel;
  ctx.textBaseline = "middle";
  const cells = clip.target.cells;
  const label = `${controlValueLabel(clip.value)}${cells === null ? "" : ` · cells ${cells.start + 1}–${cells.start + cells.count}`}`;
  ctx.fillText(label, rect.x + THEME_METRICS.automationClipLabelInset, rect.y + rect.height / 2);
  ctx.restore();
  ctx.strokeStyle = selected ? THEME_COLORS.clipSelected : THEME_COLORS.clipBorder;
  ctx.lineWidth = THEME_METRICS.visualLineWidth;
  ctx.strokeRect(rect.x, rect.y, rect.width, rect.height);
}

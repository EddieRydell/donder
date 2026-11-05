import { useCallback, useRef, type KeyboardEvent, type PointerEvent } from "react";
import { THEME_METRICS } from "../theme";
import { Tip } from "./kit";

const COLLAPSE_THRESHOLD_PX = THEME_METRICS.workspaceCollapseThreshold;
const KEYBOARD_STEP_PX = THEME_METRICS.workspaceKeyboardStep;

export type ResizeDirection = "left" | "right";

export function WorkspaceResizeHandle({
  ariaLabel,
  collapsed,
  direction,
  max,
  min,
  value,
  onChange
}: {
  ariaLabel: string;
  collapsed: boolean;
  direction: ResizeDirection;
  max: number;
  min: number;
  value: number;
  onChange: (update: { collapsed: boolean; width: number }) => void;
}) {
  const dragRef = useRef<{
    pointerId: number;
    startX: number;
    startWidth: number;
    moved: boolean;
  } | null>(null);

  const resizeBy = useCallback(
    (delta: number) => {
      const signedDelta = direction === "left" ? delta : -delta;
      onChange({ collapsed: false, width: clamp(value + signedDelta, min, max) });
    },
    [direction, max, min, onChange, value]
  );

  const toggleCollapsed = useCallback(() => {
    onChange({ collapsed: !collapsed, width: value });
  }, [collapsed, onChange, value]);

  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        toggleCollapsed();
        return;
      }
      if (event.key === "ArrowLeft") {
        event.preventDefault();
        resizeBy(-KEYBOARD_STEP_PX);
        return;
      }
      if (event.key === "ArrowRight") {
        event.preventDefault();
        resizeBy(KEYBOARD_STEP_PX);
      }
    },
    [resizeBy, toggleCollapsed]
  );

  const handlePointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      dragRef.current = {
        pointerId: event.pointerId,
        startX: event.clientX,
        startWidth: collapsed ? min : value,
        moved: false
      };
    },
    [collapsed, min, value]
  );

  const handlePointerMove = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      const drag = dragRef.current;
      if (drag === null || drag.pointerId !== event.pointerId) return;
      const pointerDelta = event.clientX - drag.startX;
      if (Math.abs(pointerDelta) > THEME_METRICS.interactionDragThreshold) drag.moved = true;
      const rawWidth = direction === "left" ? drag.startWidth + pointerDelta : drag.startWidth - pointerDelta;
      if (rawWidth < COLLAPSE_THRESHOLD_PX) {
        onChange({ collapsed: true, width: value });
        return;
      }
      onChange({ collapsed: false, width: clamp(rawWidth, min, max) });
    },
    [direction, max, min, onChange, value]
  );

  const endPointer = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      const drag = dragRef.current;
      if (drag === null || drag.pointerId !== event.pointerId) return;
      dragRef.current = null;
      event.currentTarget.releasePointerCapture(event.pointerId);
      if (collapsed && !drag.moved) {
        onChange({ collapsed: false, width: value });
      }
    },
    [collapsed, onChange, value]
  );

  return (
    <Tip label={ariaLabel}><div
      className={`workspace-resize-handle ${collapsed ? "collapsed" : ""}`}
      role="separator"
      aria-label={ariaLabel}
      aria-orientation="vertical"
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={Math.round(value)}
      tabIndex={0}
      onKeyDown={handleKeyDown}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={endPointer}
      onPointerCancel={endPointer}
    /></Tip>
  );
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

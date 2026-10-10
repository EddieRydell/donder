import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import { clamp } from "./shared";
import { THEME_METRICS } from "../../theme";

export function InspectorScrollArea({ children, footer }: { children: ReactNode; footer?: ReactNode }) {
  const contentRef = useRef<HTMLDivElement | null>(null);
  const railRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{ pointerId: number; startY: number; startScrollTop: number } | null>(null);
  const [metrics, setMetrics] = useState({ top: 0, height: 0, scrollable: false });

  const updateMetrics = useCallback(() => {
    const content = contentRef.current;
    if (content === null) return;
    const scrollable = content.scrollHeight > content.clientHeight + 1;
    const railHeight = Math.max(1, content.clientHeight);
    const height = scrollable ? Math.max(THEME_METRICS.scrollbarThumbMinHeight, (content.clientHeight / content.scrollHeight) * railHeight) : railHeight;
    const maxTop = Math.max(0, railHeight - height);
    const top = scrollable ? (content.scrollTop / Math.max(1, content.scrollHeight - content.clientHeight)) * maxTop : 0;
    setMetrics({ top, height, scrollable });
  }, []);

  useEffect(() => {
    const content = contentRef.current;
    if (content === null) return;
    updateMetrics();
    const resizeObserver = new ResizeObserver(updateMetrics);
    resizeObserver.observe(content);
    const mutationObserver = new MutationObserver(updateMetrics);
    mutationObserver.observe(content, { childList: true, subtree: true, characterData: true });
    content.addEventListener("scroll", updateMetrics, { passive: true });
    return () => {
      resizeObserver.disconnect();
      mutationObserver.disconnect();
      content.removeEventListener("scroll", updateMetrics);
    };
  }, [updateMetrics]);

  const scrollToPointer = useCallback((clientY: number) => {
    const content = contentRef.current;
    const rail = railRef.current;
    if (content === null || rail === null || !metrics.scrollable) return;
    const railRect = rail.getBoundingClientRect();
    const maxTop = Math.max(1, railRect.height - metrics.height);
    const top = clamp(clientY - railRect.top - metrics.height / 2, 0, maxTop);
    content.scrollTop = (top / maxTop) * Math.max(1, content.scrollHeight - content.clientHeight);
  }, [metrics.height, metrics.scrollable]);

  return (
    <aside className="gui-inspector-shell">
      <div ref={contentRef} className="gui-inspector">
        <div onKeyDownCapture={commitInspectorFieldOnEnter}>{children}</div>
      </div>
      <div className="editor-scrollbar" aria-hidden={!metrics.scrollable}>
        <div
          ref={railRef}
          className="editor-scrollbar-rail"
          onPointerDown={(event) => {
            if (!metrics.scrollable) return;
            event.currentTarget.setPointerCapture(event.pointerId);
            scrollToPointer(event.clientY);
          }}
        >
          <div
            className={`editor-scrollbar-thumb ${metrics.scrollable ? "" : "disabled"}`}
            style={{ top: `${metrics.top}px`, height: `${metrics.height}px` }}
            onPointerDown={(event) => {
              if (!metrics.scrollable) return;
              event.stopPropagation();
              event.currentTarget.setPointerCapture(event.pointerId);
              dragRef.current = {
                pointerId: event.pointerId,
                startY: event.clientY,
                startScrollTop: contentRef.current?.scrollTop ?? 0
              };
            }}
            onPointerMove={(event) => {
              const drag = dragRef.current;
              const content = contentRef.current;
              const rail = railRef.current;
              if (drag === null || content === null || rail === null || drag.pointerId !== event.pointerId) return;
              const maxTop = Math.max(1, rail.clientHeight - metrics.height);
              const scrollMax = Math.max(1, content.scrollHeight - content.clientHeight);
              content.scrollTop = drag.startScrollTop + ((event.clientY - drag.startY) / maxTop) * scrollMax;
            }}
            onPointerUp={(event) => {
              if (dragRef.current?.pointerId === event.pointerId) {
                dragRef.current = null;
              }
            }}
            onPointerCancel={(event) => {
              if (dragRef.current?.pointerId === event.pointerId) {
                dragRef.current = null;
              }
            }}
          />
        </div>
      </div>
      {footer !== undefined && <div className="gui-inspector-footer">{footer}</div>}
    </aside>
  );
}

export function Readout({ label, value, swatch }: { label: string; value: string | number; swatch?: string }) {
  return (
    <div className="inspector-readout">
      <span>{label}</span>
      <strong>
        {swatch !== undefined && <i style={{ background: swatch }} />}
        {value}
      </strong>
    </div>
  );
}

function commitInspectorFieldOnEnter(event: KeyboardEvent<HTMLDivElement>) {
  if (event.key !== "Enter") return;
  const target = event.target;
  if (!(target instanceof HTMLInputElement || target instanceof HTMLSelectElement || target instanceof HTMLTextAreaElement)) return;
  event.preventDefault();
  event.stopPropagation();
  target.blur();
}

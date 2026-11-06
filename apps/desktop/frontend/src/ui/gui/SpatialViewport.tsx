import { useCallback, useEffect, useRef, useState } from "react";
import { Scan, ZoomIn, ZoomOut } from "lucide-react";
import { clamp, fitViewport, type RenderBounds, type SpatialViewport } from "./shared";
import { IconButton } from "../kit";

const sessionViewports = new Map<string, SpatialViewport>();

export function useSpatialViewport(bounds: RenderBounds, resetKey?: string, sessionKey?: string) {
  const cached = sessionKey === undefined ? undefined : sessionViewports.get(sessionKey);
  const [view, setView] = useState<SpatialViewport>(() => cached ?? { scale: 1, fitScale: 1, offsetX: 0, offsetY: 0 });
  const size = useRef({ width: 1, height: 1 });
  const key = resetKey ?? "initial";
  const previousKey = useRef(cached === undefined ? "" : key);
  const reset = useCallback(() => { setView(fitViewport(bounds, size.current.width, size.current.height)); }, [bounds]);
  const resize = useCallback((width: number, height: number) => {
    size.current = { width, height };
    if (previousKey.current !== key) { previousKey.current = key; setView(fitViewport(bounds, width, height)); }
  }, [bounds, key]);
  useEffect(() => { if (sessionKey !== undefined) sessionViewports.set(sessionKey, view); }, [sessionKey, view]);
  const zoomAt = useCallback((factor: number, x: number, y: number) => { setView((v) => {
    const scale = clamp(v.scale * factor, v.fitScale * 0.05, v.fitScale * 64);
    const ratio = scale / v.scale;
    return { ...v, scale, offsetX: x - (x - v.offsetX) * ratio, offsetY: y - (y - v.offsetY) * ratio };
  }); }, []);
  const panBy = useCallback((x: number, y: number) => { setView((v) => ({ ...v, offsetX: v.offsetX + x, offsetY: v.offsetY + y })); }, []);
  return { view, resize, reset, zoomAt, panBy };
}

export function SpatialControls({ view, reset, zoomAt }: { view: SpatialViewport; reset: () => void; zoomAt: (factor: number, x: number, y: number) => void }) {
  return <div className="spatial-controls">
    <IconButton icon={ZoomOut} label="Zoom out" onClick={() => { zoomAt(0.8, 0, 0); }} />
    <span>{Math.round(view.scale / view.fitScale * 100)}%</span>
    <IconButton icon={ZoomIn} label="Zoom in" onClick={() => { zoomAt(1.25, 0, 0); }} />
    <IconButton icon={Scan} label="Fit" onClick={reset} />
  </div>;
}

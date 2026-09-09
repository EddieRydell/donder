import { THEME_COLORS, THEME_METRICS } from "../theme";
import { normalizeHexColor } from "../color";
import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { HexColorPicker } from "react-colorful";

type RgbColor = { r: number; g: number; b: number };
type HsvColor = { h: number; s: number; v: number };

export function ColorPicker({
  value,
  label,
  className = "",
  triggerClassName = "",
  openRequestKey = 0,
  stopTriggerPointerDownPropagation = false,
  onOpenChange,
  commit
}: {
  value: string;
  label: string;
  className?: string;
  triggerClassName?: string;
  openRequestKey?: number;
  stopTriggerPointerDownPropagation?: boolean;
  onOpenChange?: (open: boolean) => void;
  commit: (value: string) => Promise<void>;
}) {
  const normalizedValue = normalizeHexColor(value) ?? THEME_COLORS.white;
  const [internalOpen, setInternalOpen] = useState(false);
  const [dismissedOpenRequestKey, setDismissedOpenRequestKey] = useState(0);
  const [pickerDraft, setPickerDraft] = useState({ value: normalizedValue, requestKey: openRequestKey });
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const [popoverPosition, setPopoverPosition] = useState({ top: 0, left: 0 });
  const requestedOpen = openRequestKey !== 0 && dismissedOpenRequestKey !== openRequestKey;
  const open = internalOpen || requestedOpen;
  const draft = open ? (pickerDraft.requestKey === openRequestKey ? pickerDraft.value : normalizedValue) : normalizedValue;
  const rgb = useMemo(() => hexToRgb(draft), [draft]);
  const hsv = useMemo(() => rgbToHsv(rgb), [rgb]);

  useEffect(() => {
    if (!open) return;
    const updatePopoverPosition = () => {
      const rect = triggerRef.current?.getBoundingClientRect();
      if (rect === undefined) return;
      const width = THEME_METRICS.colorPickerWidth;
      const height = THEME_METRICS.colorPickerHeight;
      const gap = THEME_METRICS.popoverGap;
      const below = rect.bottom + gap;
      const above = rect.top - height - gap;
      setPopoverPosition({
        top: below + height > window.innerHeight - THEME_METRICS.popoverOffset ? Math.max(THEME_METRICS.popoverOffset, above) : below,
        left: clamp(rect.left, THEME_METRICS.popoverOffset, window.innerWidth - width - THEME_METRICS.popoverOffset)
      });
    };
    const closeOnPointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (rootRef.current?.contains(target) === true || popoverRef.current?.contains(target) === true) return;
      setInternalOpen(false);
      setDismissedOpenRequestKey(openRequestKey);
      onOpenChange?.(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setInternalOpen(false);
        setDismissedOpenRequestKey(openRequestKey);
        onOpenChange?.(false);
      }
    };
    updatePopoverPosition();
    window.addEventListener("mousedown", closeOnPointerDown);
    window.addEventListener("keydown", closeOnEscape);
    window.addEventListener("resize", updatePopoverPosition);
    window.addEventListener("scroll", updatePopoverPosition, true);
    return () => {
      window.removeEventListener("mousedown", closeOnPointerDown);
      window.removeEventListener("keydown", closeOnEscape);
      window.removeEventListener("resize", updatePopoverPosition);
      window.removeEventListener("scroll", updatePopoverPosition, true);
    };
  }, [onOpenChange, open, openRequestKey]);

  const commitColor = (candidate: string) => {
    const next = normalizeHexColor(candidate);
    if (next === null) return;
    setPickerDraft({ value: next, requestKey: openRequestKey });
    if (next !== normalizedValue) void commit(next);
  };

  const updateRgb = (nextRgb: RgbColor) => {
    commitColor(rgbToHex(nextRgb));
  };

  const updateHsv = (nextHsv: HsvColor) => {
    commitColor(rgbToHex(hsvToRgb(nextHsv)));
  };

  return (
    <div ref={rootRef} className={`color-picker ${className}`}>
      <button
        ref={triggerRef}
        type="button"
        className={`color-picker-trigger ${triggerClassName}`}
        aria-label={label}
        title={label}
        style={{ "--color-picker-value": draft } as CSSProperties}
        onPointerDown={(event) => {
          if (stopTriggerPointerDownPropagation) event.stopPropagation();
        }}
        onClick={() => {
          if (!open) setPickerDraft({ value: normalizedValue, requestKey: openRequestKey });
          setInternalOpen(!open);
          if (open) setDismissedOpenRequestKey(openRequestKey);
          onOpenChange?.(!open);
        }}
      />
      {open && createPortal(
        <div
          ref={popoverRef}
          className="color-picker-popover"
          style={{ top: popoverPosition.top, left: popoverPosition.left }}
          onPointerDown={(event) => { event.stopPropagation(); }}
          onPointerMove={(event) => { event.stopPropagation(); }}
          onPointerUp={(event) => { event.stopPropagation(); }}
          onPointerCancel={(event) => { event.stopPropagation(); }}
          onClick={(event) => { event.stopPropagation(); }}
        >
          <HexColorPicker
            color={draft}
            onChange={(next) => {
              setPickerDraft({ value: next, requestKey: openRequestKey });
            }}
            onChangeEnd={commitColor}
          />
          <div className="color-picker-grid">
            <label>
              Hex
              <input
                key={`hex:${draft}`}
                defaultValue={draft}
                onBlur={(event) => {
                  commitColor(event.currentTarget.value);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    commitColor(event.currentTarget.value);
                    event.currentTarget.blur();
                  }
                }}
              />
            </label>
            <NumberColorInput label="R" value={rgb.r} min={0} max={255} commit={(r) => { updateRgb({ ...rgb, r }); }} />
            <NumberColorInput label="G" value={rgb.g} min={0} max={255} commit={(g) => { updateRgb({ ...rgb, g }); }} />
            <NumberColorInput label="B" value={rgb.b} min={0} max={255} commit={(b) => { updateRgb({ ...rgb, b }); }} />
            <NumberColorInput label="H" value={Math.round(hsv.h)} min={0} max={360} commit={(h) => { updateHsv({ ...hsv, h }); }} />
            <NumberColorInput label="S" value={Math.round(hsv.s)} min={0} max={100} commit={(s) => { updateHsv({ ...hsv, s }); }} />
            <NumberColorInput label="V" value={Math.round(hsv.v)} min={0} max={100} commit={(v) => { updateHsv({ ...hsv, v }); }} />
          </div>
        </div>,
        document.body
      )}
    </div>
  );
}

function NumberColorInput({
  label,
  value,
  min,
  max,
  commit
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  commit: (value: number) => void;
}) {
  return (
    <label>
      {label}
      <input
        key={`${label}:${value}`}
        type="number"
        min={min}
        max={max}
        defaultValue={value}
        onBlur={(event) => {
          const next = Number(event.currentTarget.value);
          if (Number.isFinite(next)) commit(Math.round(clamp(next, min, max)));
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
        }}
      />
    </label>
  );
}

function hexToRgb(hex: string): RgbColor {
  const normalized = normalizeHexColor(hex) ?? THEME_COLORS.white;
  return {
    r: Number.parseInt(normalized.slice(1, 3), 16),
    g: Number.parseInt(normalized.slice(3, 5), 16),
    b: Number.parseInt(normalized.slice(5, 7), 16)
  };
}

function rgbToHex({ r, g, b }: RgbColor) {
  return `#${hexByte(r)}${hexByte(g)}${hexByte(b)}`;
}

function hexByte(value: number) {
  return Math.round(clamp(value, 0, 255)).toString(16).padStart(2, "0");
}

function rgbToHsv({ r, g, b }: RgbColor): HsvColor {
  const red = r / 255;
  const green = g / 255;
  const blue = b / 255;
  const max = Math.max(red, green, blue);
  const min = Math.min(red, green, blue);
  const delta = max - min;
  const h =
    delta === 0
      ? 0
      : max === red
        ? 60 * (((green - blue) / delta) % 6)
        : max === green
          ? 60 * ((blue - red) / delta + 2)
          : 60 * ((red - green) / delta + 4);
  return {
    h: h < 0 ? h + 360 : h,
    s: max === 0 ? 0 : (delta / max) * 100,
    v: max * 100
  };
}

function hsvToRgb({ h, s, v }: HsvColor): RgbColor {
  const normalizedHue = ((h % 360) + 360) % 360;
  const saturation = clamp(s, 0, 100) / 100;
  const value = clamp(v, 0, 100) / 100;
  const chroma = value * saturation;
  const x = chroma * (1 - Math.abs(((normalizedHue / 60) % 2) - 1));
  const m = value - chroma;
  const [red, green, blue] =
    normalizedHue < 60
      ? [chroma, x, 0]
      : normalizedHue < 120
        ? [x, chroma, 0]
        : normalizedHue < 180
          ? [0, chroma, x]
          : normalizedHue < 240
            ? [0, x, chroma]
            : normalizedHue < 300
              ? [x, 0, chroma]
              : [chroma, 0, x];
  return {
    r: Math.round((red + m) * 255),
    g: Math.round((green + m) * 255),
    b: Math.round((blue + m) * 255)
  };
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

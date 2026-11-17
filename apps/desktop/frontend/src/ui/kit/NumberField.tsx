import { useRef, useState, type CSSProperties, type KeyboardEvent, type MouseEvent, type PointerEvent } from "react";
import { THEME_METRICS } from "../../theme";

type NumberFieldProps = {
  value: number;
  onCommit: (value: number) => void;
  /** The value one scrub step or arrow press moves by. */
  step: number;
  /** Double-click resets to this value. */
  defaultValue?: number;
  min?: number;
  max?: number;
  integer?: boolean;
  /** A short name drawn inside the field, such as "X". */
  label?: string;
  unit?: string;
  /** Draw `min`–`max` inside the field, so the allowed values are visible before typing. */
  showRange?: boolean;
  format?: (value: number) => string;
  disabled?: boolean;
  invalid?: boolean;
  /** The accessible name; also the tooltip when there is no visible label. */
  name: string;
  className?: string;
};

type Scrub = { pointer: number; startValue: number; travel: number; scrubbing: boolean };

/** Shift moves a tenth of a step and Ctrl or Command ten steps, for scrubs and arrow keys alike. */
const stepScale = (event: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) => event.shiftKey ? 0.1 : event.ctrlKey || event.metaKey ? 10 : 1;
/** The keys that move the caret through typed text. */
const CARET_KEYS = new Set(["ArrowLeft", "ArrowRight", "Home", "End"]);

/**
 * The app's number control. Drag sideways to scrub (Shift for fine, Ctrl for coarse), double-click
 * to reset, click to type; Enter commits and Escape reverts. A scrub commits once, on release.
 * The value is as wide shown as typed: wide enough for its range, or for itself when the range is open.
 */
export function NumberField({ value, onCommit, step, defaultValue, min, max, integer = false, label, unit, showRange = false, format, disabled = false, invalid = false, name, className }: NumberFieldProps) {
  const [draft, setDraft] = useState<number | null>(null);
  const [text, setText] = useState<string | null>(null);
  const scrub = useRef<Scrub | null>(null);
  const root = useRef<HTMLDivElement | null>(null);
  // Escape hands focus back to the field, and that blur must not commit the abandoned text.
  const cancelled = useRef(false);
  const shown = draft ?? value;
  const show = (number: number) => format === undefined ? formatNumber(number, step, integer) : format(number);
  const display = show(shown);
  const chars = Math.max(display.length, min !== undefined && max !== undefined ? Math.max(show(min).length, show(max).length) : 0);

  const clamp = (next: number) => {
    let result = integer ? Math.round(next) : next;
    if (min !== undefined) result = Math.max(min, result);
    if (max !== undefined) result = Math.min(max, result);
    return result;
  };
  const commit = (next: number) => {
    const clamped = clamp(next);
    if (clamped !== value) onCommit(clamped);
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (disabled || event.button !== 0 || text !== null) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    scrub.current = { pointer: event.pointerId, startValue: value, travel: 0, scrubbing: false };
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const current = scrub.current;
    if (current?.pointer !== event.pointerId) return;
    current.travel += event.movementX;
    if (!current.scrubbing) {
      if (Math.abs(current.travel) < THEME_METRICS.interactionDragThreshold) return;
      current.scrubbing = true;
      void event.currentTarget.requestPointerLock();
    }
    const steps = Math.round(current.travel / THEME_METRICS.numberScrubPixelsPerStep);
    setDraft(clamp(current.startValue + steps * step * stepScale(event)));
  };
  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    const current = scrub.current;
    if (current?.pointer !== event.pointerId) return;
    scrub.current = null;
    if (document.pointerLockElement !== null) document.exitPointerLock();
    if (current.scrubbing) {
      if (draft !== null) commit(draft);
      setDraft(null);
      return;
    }
    setText(String(value));
  };
  const onMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.detail !== 2 || defaultValue === undefined || disabled) return;
    event.preventDefault();
    setText(null);
    commit(defaultValue);
  };
  // Up and Down step, as on any spinbutton; Left and Right step as well, so arrows on a focused field never nudge the canvas behind it.
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (disabled || event.target !== event.currentTarget) return;
    const direction = event.key === "ArrowUp" || event.key === "ArrowRight" ? 1 : event.key === "ArrowDown" || event.key === "ArrowLeft" ? -1 : 0;
    if (direction !== 0) {
      event.preventDefault();
      commit(value + direction * step * stepScale(event));
    } else if (event.key === "Enter") {
      event.preventDefault();
      setText(String(value));
    }
  };
  // Typing keeps its caret keys; Up and Down step the typed value; Enter and Escape hand focus back to the field.
  const onTextKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (CARET_KEYS.has(event.key)) {
      event.stopPropagation();
    } else if (event.key === "ArrowUp" || event.key === "ArrowDown") {
      event.preventDefault();
      event.stopPropagation();
      const typed = event.currentTarget.value.trim();
      const base = typed !== "" && Number.isFinite(Number(typed)) ? Number(typed) : value;
      setText(formatNumber(clamp(base + (event.key === "ArrowUp" ? 1 : -1) * step * stepScale(event)), step, integer));
    } else if (event.key === "Enter" || event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancelled.current = event.key === "Escape";
      root.current?.focus();
    }
  };
  const finishText = () => {
    if (text === null) return;
    const abandoned = cancelled.current;
    cancelled.current = false;
    const parsed = Number(text.trim());
    setText(null);
    if (!abandoned && text.trim() !== "" && Number.isFinite(parsed)) commit(parsed);
  };

  return <div
    ref={root}
    className={["kit-number", draft !== null ? "scrubbing" : null, className].filter(Boolean).join(" ")}
    style={{ "--kit-value-chars": chars } as CSSProperties}
    role="spinbutton"
    aria-label={name}
    aria-valuenow={shown}
    aria-valuemin={min}
    aria-valuemax={max}
    aria-disabled={disabled}
    aria-invalid={invalid}
    tabIndex={disabled || text !== null ? -1 : 0}
    onPointerDown={onPointerDown}
    onPointerMove={onPointerMove}
    onPointerUp={onPointerUp}
    onPointerCancel={onPointerUp}
    onMouseDown={onMouseDown}
    onKeyDown={onKeyDown}
  >
    {label !== undefined && <span className="kit-number-label">{label}</span>}
    {showRange && min !== undefined && max !== undefined &&
      <span className="kit-number-range">{show(min)}–{show(max)}</span>}
    {text === null
      ? <span className="kit-number-value">{display}</span>
      : <input
        className="kit-number-input"
        autoFocus
        value={text}
        inputMode="decimal"
        aria-label={name}
        onFocus={(event) => { event.currentTarget.select(); }}
        onChange={(event) => { setText(event.target.value); }}
        onKeyDown={onTextKeyDown}
        onBlur={finishText}
      />}
    {unit !== undefined && <span className="kit-number-unit">{unit}</span>}
  </div>;
}

function formatNumber(value: number, step: number, integer: boolean): string {
  if (integer) return String(Math.round(value));
  const decimals = Math.min(6, Math.max(0, -Math.floor(Math.log10(Math.abs(step) / 10 || 1))));
  return String(Number(value.toFixed(decimals)));
}

import { useRef, useState, type KeyboardEvent, type MouseEvent, type PointerEvent } from "react";
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
  format?: (value: number) => string;
  disabled?: boolean;
  invalid?: boolean;
  /** The accessible name; also the tooltip when there is no visible label. */
  name: string;
  className?: string;
};

type Scrub = { pointer: number; startValue: number; travel: number; scrubbing: boolean };

/**
 * The app's number control. Drag sideways to scrub (Shift for fine, Ctrl for coarse), double-click
 * to reset, click to type; Enter commits and Escape reverts. A scrub commits once, on release.
 */
export function NumberField({ value, onCommit, step, defaultValue, min, max, integer = false, label, unit, format, disabled = false, invalid = false, name, className }: NumberFieldProps) {
  const [draft, setDraft] = useState<number | null>(null);
  const [text, setText] = useState<string | null>(null);
  const scrub = useRef<Scrub | null>(null);
  const shown = draft ?? value;
  const display = format === undefined ? formatNumber(shown, step, integer) : format(shown);

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
    const scale = event.shiftKey ? 0.1 : event.ctrlKey || event.metaKey ? 10 : 1;
    const steps = Math.round(current.travel / THEME_METRICS.numberScrubPixelsPerStep);
    setDraft(clamp(current.startValue + steps * step * scale));
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
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (disabled) return;
    const direction = event.key === "ArrowUp" || event.key === "ArrowRight" ? 1 : event.key === "ArrowDown" || event.key === "ArrowLeft" ? -1 : 0;
    if (direction !== 0) {
      event.preventDefault();
      commit(value + direction * step * (event.shiftKey ? 0.1 : 1));
    } else if (event.key === "Enter") {
      event.preventDefault();
      setText(String(value));
    }
  };
  const onTextKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      event.currentTarget.blur();
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      setText(null);
    }
  };
  const finishText = () => {
    if (text === null) return;
    const parsed = Number(text.trim());
    setText(null);
    if (text.trim() !== "" && Number.isFinite(parsed)) commit(parsed);
  };

  return <div
    className={["kit-number", draft !== null ? "scrubbing" : null, className].filter(Boolean).join(" ")}
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
    {text === null
      ? <span className="kit-number-value">{display}{unit !== undefined && <span className="kit-number-unit">{unit}</span>}</span>
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
  </div>;
}

function formatNumber(value: number, step: number, integer: boolean): string {
  if (integer) return String(Math.round(value));
  const decimals = Math.min(6, Math.max(0, -Math.floor(Math.log10(Math.abs(step) / 10 || 1))));
  return String(Number(value.toFixed(decimals)));
}

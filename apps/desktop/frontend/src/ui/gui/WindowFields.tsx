import type { GuiWindow, SequenceParamRange } from "../../types";

const PERCENT = 100;

/** A fraction as a percentage a field shows, without float noise. */
function percent(fraction: number): number {
  return Number((fraction * PERCENT).toFixed(2));
}

/** A value of the parameter's range at a fraction of it, without float noise. */
function atFraction(range: SequenceParamRange, fraction: number): number {
  return Number((range.min + fraction * (range.max - range.min)).toFixed(3));
}

/** The part of its `0..1` a binding uses, as two percentages of the parameter's
 * range: 0 lands on the first and 1 on the second, so a second below the first
 * inverts the control. Each commits when it loses focus. A numeric parameter's
 * `range` also shows the values the window reaches. */
export function WindowFields({ value, range, onChange }: { value: GuiWindow; range: SequenceParamRange | null; onChange: (window: GuiWindow) => void }) {
  const field = (end: keyof GuiWindow, label: string) => <input
    key={value[end]}
    className="window-field"
    type="number"
    min={0}
    max={PERCENT}
    step="any"
    aria-label={label}
    title={label}
    defaultValue={percent(value[end])}
    onBlur={(event) => {
      const next = Number(event.currentTarget.value);
      if (Number.isNaN(next) || next === percent(value[end])) return;
      onChange({ ...value, [end]: Math.min(PERCENT, Math.max(0, next)) / PERCENT });
    }} />;
  return <span className="window-fields" title="The part of the parameter's range this control covers">
    {field("low", "Percent of the range at the bottom of the control")}
    <span aria-hidden="true">–</span>
    {field("high", "Percent of the range at the top of the control")}
    <span aria-hidden="true">%</span>
    {range !== null && <span className="window-result" title={`The declared range is ${String(range.min)} to ${String(range.max)}`}>
      = {atFraction(range, value.low)} to {atFraction(range, value.high)}
    </span>}
  </span>;
}

import type { GuiWindow, SequenceParamRange } from "../../types";
import { NumberField, Tip } from "../kit";

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
 * inverts the control. A numeric parameter's `range` also shows the values the
 * window reaches. */
export function WindowFields({ value, range, onChange }: { value: GuiWindow; range: SequenceParamRange | null; onChange: (window: GuiWindow) => void }) {
  const field = (end: keyof GuiWindow, name: string) => <NumberField
    className="window-field"
    name={name}
    value={percent(value[end])}
    step={1}
    min={0}
    max={PERCENT}
    defaultValue={end === "low" ? 0 : PERCENT}
    unit="%"
    onCommit={(next) => { onChange({ ...value, [end]: next / PERCENT }); }} />;
  return <span className="window-fields">
    {field("low", "Range at the control's bottom")}
    <span aria-hidden="true">–</span>
    {field("high", "Range at the control's top")}
    {range !== null && <Tip label={`Declared range ${String(range.min)} to ${String(range.max)}`}>
      <span className="window-result">= {atFraction(range, value.low)} to {atFraction(range, value.high)}</span>
    </Tip>}
  </span>;
}

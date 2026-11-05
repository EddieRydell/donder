import { Check } from "lucide-react";
import { THEME_METRICS } from "../../theme";

/** The app's on/off control: a square check. Put the name in a `Field` or pass `label`. */
export function Toggle({ checked, onChange, name, label, disabled = false }: { checked: boolean; onChange: (checked: boolean) => void; name: string; label?: string; disabled?: boolean }) {
  return <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={name}
    disabled={disabled}
    className="kit-toggle"
    onClick={() => { onChange(!checked); }}
  >
    <span className="kit-toggle-box">{checked && <Check size={THEME_METRICS.iconSizeExtraSmall} aria-hidden />}</span>
    {label !== undefined && <span>{label}</span>}
  </button>;
}

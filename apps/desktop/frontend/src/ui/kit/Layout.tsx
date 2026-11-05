import { ChevronRight, Plus } from "lucide-react";
import { useState, type ReactNode } from "react";
import { THEME_METRICS } from "../../theme";
import { Tip } from "./Tip";

/** A labelled row: a short name on the left, the control on the right; `tip` explains it on hover. */
export function Field({ label, tip, children }: { label: string; tip?: string | undefined; children: ReactNode }) {
  const name = <span className="kit-field-label">{label}</span>;
  return <div className="kit-field">
    {tip === undefined ? name : <Tip label={tip}>{name}</Tip>}
    <div className="kit-field-control">{children}</div>
  </div>;
}

/** Controls that sit side by side in one field, such as X and Y. */
export function FieldGroup({ children }: { children: ReactNode }) {
  return <div className="kit-field-group">{children}</div>;
}

/** A collapsible group with a small uppercase header and optional header actions. */
export function Section({ title, actions, defaultOpen = true, children }: { title: string; actions?: ReactNode; defaultOpen?: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(defaultOpen);
  return <section className="kit-section" data-open={open}>
    <div className="kit-section-header">
      <button type="button" className="kit-section-toggle" aria-expanded={open} onClick={() => { setOpen(!open); }}>
        <ChevronRight size={THEME_METRICS.iconSizeExtraSmall} aria-hidden className="kit-section-chevron" />
        <span>{title}</span>
      </button>
      {actions !== undefined && <div className="kit-section-actions">{actions}</div>}
    </div>
    {open && <div className="kit-section-body">{children}</div>}
  </section>;
}

/** What an empty list shows: one "+" that adds the first item. */
export function AddRow({ label, onAdd, disabled = false }: { label: string; onAdd: () => void; disabled?: boolean }) {
  return <Tip label={label}>
    <button type="button" className="kit-add-row" aria-label={label} disabled={disabled} onClick={onAdd}>
      <Plus size={THEME_METRICS.iconSizeSmall} aria-hidden />
    </button>
  </Tip>;
}

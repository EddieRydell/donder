import { ChevronRight, Plus } from "lucide-react";
import { forwardRef, useState, type ButtonHTMLAttributes, type ReactNode } from "react";
import { THEME_METRICS } from "../../theme";
import { MenuButton, type MenuEntry } from "./Menu";
import { Tip } from "./Tip";

/**
 * A labelled control: in a column of fields the short name sits on the left at a shared width; in a
 * wrapping row of fields it sits above its own control. `tip` explains it on hover. `error` marks
 * the field invalid and is its tooltip instead; the control shows its own `invalid` state.
 */
export function Field({ label, tip, error, children }: { label: string; tip?: string | undefined; error?: string | null | undefined; children: ReactNode }) {
  const name = <span className="kit-field-label">{label}</span>;
  if (error !== undefined && error !== null) {
    return <Tip label={error}>
      <div className="kit-field" data-invalid="true">
        {name}
        <div className="kit-field-control">{children}</div>
      </div>
    </Tip>;
  }
  return <div className="kit-field">
    {tip === undefined ? name : <Tip label={tip}>{name}</Tip>}
    <div className="kit-field-control">{children}</div>
  </div>;
}

/**
 * A control outside a labelled field, such as a grid cell or a row's name, that breaks a rule:
 * `error` is its tooltip, and the control shows its own `invalid` state. No error, no tooltip.
 */
export function Problem({ error, children }: { error: string | null; children: ReactNode }) {
  return error === null ? children : <Tip label={error}><span className="kit-problem">{children}</span></Tip>;
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

/** What an empty list shows: one "+" that adds the first item, or opens `entries` when there are several kinds to add. */
export function AddRow({ label, disabled = false, ...add }: { label: string; disabled?: boolean } & ({ onAdd: () => void } | { entries: MenuEntry[] })) {
  return "entries" in add
    ? <MenuButton entries={add.entries}><AddRowButton label={label} disabled={disabled} /></MenuButton>
    : <AddRowButton label={label} disabled={disabled} onClick={add.onAdd} />;
}

const AddRowButton = forwardRef<HTMLButtonElement, Omit<ButtonHTMLAttributes<HTMLButtonElement>, "aria-label" | "children"> & { label: string }>(function AddRowButton({ label, ...rest }, ref) {
  return <Tip label={label}>
    <button ref={ref} type="button" className="kit-add-row" aria-label={label} {...rest}>
      <Plus size={THEME_METRICS.iconSizeSmall} aria-hidden />
    </button>
  </Tip>;
});

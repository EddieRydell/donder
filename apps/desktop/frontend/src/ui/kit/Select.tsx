import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Check, ChevronDown, type LucideIcon } from "lucide-react";
import { useContext } from "react";
import { THEME_METRICS } from "../../theme";
import { OverlayPortal } from "../OverlayPortal";

export type SelectOption<T extends string> = { value: T; label: string; icon?: LucideIcon; disabled?: boolean };

type SelectProps<T extends string> = {
  value: T;
  options: readonly SelectOption<T>[];
  onChange: (value: T) => void;
  /** The accessible name. */
  name: string;
  disabled?: boolean;
  invalid?: boolean;
  className?: string;
};

/** The app's choice control: a compact button that opens a checked menu. */
export function Select<T extends string>({ value, options, onChange, name, disabled = false, invalid = false, className }: SelectProps<T>) {
  const container = useContext(OverlayPortal);
  const current = options.find((option) => option.value === value);
  const Icon = current?.icon;
  return <DropdownMenu.Root modal={false}>
    <DropdownMenu.Trigger asChild disabled={disabled}>
      <button type="button" className={["kit-select", className].filter(Boolean).join(" ")} aria-label={name} aria-invalid={invalid}>
        {Icon !== undefined && <Icon size={THEME_METRICS.iconSizeSmall} aria-hidden />}
        <span className="kit-select-value">{current?.label ?? value}</span>
        <ChevronDown size={THEME_METRICS.iconSizeExtraSmall} aria-hidden />
      </button>
    </DropdownMenu.Trigger>
    <DropdownMenu.Portal container={container}>
      <DropdownMenu.Content className="menu-content" align="start" sideOffset={THEME_METRICS.popoverOffset}>
        <DropdownMenu.RadioGroup value={value} onValueChange={(next) => { onChange(next as T); }}>
          {options.map((option) => {
            const OptionIcon = option.icon;
            return <DropdownMenu.RadioItem key={option.value} value={option.value} disabled={option.disabled ?? false} className="menu-item kit-menu-check-item">
              <DropdownMenu.ItemIndicator className="kit-menu-indicator"><Check size={THEME_METRICS.iconSizeExtraSmall} /></DropdownMenu.ItemIndicator>
              {OptionIcon !== undefined && <OptionIcon size={THEME_METRICS.iconSizeSmall} aria-hidden />}
              <span>{option.label}</span>
            </DropdownMenu.RadioItem>;
          })}
        </DropdownMenu.RadioGroup>
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  </DropdownMenu.Root>;
}

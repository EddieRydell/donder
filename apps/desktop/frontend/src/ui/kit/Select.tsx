import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Check, ChevronDown, type LucideIcon } from "lucide-react";
import { forwardRef, useContext, type ButtonHTMLAttributes } from "react";
import { THEME_METRICS } from "../../theme";
import { OverlayPortal } from "../OverlayPortal";
import { MenuButton, type MenuEntry } from "./Menu";

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
  return <DropdownMenu.Root modal={false}>
    <DropdownMenu.Trigger asChild disabled={disabled}>
      <SelectTrigger label={current?.label ?? value} icon={current?.icon} className={className} aria-label={name} aria-invalid={invalid} />
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


type SelectTriggerProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> & { label: string; icon?: LucideIcon | undefined };

/** The closed look of a choice control: the current choice and a chevron. */
const SelectTrigger = forwardRef<HTMLButtonElement, SelectTriggerProps>(function SelectTrigger({ label, icon: Icon, className, ...rest }, ref) {
  return <button ref={ref} type="button" className={["kit-select", className].filter(Boolean).join(" ")} {...rest}>
    {Icon !== undefined && <Icon size={THEME_METRICS.iconSizeSmall} aria-hidden />}
    <span className="kit-select-value">{label}</span>
    <ChevronDown size={THEME_METRICS.iconSizeExtraSmall} aria-hidden />
  </button>;
});

/** A choice control whose choices are a menu tree, such as definitions in their folders. */
export function MenuSelect({ label, name, entries, disabled = false }: { label: string; name: string; entries: MenuEntry[]; disabled?: boolean }) {
  return <MenuButton entries={entries}>
    <SelectTrigger label={label} aria-label={name} disabled={disabled} />
  </MenuButton>;
}

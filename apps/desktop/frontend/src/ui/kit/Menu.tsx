import * as ContextMenu from "@radix-ui/react-context-menu";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Check, ChevronRight, type LucideIcon } from "lucide-react";
import { useContext, type ReactNode } from "react";
import { commandRegistry, runCommand, shortcutLabel } from "../../commandRegistry";
import type { CommandId } from "../../types";
import { THEME_METRICS } from "../../theme";
import { OverlayPortal } from "../OverlayPortal";

/**
 * One menu line. A `command` entry takes its label, state and shortcut from the registry. An
 * action's `color` is a data color, such as a mark collection's, shown on its label.
 */
export type MenuEntry =
  | { type: "command"; id: CommandId }
  | { type: "action"; label: string; icon?: LucideIcon; color?: string; shortcut?: CommandId; danger?: boolean; disabled?: boolean; onSelect: () => void }
  | { type: "submenu"; label: string; icon?: LucideIcon; disabled?: boolean; entries: MenuEntry[] }
  /** A checked option; toggling it keeps the menu open so several can change at once. */
  | { type: "check"; label: string; checked: boolean; detail?: string; disabled?: boolean; onCheckedChange: (checked: boolean) => void }
  | { type: "separator" };

type MenuParts = typeof ContextMenu | typeof DropdownMenu;

function Entries({ parts: M, entries }: { parts: MenuParts; entries: MenuEntry[] }) {
  const container = useContext(OverlayPortal);
  return <>{entries.map((entry, index) => {
    if (entry.type === "separator") return <M.Separator key={`separator-${index}`} className="menu-separator" />;
    if (entry.type === "submenu") {
      const Icon = entry.icon;
      return <M.Sub key={`${index}-${entry.label}`}>
        <M.SubTrigger className="menu-item" disabled={entry.disabled ?? false}>
          <span className="kit-menu-label">{Icon !== undefined && <Icon size={THEME_METRICS.iconSizeSmall} aria-hidden />}{entry.label}</span>
          <ChevronRight size={THEME_METRICS.iconSizeExtraSmall} aria-hidden />
        </M.SubTrigger>
        <M.Portal container={container}>
          <M.SubContent className="menu-content"><Entries parts={M} entries={entry.entries} /></M.SubContent>
        </M.Portal>
      </M.Sub>;
    }
    if (entry.type === "check") {
      return <M.CheckboxItem key={`${index}-${entry.label}`} className="menu-item kit-menu-check-item" checked={entry.checked} disabled={entry.disabled ?? false}
        onSelect={(event) => { event.preventDefault(); }} onCheckedChange={entry.onCheckedChange}>
        <M.ItemIndicator className="kit-menu-indicator"><Check size={THEME_METRICS.iconSizeExtraSmall} /></M.ItemIndicator>
        <span className="kit-menu-label">{entry.label}</span>
        {entry.detail !== undefined && <span className="shortcut">{entry.detail}</span>}
      </M.CheckboxItem>;
    }
    const item = entry.type === "command"
      ? { label: commandRegistry[entry.id].label, icon: undefined, color: undefined, shortcut: entry.id, danger: false, disabled: !commandRegistry[entry.id].enabled(), onSelect: () => { runCommand(entry.id); } }
      : entry;
    const Icon = item.icon;
    const keys = item.shortcut === undefined ? undefined : shortcutLabel(item.shortcut);
    return <M.Item key={`${index}-${item.label}`} className={item.danger === true ? "menu-item danger" : "menu-item"} disabled={item.disabled ?? false} onSelect={item.onSelect}>
      <span className="kit-menu-label" style={item.color === undefined ? undefined : { color: item.color }}>{Icon !== undefined && <Icon size={THEME_METRICS.iconSizeSmall} aria-hidden />}{item.label}</span>
      {keys !== undefined && <span className="shortcut">{keys}</span>}
    </M.Item>;
  })}</>;
}

/** Right-click on `children` opens `entries`. */
export function ContextMenuArea({ entries, children, disabled = false }: { entries: MenuEntry[]; children: ReactNode; disabled?: boolean }) {
  const container = useContext(OverlayPortal);
  return <ContextMenu.Root modal={false}>
    <ContextMenu.Trigger asChild disabled={disabled}>{children}</ContextMenu.Trigger>
    <ContextMenu.Portal container={container}>
      <ContextMenu.Content className="menu-content"><Entries parts={ContextMenu} entries={entries} /></ContextMenu.Content>
    </ContextMenu.Portal>
  </ContextMenu.Root>;
}

/** The items of an already-open context menu, for surfaces that open it themselves. */
export function ContextMenuEntries({ entries }: { entries: MenuEntry[] }) {
  return <Entries parts={ContextMenu} entries={entries} />;
}

/** A button, usually an `IconButton`, that opens `entries` below it. */
export function MenuButton({ entries, children, align = "start" }: { entries: MenuEntry[]; children: ReactNode; align?: "start" | "end" }) {
  const container = useContext(OverlayPortal);
  return <DropdownMenu.Root modal={false}>
    <DropdownMenu.Trigger asChild>{children}</DropdownMenu.Trigger>
    <DropdownMenu.Portal container={container}>
      <DropdownMenu.Content className="menu-content" align={align} sideOffset={THEME_METRICS.popoverOffset}>
        <Entries parts={DropdownMenu} entries={entries} />
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  </DropdownMenu.Root>;
}

import * as Dialog from "@radix-ui/react-dialog";
import { Command } from "cmdk";
import { useContext, useEffect, useState, type ReactNode } from "react";
import { commandRegistry, runCommand, shortcutLabel } from "../commandRegistry";
import { listenUiEvent } from "../ui/uiEvents";
import type { CommandId } from "../types";
import { OverlayPortal } from "../ui/OverlayPortal";
import { matchesCommand } from "./helpers";

export function CommandOverlays() {
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteQuery, setPaletteQuery] = useState("");

  useEffect(() => {
    return listenUiEvent("openCommandPalette", () => { setPaletteOpen(true); });
  }, []);

  const availableCommands = (
    Object.entries(commandRegistry) as Array<[CommandId, (typeof commandRegistry)[CommandId]]>
  )
    .filter(([, command]) => command.enabled())
    .filter(([, command]) =>
      matchesCommand(command.label, command.category, command.keywords, paletteQuery)
    );

  return (
    <>
      <CommandDialog
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        label="Command Palette"
        query={paletteQuery}
        onQueryChange={setPaletteQuery}
        placeholder="Command"
      >
        {availableCommands.map(([id, command]) => {
          const keys = shortcutLabel(id);
          return <Command.Item
            key={id}
            value={`${command.category} ${command.label} ${command.keywords.join(" ")}`}
            onSelect={() => {
              setPaletteOpen(false);
              runCommand(id);
            }}
          >
            <span className="command-item-main">
              <small>{command.category}</small>
              {command.label}
            </span>
            {keys !== undefined && <kbd>{keys}</kbd>}
          </Command.Item>;
        })}
      </CommandDialog>
    </>
  );
}

function CommandDialog({
  open,
  onOpenChange,
  label,
  query,
  onQueryChange,
  placeholder,
  children
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  label: string;
  query: string;
  onQueryChange: (query: string) => void;
  placeholder: string;
  children: ReactNode;
}) {
  const container = useContext(OverlayPortal);
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal container={container}>
        <Dialog.Overlay className="kit-dialog-overlay" />
        <Dialog.Content className="command-dialog" aria-label={label} aria-describedby={undefined}>
          <Dialog.Title className="visually-hidden">{label}</Dialog.Title>
          <Command shouldFilter={false} label={label}>
            <Command.Input
              autoFocus
              value={query}
              onValueChange={onQueryChange}
              placeholder={placeholder}
            />
            <Command.List>
              <Command.Empty>No matches</Command.Empty>
              {children}
            </Command.List>
          </Command>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

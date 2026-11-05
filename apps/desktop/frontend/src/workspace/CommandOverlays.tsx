import * as Dialog from "@radix-ui/react-dialog";
import { Command } from "cmdk";
import { useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  OPEN_COMMAND_PALETTE_EVENT,
  OPEN_QUICK_OPEN_EVENT,
  commandRegistry,
  runCommand,
  shortcutLabel
} from "../commandRegistry";
import type { CommandId } from "../types";
import { runSnapshotCommand, useAppStore } from "../store";
import { commands } from "../api";
import { OverlayPortal } from "../ui/OverlayPortal";
import { matchesCommand, rankQuickOpenFiles } from "./helpers";

export function CommandOverlays() {
  const snapshot = useAppStore((state) => state.snapshot);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [quickOpen, setQuickOpen] = useState(false);
  const [paletteQuery, setPaletteQuery] = useState("");
  const [fileQuery, setFileQuery] = useState("");

  useEffect(() => {
    const openPalette = () => { setPaletteOpen(true); };
    const openFiles = () => { setQuickOpen(true); };
    window.addEventListener(OPEN_COMMAND_PALETTE_EVENT, openPalette);
    window.addEventListener(OPEN_QUICK_OPEN_EVENT, openFiles);
    return () => {
      window.removeEventListener(OPEN_COMMAND_PALETTE_EVENT, openPalette);
      window.removeEventListener(OPEN_QUICK_OPEN_EVENT, openFiles);
    };
  }, []);

  const availableCommands = (
    Object.entries(commandRegistry) as Array<[CommandId, (typeof commandRegistry)[CommandId]]>
  )
    .filter(([, command]) => command.enabled())
    .filter(([, command]) =>
      matchesCommand(command.label, command.category, command.keywords, paletteQuery)
    );
  const files = useMemo(() => {
    if (snapshot === null) return [];
    const query = fileQuery.trim().toLowerCase();
    return rankQuickOpenFiles(snapshot).filter((path) => path.toLowerCase().includes(query));
  }, [fileQuery, snapshot]);

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
      <CommandDialog
        open={quickOpen}
        onOpenChange={setQuickOpen}
        label="Quick Open"
        query={fileQuery}
        onQueryChange={setFileQuery}
        placeholder="File"
      >
        {files.map((path) => (
          <Command.Item
            key={path}
            value={path}
            onSelect={() => {
              setQuickOpen(false);
              void runSnapshotCommand(() => commands.openFile(path));
            }}
          >
            <span>{path}</span>
          </Command.Item>
        ))}
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

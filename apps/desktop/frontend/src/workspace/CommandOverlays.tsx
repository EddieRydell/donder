import * as Dialog from "@radix-ui/react-dialog";
import { Command } from "cmdk";
import { FileText, TerminalSquare } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import {
  OPEN_COMMAND_PALETTE_EVENT,
  OPEN_QUICK_OPEN_EVENT,
  commandRegistry,
  type CommandId
} from "../commandRegistry";
import { runSnapshotCommand, useAppStore } from "../store";
import { commands } from "../api";
import { THEME_METRICS } from "../theme";
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
        placeholder="Type a command"
      >
        <Command.Group heading="Commands">
          {availableCommands.map(([id, command]) => (
            <Command.Item
              key={id}
              value={`${command.category} ${command.label} ${command.keywords.join(" ")}`}
              onSelect={() => {
                setPaletteOpen(false);
                void command.run();
              }}
            >
              <TerminalSquare size={THEME_METRICS.iconSizeSmall} />
              <span className="command-item-main">
                <small>{command.category}</small>
                {command.label}
              </span>
              {command.shortcut !== undefined && <kbd>{command.shortcut}</kbd>}
            </Command.Item>
          ))}
        </Command.Group>
      </CommandDialog>
      <CommandDialog
        open={quickOpen}
        onOpenChange={setQuickOpen}
        label="Quick Open"
        query={fileQuery}
        onQueryChange={setFileQuery}
        placeholder="Search files by name"
      >
        <Command.Group heading="Files">
          {files.map((path) => (
            <Command.Item
              key={path}
              value={path}
              onSelect={() => {
                setQuickOpen(false);
                void runSnapshotCommand(() => commands.openFile(path));
              }}
            >
              <FileText size={THEME_METRICS.iconSizeSmall} />
              <span>{path}</span>
            </Command.Item>
          ))}
        </Command.Group>
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
  children: React.ReactNode;
}) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="command-overlay" />
        <Dialog.Content className="command-dialog" aria-label={label}>
          <Dialog.Title className="visually-hidden">{label}</Dialog.Title>
          <Command shouldFilter={false}>
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

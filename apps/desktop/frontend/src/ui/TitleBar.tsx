import * as Menubar from "@radix-ui/react-menubar";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { Check, Maximize2, Minimize2, Minus, Redo2, Undo2, X } from "lucide-react";
import { EDIT_MENU, FILE_MENU, VIEW_MENU, type AppMenuEntry } from "../appMenus";
import { commandRegistry, runCommand, shortcutLabel } from "../commandRegistry";
import { useAppStore } from "../store";
import { THEME_METRICS } from "../theme";
import { isMac } from "../platform";
import { BrandLogo } from "./BrandLogo";
import { IconButton } from "./kit";

const appWindow = getCurrentWindow();

export function TitleBar() {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    let disposed = false;
    const updateMaximizedState = () => {
      void appWindow.isMaximized().then((maximized) => {
        if (!disposed) setIsMaximized(maximized);
      });
    };
    updateMaximizedState();
    const unlisten = appWindow.onResized(updateMaximizedState);

    return () => {
      disposed = true;
      void unlisten.then((removeListener) => {
        removeListener();
      });
    };
  }, []);

  async function toggleMaximize() {
    await appWindow.toggleMaximize();
    setIsMaximized(await appWindow.isMaximized());
  }

  // macOS draws the traffic lights over this bar and hosts the menus in the system menu bar.
  if (isMac) {
    return (
      <header className="titlebar titlebar-mac" onMouseDown={startTitlebarDrag}>
        <div className="brand"><BrandLogo /></div>
        <HistoryButtons />
      </header>
    );
  }

  return (
    <header className="titlebar" onMouseDown={startTitlebarDrag}>
      <div className="brand"><BrandLogo /></div>
      <Menubar.Root className="menu-row">
        <Menu label="File" entries={[...FILE_MENU, { type: "separator" }, { type: "command", id: "file.settings" }]} />
        <Menu label="Edit" entries={EDIT_MENU} />
        <Menu label="View" entries={VIEW_MENU} />
      </Menubar.Root>
      <HistoryButtons />
      <div className="window-controls">
        <IconButton icon={Minus} label="Minimize" onClick={() => void appWindow.minimize()} />
        <IconButton icon={isMaximized ? Minimize2 : Maximize2} label={isMaximized ? "Restore" : "Maximize"} onClick={() => void toggleMaximize()} />
        <IconButton icon={X} label="Close" command="file.closeWindow" className="window-close" onClick={() => void appWindow.close()} />
      </div>
    </header>
  );
}

/** Undo and Redo, enabled while the GUI history has a step. */
function HistoryButtons() {
  const history = useAppStore((store) => store.snapshot?.history);
  return <div className="titlebar-history">
    <IconButton icon={Undo2} label="Undo" command="edit.undo" disabled={history?.canUndo !== true} onClick={() => { runCommand("edit.undo"); }} />
    <IconButton icon={Redo2} label="Redo" command="edit.redo" disabled={history?.canRedo !== true} onClick={() => { runCommand("edit.redo"); }} />
  </div>;
}

function startTitlebarDrag(event: React.MouseEvent<HTMLElement>) {
  if (event.button !== 0) return;
  if (event.target instanceof Element && event.target.closest("button, [role='menuitem'], [role='menu'], [data-radix-popper-content-wrapper]")) return;
  event.preventDefault();
  if (event.detail === 2) {
    void appWindow.toggleMaximize();
    return;
  }
  void appWindow.startDragging();
}

function Menu({ label, entries }: { label: string; entries: AppMenuEntry[] }) {
  return (
    <Menubar.Menu>
      <Menubar.Trigger className="menu-trigger">{label}</Menubar.Trigger>
      <Menubar.Portal>
        <MenuContent entries={entries} />
      </Menubar.Portal>
    </Menubar.Menu>
  );
}

/** Mounted only while its menu is open, so its subscriptions cost nothing when closed. */
function MenuContent({ entries }: { entries: AppMenuEntry[] }) {
  // Command state reads the store; re-render when it changes.
  useAppStore((store) => store.snapshot);
  useAppStore((store) => store.guiDocument);
  return (
    <Menubar.Content className="menu-content" sideOffset={THEME_METRICS.menuOffset}>
      {entries.map((entry, index) => {
        if (entry.type === "separator") return <Menubar.Separator key={`separator-${index}`} className="menu-separator" />;
        const command = commandRegistry[entry.id];
        const content = <>
          <span>{command.label}</span>
          <span className="shortcut">{shortcutLabel(entry.id)}</span>
        </>;
        if (command.checked !== undefined) {
          return (
            <Menubar.CheckboxItem
              key={entry.id}
              className="menu-item"
              checked={command.checked()}
              disabled={!command.enabled()}
              onCheckedChange={() => { runCommand(entry.id); }}
            >
              {content}
              <Menubar.ItemIndicator>
                <Check size={THEME_METRICS.iconSizeExtraSmall} />
              </Menubar.ItemIndicator>
            </Menubar.CheckboxItem>
          );
        }
        return (
          <Menubar.Item key={entry.id} className="menu-item" disabled={!command.enabled()} onSelect={() => { runCommand(entry.id); }}>
            {content}
          </Menubar.Item>
        );
      })}
    </Menubar.Content>
  );
}

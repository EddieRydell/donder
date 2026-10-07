import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { Check, Maximize2, Minimize2, Minus, X } from "lucide-react";
import { EDIT_MENU, FILE_MENU, VIEW_MENU, type AppMenuEntry } from "../appMenus";
import { commandRegistry, runCommand, shortcutLabel } from "../commandRegistry";
import { useAppStore } from "../store";
import { THEME_METRICS } from "../theme";
import { isMac } from "../platform";

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
        <div className="brand">Donder</div>
      </header>
    );
  }

  return (
    <header className="titlebar" onMouseDown={startTitlebarDrag}>
      <div className="brand">
        Donder
      </div>
      <nav className="menu-row">
        <Menu label="File" entries={[...FILE_MENU, { type: "separator" }, { type: "command", id: "file.settings" }]} />
        <Menu label="Edit" entries={EDIT_MENU} />
        <Menu label="View" entries={VIEW_MENU} />
      </nav>
      <div className="window-controls">
        <button onClick={() => void appWindow.minimize()} aria-label="Minimize">
        <Minus size={THEME_METRICS.iconSizeCompact} />
        </button>
        <button onClick={() => void toggleMaximize()} aria-label={isMaximized ? "Restore" : "Maximize"}>
          {isMaximized
            ? <Minimize2 size={THEME_METRICS.iconSizeSmall} />
            : <Maximize2 size={THEME_METRICS.iconSizeSmall} />}
        </button>
        <button className="close" onClick={() => void appWindow.close()} aria-label="Close">
          <X size={THEME_METRICS.iconSizeCompact} />
        </button>
      </div>
    </header>
  );
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
    <DropdownMenu.Root>
      <DropdownMenu.Trigger className="menu-trigger">{label}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <MenuContent entries={entries} />
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** Mounted only while its menu is open, so its subscriptions cost nothing when closed. */
function MenuContent({ entries }: { entries: AppMenuEntry[] }) {
  // Command state reads the store; re-render when it changes.
  useAppStore((store) => store.snapshot);
  useAppStore((store) => store.guiDocument);
  return (
    <DropdownMenu.Content className="menu-content" sideOffset={THEME_METRICS.menuOffset}>
      {entries.map((entry, index) => {
        if (entry.type === "separator") return <DropdownMenu.Separator key={`separator-${index}`} className="menu-separator" />;
        const command = commandRegistry[entry.id];
        const content = <>
          <span>{command.label}</span>
          <span className="shortcut">{shortcutLabel(entry.id)}</span>
        </>;
        if (command.checked !== undefined) {
          return (
            <DropdownMenu.CheckboxItem
              key={entry.id}
              className="menu-item"
              checked={command.checked()}
              disabled={!command.enabled()}
              onCheckedChange={() => { runCommand(entry.id); }}
            >
              {content}
              <DropdownMenu.ItemIndicator>
                <Check size={THEME_METRICS.iconSizeExtraSmall} />
              </DropdownMenu.ItemIndicator>
            </DropdownMenu.CheckboxItem>
          );
        }
        return (
          <DropdownMenu.Item key={entry.id} className="menu-item" disabled={!command.enabled()} onSelect={() => { runCommand(entry.id); }}>
            {content}
          </DropdownMenu.Item>
        );
      })}
    </DropdownMenu.Content>
  );
}

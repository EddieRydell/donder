import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { Check, Maximize2, Minimize2, Minus, X } from "lucide-react";
import { commandRegistry } from "../commandRegistry";
import { useAppStore } from "../store";
import { setGlobalMarkDisplayMode, useMarkDisplayMode, type MarkDisplayMode } from "./gui/sequence/marks";
import { requestOpenLayerGraph } from "./uiEvents";
import { THEME_METRICS } from "../theme";

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

  return (
    <header className="titlebar" onMouseDown={startTitlebarDrag}>
      <div className="brand">
        Donder
      </div>
      <nav className="menu-row">
        <Menu
          label="File"
          commands={[
            "file.newProject",
            "file.copyProject",
            "file.newSequence",
            "file.openProject",
            "file.save",
            "file.reloadFromDisk",
            "file.settings"
          ]}
        />
        <Menu label="Edit" commands={["edit.undo", "edit.redo"]} />
        <ViewMenu />
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

function ViewMenu() {
  const checked = useAppStore((store) => (store.snapshot?.settings.editorViewMode ?? "gui") === "gui");
  const showSequenceItems = useAppStore((store) => store.guiDocument?.type === "sequence");
  const [markMode] = useMarkDisplayMode();
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger className="menu-trigger">View</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="menu-content" sideOffset={THEME_METRICS.menuOffset}>
          <DropdownMenu.CheckboxItem
            checked={checked}
            className="menu-item"
            onCheckedChange={() => {
              void commandRegistry["view.toggleGuiMode"].run();
            }}
          >
            <span>{commandRegistry["view.toggleGuiMode"].label}</span>
            <span className="shortcut" />
          </DropdownMenu.CheckboxItem>
          {(["view.toggleProjectTree", "project.reload"] as const).map((id) => {
            const command = commandRegistry[id];
            return (
              <DropdownMenu.Item
                key={id}
                className="menu-item"
                onSelect={() => {
                  void command.run();
                }}
              >
                <span>{command.label}</span>
                <span className="shortcut">{command.shortcut}</span>
              </DropdownMenu.Item>
            );
          })}
          {showSequenceItems && (
            <>
              <DropdownMenu.Separator className="menu-separator" />
              <DropdownMenu.Item className="menu-item" onSelect={requestOpenLayerGraph}>
                <span>Layer Graph</span>
                <span className="shortcut" />
              </DropdownMenu.Item>
              <DropdownMenu.Separator className="menu-separator" />
              <DropdownMenu.Label className="menu-label">Mark display</DropdownMenu.Label>
              <DropdownMenu.RadioGroup
                value={markMode}
                onValueChange={(value) => {
                  setGlobalMarkDisplayMode(value as MarkDisplayMode);
                }}
              >
                <MarkDisplayItem value="overlay" label="Overlay" />
                <MarkDisplayItem value="strip" label="Strip" />
                <MarkDisplayItem value="hidden" label="Hidden" />
              </DropdownMenu.RadioGroup>
            </>
          )}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function MarkDisplayItem({ value, label }: { value: MarkDisplayMode; label: string }) {
  return (
    <DropdownMenu.RadioItem className="menu-item" value={value}>
      <span>{label}</span>
      <DropdownMenu.ItemIndicator>
        <Check size={THEME_METRICS.iconSizeExtraSmall} />
      </DropdownMenu.ItemIndicator>
    </DropdownMenu.RadioItem>
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

function Menu({ label, commands }: { label: string; commands: Array<keyof typeof commandRegistry> }) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger className="menu-trigger">{label}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="menu-content" sideOffset={THEME_METRICS.menuOffset}>
          {commands.map((id) => {
            const command = commandRegistry[id];
            return (
              <DropdownMenu.Item
                key={id}
                className="menu-item"
                onSelect={() => {
                  void command.run();
                }}
              >
                <span>{command.label}</span>
                <span className="shortcut">{command.shortcut}</span>
              </DropdownMenu.Item>
            );
          })}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

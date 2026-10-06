import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";
import { commandRegistry, runCommand, type CommandId } from "./commandRegistry";
import { handledEditShortcuts, isTextEditingTarget, shortcutAccelerator, type EditShortcut } from "./platform";
import { useAppStore } from "./store";
import { MARK_DISPLAY_MODE_EVENT, markDisplayModeValue, setGlobalMarkDisplayMode, type MarkDisplayMode } from "./ui/gui/sequence/marks";
import { requestOpenLayerGraph } from "./ui/uiEvents";
import { runWorkspaceTransition } from "./workspaceTransitions";

const separator = () => PredefinedMenuItem.new({ item: "Separator" });
const EDIT_MENU_INDEX = 2;

/** Delivers a menu shortcut to the focused element as the key event the editor canvases handle. */
function forwardShortcut(key: string) {
  (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent("keydown", { key, metaKey: true, bubbles: true, cancelable: true }));
}

/** Installs the macOS app menu. The menu receives shortcuts before the page, so it owns them and the in-page shortcut handler stays off. */
export async function installNativeMenu(): Promise<() => void> {
  const commandItems = new Map<CommandId, MenuItem>();
  const item = async (id: CommandId) => {
    const command = commandRegistry[id];
    const shortcut = command.shortcuts[0];
    const menuItem = await MenuItem.new({
      text: command.label,
      enabled: command.enabled(),
      ...(shortcut === undefined ? {} : { accelerator: shortcutAccelerator(shortcut) }),
      action: () => { runCommand(id); }
    });
    commandItems.set(id, menuItem);
    return menuItem;
  };

  const settings = () => useAppStore.getState().snapshot?.settings;
  const guiMode = await CheckMenuItem.new({
    text: commandRegistry["view.toggleGuiMode"].label,
    checked: (settings()?.editorViewMode ?? "gui") === "gui",
    action: () => { runCommand("view.toggleGuiMode"); }
  });
  const spectrogram = await CheckMenuItem.new({
    text: commandRegistry["view.toggleSpectrogram"].label,
    checked: settings()?.sequenceSpectrogramEnabled ?? false,
    action: () => { runCommand("view.toggleSpectrogram"); }
  });
  const layerGraph = await MenuItem.new({ text: "Layer Graph", action: requestOpenLayerGraph });
  const markModes: Array<[MarkDisplayMode, string]> = [["overlay", "Overlay"], ["strip", "Strip"], ["hidden", "Hidden"]];
  const markItems = await Promise.all(markModes.map(([mode, text]) => CheckMenuItem.new({
    text,
    checked: markDisplayModeValue() === mode,
    action: () => { setGlobalMarkDisplayMode(mode); }
  })));
  const markDisplay = await Submenu.new({ text: "Mark Display", items: markItems });

  // macOS gives menu shortcuts to the menu before the page. Text fields and the code editor need
  // the native Edit actions; elsewhere Donder's undo runs and the canvases receive their shortcuts.
  const textEdit = await Submenu.new({
    text: "Edit",
    items: await Promise.all((["Undo", "Redo", "Separator", "Cut", "Copy", "Paste", "SelectAll"] as const)
      .map((native) => PredefinedMenuItem.new({ item: native })))
  });
  const forwardedItems = new Map<EditShortcut, MenuItem>();
  const forwarded = async (shortcut: EditShortcut, text: string, key: string) => {
    const menuItem = await MenuItem.new({
      text,
      accelerator: `CmdOrCtrl+${key.toUpperCase()}`,
      enabled: false,
      action: () => { forwardShortcut(key); }
    });
    forwardedItems.set(shortcut, menuItem);
    return menuItem;
  };
  const canvasEdit = await Submenu.new({
    text: "Edit",
    items: [
      await item("edit.undo"),
      await item("edit.redo"),
      await separator(),
      await forwarded("cut", "Cut", "x"),
      await forwarded("copy", "Copy", "c"),
      await forwarded("paste", "Paste", "v"),
      await forwarded("selectAll", "Select All", "a")
    ]
  });

  const menu = await Menu.new({
    items: [
      await Submenu.new({
        text: "Donder",
        items: [
          await PredefinedMenuItem.new({ item: { About: null } }),
          await separator(),
          await item("file.settings"),
          await separator(),
          await PredefinedMenuItem.new({ item: "Services" }),
          await separator(),
          await PredefinedMenuItem.new({ item: "Hide" }),
          await PredefinedMenuItem.new({ item: "HideOthers" }),
          await PredefinedMenuItem.new({ item: "ShowAll" }),
          await separator(),
          await MenuItem.new({
            text: "Quit Donder",
            accelerator: "CmdOrCtrl+Q",
            action: () => { void runWorkspaceTransition({ type: "closeApplication" }); }
          })
        ]
      }),
      await Submenu.new({
        text: "File",
        items: [
          await item("file.newProject"),
          await item("file.newSequence"),
          await item("file.copyProject"),
          await separator(),
          await item("file.openProject"),
          await item("workbench.quickOpen"),
          await separator(),
          await item("file.save"),
          await item("file.reloadFromDisk"),
          await separator(),
          await PredefinedMenuItem.new({ item: "CloseWindow" })
        ]
      }),
      isTextEditingTarget(document.activeElement) ? textEdit : canvasEdit,
      await Submenu.new({
        text: "View",
        items: [
          await item("workbench.commandPalette"),
          await separator(),
          guiMode,
          await item("view.toggleProjectTree"),
          await item("view.focusExplorer"),
          await item("view.focusSearch"),
          await item("view.focusProblems"),
          await separator(),
          spectrogram,
          layerGraph,
          markDisplay,
          await separator(),
          await item("project.reload"),
          await separator(),
          await PredefinedMenuItem.new({ item: "Fullscreen" })
        ]
      }),
      await Submenu.new({
        text: "Window",
        items: [
          await PredefinedMenuItem.new({ item: "Minimize" }),
          await PredefinedMenuItem.new({ item: "Maximize" }),
          await separator(),
          await PredefinedMenuItem.new({ item: "BringAllToFront" })
        ]
      })
    ]
  });
  await menu.setAsAppMenu();

  // Menu state follows the snapshot: enabled commands and the view toggles.
  // Each update is an IPC call, so only changed values are sent.
  const sent = new Map<object, string>();
  const send = (target: object, key: string, value: boolean, apply: () => Promise<void>) => {
    const encoded = `${key}:${String(value)}`;
    if (sent.get(target) === encoded) return;
    sent.set(target, encoded);
    void apply();
  };
  const sync = () => {
    for (const [id, menuItem] of commandItems) {
      const enabled = commandRegistry[id].enabled();
      send(menuItem, "enabled", enabled, () => menuItem.setEnabled(enabled));
    }
    const state = useAppStore.getState();
    const sequenceOpen = state.guiDocument?.type === "sequence";
    const gui = (state.snapshot?.settings.editorViewMode ?? "gui") === "gui";
    const spectrogramOn = state.snapshot?.settings.sequenceSpectrogramEnabled ?? false;
    for (const [menuItem, id] of [[guiMode, "view.toggleGuiMode"], [spectrogram, "view.toggleSpectrogram"]] as const) {
      const enabled = commandRegistry[id].enabled();
      send(menuItem, "enabled", enabled, () => menuItem.setEnabled(enabled));
    }
    send(guiMode, "checked", gui, () => guiMode.setChecked(gui));
    send(spectrogram, "checked", spectrogramOn, () => spectrogram.setChecked(spectrogramOn));
    for (const menuItem of [layerGraph, markDisplay]) send(menuItem, "enabled", sequenceOpen, () => menuItem.setEnabled(sequenceOpen));
  };
  const syncMarks = () => {
    markItems.forEach((menuItem, index) => { void menuItem.setChecked(markModes[index]?.[0] === markDisplayModeValue()); });
  };
  sync();

  let textEditing = isTextEditingTarget(document.activeElement);
  let swap = Promise.resolve();
  const onFocusChange = () => {
    // Focus moves through <body> between elements; read the settled target.
    window.setTimeout(() => {
      // Only the focused canvas's own Edit shortcuts are live.
      const handled = handledEditShortcuts(document.activeElement);
      for (const [shortcut, menuItem] of forwardedItems) {
        const enabled = handled.includes(shortcut);
        send(menuItem, "enabled", enabled, () => menuItem.setEnabled(enabled));
      }
      const next = isTextEditingTarget(document.activeElement);
      if (next === textEditing) return;
      textEditing = next;
      swap = swap.then(async () => {
        await menu.removeAt(EDIT_MENU_INDEX);
        await menu.insert(next ? textEdit : canvasEdit, EDIT_MENU_INDEX);
      });
    }, 0);
  };
  document.addEventListener("focusin", onFocusChange);
  document.addEventListener("focusout", onFocusChange);
  const unsubscribe = useAppStore.subscribe((state, previous) => {
    if (state.snapshot !== previous.snapshot || state.guiDocument !== previous.guiDocument) sync();
  });
  window.addEventListener(MARK_DISPLAY_MODE_EVENT, syncMarks);
  return () => {
    unsubscribe();
    document.removeEventListener("focusin", onFocusChange);
    document.removeEventListener("focusout", onFocusChange);
    window.removeEventListener(MARK_DISPLAY_MODE_EVENT, syncMarks);
  };
}

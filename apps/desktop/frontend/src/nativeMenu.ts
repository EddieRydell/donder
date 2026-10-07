import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";
import { EDIT_MENU, FILE_MENU, VIEW_MENU, type AppMenuEntry } from "./appMenus";
import { commandRegistry, runCommand, type CommandId } from "./commandRegistry";
import { EDIT_SHORTCUT_KEYS, editTarget, handledEditShortcuts, shortcutAccelerator, type EditShortcut, type EditTarget } from "./platform";
import { useAppStore } from "./store";
import { runFocusedEditorAction } from "./ui/source/monaco";
import { runWorkspaceTransition } from "./workspaceTransitions";

const separator = () => PredefinedMenuItem.new({ item: "Separator" });
const EDIT_MENU_INDEX = 2;
const EDIT_LABELS: Record<EditShortcut, string> = { cut: "Cut", copy: "Copy", paste: "Paste", selectAll: "Select All" };

/** Delivers a menu shortcut to the focused element as the key event the editor canvases handle. */
function forwardShortcut(shortcut: EditShortcut) {
  (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent("keydown", { key: EDIT_SHORTCUT_KEYS[shortcut], metaKey: true, bubbles: true, cancelable: true }));
}

/** Installs the macOS app menu. The menu receives shortcuts before the page, so it owns them and the in-page shortcut handler stays off. */
export async function installNativeMenu(): Promise<() => void> {
  // Every item whose enabled or checked state follows the app, with how to read that state.
  const tracked: Array<{ item: MenuItem | CheckMenuItem; enabled: () => boolean; checked?: () => boolean }> = [];

  const command = async (id: CommandId) => {
    const definition = commandRegistry[id];
    const shortcut = definition.shortcuts[0];
    const options = {
      text: definition.label,
      enabled: definition.enabled(),
      ...(shortcut === undefined ? {} : { accelerator: shortcutAccelerator(shortcut) }),
      action: () => { runCommand(id); }
    };
    const { checked } = definition;
    const item = checked === undefined ? await MenuItem.new(options) : await CheckMenuItem.new({ ...options, checked: checked() });
    tracked.push({ item, enabled: definition.enabled, ...(checked === undefined ? {} : { checked }) });
    return item;
  };
  const entries = (menu: AppMenuEntry[]) => Promise.all(menu.map((entry) => {
    switch (entry.type) {
      case "command": return command(entry.id);
      case "separator": return separator();
    }
  }));

  // macOS gives menu shortcuts to the menu before the page, so the Edit menu follows focus:
  // text fields get the native actions, the code editor runs Monaco's own actions, and elsewhere
  // Donder's undo runs and the focused canvas receives the shortcuts it handles.
  const nativeClipboard = () => Promise.all((["Cut", "Copy", "Paste"] as const).map((item) => PredefinedMenuItem.new({ item })));
  const codeAction = (text: string, accelerator: string, action: Parameters<typeof runFocusedEditorAction>[0]) =>
    MenuItem.new({ text, accelerator, action: () => { runFocusedEditorAction(action); } });
  const forwardedItems = new Map<EditShortcut, MenuItem>();
  const forwarded = async (shortcut: EditShortcut) => {
    const item = await MenuItem.new({
      text: EDIT_LABELS[shortcut],
      accelerator: shortcutAccelerator({ key: EDIT_SHORTCUT_KEYS[shortcut] }),
      enabled: false,
      action: () => { forwardShortcut(shortcut); }
    });
    forwardedItems.set(shortcut, item);
    return item;
  };
  const editMenus: Record<EditTarget, Submenu> = {
    text: await Submenu.new({
      text: "Edit",
      items: await Promise.all((["Undo", "Redo", "Separator", "Cut", "Copy", "Paste", "SelectAll"] as const).map((item) => PredefinedMenuItem.new({ item })))
    }),
    code: await Submenu.new({
      text: "Edit",
      items: [
        await codeAction("Undo", shortcutAccelerator({ key: "z" }), "undo"),
        await codeAction("Redo", shortcutAccelerator({ key: "z", shift: true }), "redo"),
        await separator(),
        ...await nativeClipboard(),
        await codeAction(EDIT_LABELS.selectAll, shortcutAccelerator({ key: EDIT_SHORTCUT_KEYS.selectAll }), "editor.action.selectAll")
      ]
    }),
    app: await Submenu.new({
      text: "Edit",
      items: [
        ...await entries(EDIT_MENU),
        await separator(),
        ...await Promise.all((Object.keys(EDIT_SHORTCUT_KEYS) as EditShortcut[]).map(forwarded))
      ]
    })
  };
  let currentEdit = editTarget(document.activeElement);

  const menu = await Menu.new({
    items: [
      await Submenu.new({
        text: "Donder",
        items: [
          await PredefinedMenuItem.new({ item: { About: null } }),
          await separator(),
          await command("file.settings"),
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
      await Submenu.new({ text: "File", items: [...await entries(FILE_MENU), await command("file.closeWindow")] }),
      editMenus[currentEdit],
      await Submenu.new({ text: "View", items: [...await entries(VIEW_MENU), await separator(), await PredefinedMenuItem.new({ item: "Fullscreen" })] }),
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

  // Each update is an IPC call, so only changed values are sent.
  const sent = new Map<object, { enabled?: boolean; checked?: boolean }>();
  const send = (item: MenuItem | CheckMenuItem, enabled: boolean, checked?: boolean) => {
    const previous = sent.get(item) ?? {};
    sent.set(item, { enabled, ...(checked === undefined ? {} : { checked }) });
    if (previous.enabled !== enabled) void item.setEnabled(enabled);
    if (checked !== undefined && previous.checked !== checked && item instanceof CheckMenuItem) void item.setChecked(checked);
  };
  let swap = Promise.resolve();
  const sync = () => {
    for (const { item, enabled, checked } of tracked) send(item, enabled(), checked?.());
    // Only the focused canvas's own Edit shortcuts are live.
    const handled = handledEditShortcuts(document.activeElement);
    for (const [shortcut, item] of forwardedItems) send(item, handled.includes(shortcut));
    const nextEdit = editTarget(document.activeElement);
    if (nextEdit === currentEdit) return;
    currentEdit = nextEdit;
    swap = swap
      .then(() => menu.removeAt(EDIT_MENU_INDEX))
      .then(() => menu.insert(editMenus[nextEdit], EDIT_MENU_INDEX))
      .catch((error: unknown) => { useAppStore.getState().setError(`The Edit menu could not follow focus: ${String(error)}`); });
  };
  sync();

  // Focus moves through <body> between elements; read the settled target.
  const onFocusChange = () => { window.setTimeout(sync, 0); };
  document.addEventListener("focusin", onFocusChange);
  document.addEventListener("focusout", onFocusChange);
  const unsubscribe = useAppStore.subscribe((state, previous) => {
    if (state.snapshot !== previous.snapshot || state.guiDocument !== previous.guiDocument) sync();
  });
  return () => {
    unsubscribe();
    document.removeEventListener("focusin", onFocusChange);
    document.removeEventListener("focusout", onFocusChange);
  };
}

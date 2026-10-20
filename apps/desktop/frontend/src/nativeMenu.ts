import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";
import { EDIT_MENU, FILE_MENU, VIEW_MENU, type AppMenuEntry } from "./appMenus";
import { commandRegistry, commandShortcuts, runCommand } from "./commandRegistry";
import { editTarget, isMac, shortcut, shortcutAccelerator, type EditTarget } from "./platform";
import { useAppStore } from "./store";
import type { AppCommandId, CommandId, EditorCommandId, KeyBinding, Shortcut } from "./types";
import { runFocusedEditorAction } from "./ui/source/monaco";
import { runWorkspaceTransition } from "./workspaceTransitions";

const separator = () => PredefinedMenuItem.new({ item: "Separator" });
const EDIT_MENU_INDEX = 2;
const EDITOR_EDIT_ITEMS: EditorCommandId[] = ["edit.cut", "edit.copy", "edit.paste", "edit.selectAll"];
/** A menu item shows a command's first key; an unbound command has none. */
const accelerator = (id: CommandId) => {
  const first = commandShortcuts(id)[0];
  return first === undefined ? {} : { accelerator: shortcutAccelerator(first) };
};

const QUIT_SHORTCUT = shortcut("q", { primary: true });

/** Keys the macOS app menu keeps for its own items; a binding to one never reaches the page. */
export const RESERVED_MENU_SHORTCUTS: Array<{ label: string; shortcut: Shortcut }> = isMac
  ? [
    { label: "Quit Donder", shortcut: QUIT_SHORTCUT },
    { label: "Hide Donder", shortcut: shortcut("h", { primary: true }) },
    { label: "Hide Others", shortcut: shortcut("h", { primary: true, alt: true }) },
    { label: "Minimize", shortcut: shortcut("m", { primary: true }) }
  ]
  : [];

type AppMenu = {
  menu: Menu;
  editMenus: Record<EditTarget, Submenu>;
  // Every item whose enabled or checked state follows the app, with how to read that state.
  tracked: Array<{ item: MenuItem | CheckMenuItem; enabled: () => boolean; checked?: () => boolean }>;
};

async function buildMenu(currentEdit: EditTarget): Promise<AppMenu> {
  const tracked: AppMenu["tracked"] = [];
  const command = async (id: AppCommandId) => {
    const definition = commandRegistry[id];
    const options = {
      text: definition.label,
      enabled: definition.enabled(),
      ...accelerator(id),
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
  // Donder's undo and the active editor's clipboard commands run.
  const nativeClipboard = () => Promise.all((["Cut", "Copy", "Paste"] as const).map((item) => PredefinedMenuItem.new({ item })));
  const codeAction = (text: string, keys: { accelerator?: string }, action: Parameters<typeof runFocusedEditorAction>[0]) =>
    MenuItem.new({ text, ...keys, action: () => { runFocusedEditorAction(action); } });
  // Editor commands stay enabled: their state follows the surface's selection, which the menu does not track.
  const editorItem = (id: EditorCommandId) =>
    MenuItem.new({ text: commandRegistry[id].label, ...accelerator(id), action: () => { runCommand(id); } });
  const editMenus: Record<EditTarget, Submenu> = {
    text: await Submenu.new({
      text: "Edit",
      items: await Promise.all((["Undo", "Redo", "Separator", "Cut", "Copy", "Paste", "SelectAll"] as const).map((item) => PredefinedMenuItem.new({ item })))
    }),
    code: await Submenu.new({
      text: "Edit",
      items: [
        // Monaco keeps its own undo keys.
        await codeAction("Undo", { accelerator: shortcutAccelerator(shortcut("z", { primary: true })) }, "undo"),
        await codeAction("Redo", { accelerator: shortcutAccelerator(shortcut("z", { primary: true, shift: true })) }, "redo"),
        await separator(),
        ...await nativeClipboard(),
        await codeAction("Select All", accelerator("edit.selectAll"), "editor.action.selectAll")
      ]
    }),
    app: await Submenu.new({
      text: "Edit",
      items: [
        ...await entries(EDIT_MENU),
        await separator(),
        ...await Promise.all(EDITOR_EDIT_ITEMS.map(editorItem))
      ]
    })
  };

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
            accelerator: shortcutAccelerator(QUIT_SHORTCUT),
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
  return { menu, editMenus, tracked };
}

// Set while the app menu is installed: turns its shortcuts off, returning how to turn them back on.
let suspendShortcuts: (() => () => void) | null = null;

/**
 * Takes the macOS app menu's shortcuts away while a key is being recorded, so every key press
 * reaches the page; the returned function restores them. Elsewhere the page already gets every key.
 */
export function suspendMenuShortcuts(): () => void {
  return suspendShortcuts?.() ?? (() => undefined);
}

const keybindingsOf = (bindings: KeyBinding[] | undefined) => JSON.stringify(bindings ?? []);

/**
 * Installs the macOS app menu. The menu receives shortcuts before the page, so it owns them and
 * the in-page shortcut handler stays off. The menu is rebuilt when the key bindings change.
 */
export async function installNativeMenu(): Promise<() => void> {
  let currentEdit = editTarget(document.activeElement);
  let current = await buildMenu(currentEdit);
  await current.menu.setAsAppMenu();
  // A menu without shortcuts stands in while a key is recorded.
  const bare = await Menu.new({ items: [await Submenu.new({ text: "Donder", items: [] })] });
  let suspensions = 0;
  let builtKeybindings = keybindingsOf(useAppStore.getState().snapshot?.settings.keybindings);

  // Each update is an IPC call, so only changed values are sent.
  let sent = new Map<object, { enabled?: boolean; checked?: boolean }>();
  const send = (item: MenuItem | CheckMenuItem, enabled: boolean, checked?: boolean) => {
    const previous = sent.get(item) ?? {};
    sent.set(item, { enabled, ...(checked === undefined ? {} : { checked }) });
    if (previous.enabled !== enabled) void item.setEnabled(enabled);
    if (checked !== undefined && previous.checked !== checked && item instanceof CheckMenuItem) void item.setChecked(checked);
  };
  let pending = Promise.resolve();
  const reportMenuError = (error: unknown) => { useAppStore.getState().setError(`The app menu could not update: ${String(error)}`); };
  const sync = () => {
    const keybindings = keybindingsOf(useAppStore.getState().snapshot?.settings.keybindings);
    if (keybindings !== builtKeybindings) {
      builtKeybindings = keybindings;
      pending = pending
        .then(async () => {
          currentEdit = editTarget(document.activeElement);
          current = await buildMenu(currentEdit);
          if (suspensions === 0) await current.menu.setAsAppMenu();
          sent = new Map();
          sync();
        })
        .catch(reportMenuError);
      return;
    }
    for (const { item, enabled, checked } of current.tracked) send(item, enabled(), checked?.());
    const nextEdit = editTarget(document.activeElement);
    if (nextEdit === currentEdit) return;
    currentEdit = nextEdit;
    const { menu, editMenus } = current;
    pending = pending
      .then(() => menu.removeAt(EDIT_MENU_INDEX))
      .then(() => menu.insert(editMenus[nextEdit], EDIT_MENU_INDEX))
      .catch(reportMenuError);
  };
  sync();
  suspendShortcuts = () => {
    suspensions += 1;
    if (suspensions === 1) pending = pending.then(async () => { await bare.setAsAppMenu(); }).catch(reportMenuError);
    let resumed = false;
    return () => {
      if (resumed) return;
      resumed = true;
      suspensions -= 1;
      if (suspensions === 0) pending = pending.then(async () => { await current.menu.setAsAppMenu(); }).catch(reportMenuError);
    };
  };

  // Focus moves through <body> between elements; read the settled target.
  const onFocusChange = () => { window.setTimeout(sync, 0); };
  document.addEventListener("focusin", onFocusChange);
  document.addEventListener("focusout", onFocusChange);
  const unsubscribe = useAppStore.subscribe((state, previous) => {
    if (state.snapshot !== previous.snapshot || state.guiDocument !== previous.guiDocument) sync();
  });
  return () => {
    suspendShortcuts = null;
    unsubscribe();
    document.removeEventListener("focusin", onFocusChange);
    document.removeEventListener("focusout", onFocusChange);
  };
}

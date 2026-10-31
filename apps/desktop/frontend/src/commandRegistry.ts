import { getCurrentWindow } from "@tauri-apps/api/window";
import { commands } from "./api";
import { effectiveEditorViewMode } from "./editorViewMode";
import { openProjectDialog, runWorkspaceTransition, useTransitionStore } from "./workspaceTransitions";
import { navigateToText } from "./workspace/navigation";
import { runSnapshotCommand, useAppStore } from "./store";
import type { AppCommandId, CommandId, EditorCommandId, KeyBinding, Shortcut, SidebarView } from "./types";
import { editTarget, formatShortcut, isMac, matchesShortcut, shortcut } from "./platform";
import { activeEditorCommand, editorSurfaceRoot } from "./shortcuts";
import { requestOpenLayerGraph } from "./ui/uiEvents";
import { isPerforming, stopPerforming } from "./perform";

import { GUI_HISTORY_CHANGED_EVENT } from "./editor/host";

export const OPEN_COMMAND_PALETTE_EVENT = "donder:open-command-palette";
export const OPEN_QUICK_OPEN_EVENT = "donder:open-quick-open";
export const FOCUS_SIDEBAR_EVENT = "donder:focus-sidebar";

export type CommandDefinition = {
  label: string;
  category: "File" | "Edit" | "View" | "Transport" | "Sequence" | "Project" | "Perform" | "Workbench";
  keywords: string[];
  /** The keys the command has until a binding in the settings replaces them. */
  defaultShortcuts: Shortcut[];
  /** Shift is an argument, such as a larger step, so the command's keys answer with or without it. */
  shiftArgument?: boolean;
  enabled: () => boolean;
  /** Present on toggles: whether the setting is on. */
  checked?: () => boolean;
  run: () => Promise<void> | void;
};

const always = () => true;
const hasProject = () => useAppStore.getState().snapshot?.projectRoot !== null;
const sequenceOpen = () => useAppStore.getState().guiDocument?.type === "sequence";
const settings = () => useAppStore.getState().snapshot?.settings;
const focusSidebar = (view: SidebarView) => () => {
  window.dispatchEvent(new CustomEvent<SidebarView>(FOCUS_SIDEBAR_EVENT, { detail: view }));
};

const appCommands: Record<AppCommandId, CommandDefinition> = {
  "file.newProject": command("New Project...", "File", ["create"], () => {
    window.dispatchEvent(new CustomEvent("donder:new-project"));
  }),
  "file.copyProject": command("Create Standalone Project Copy...", "File", ["copy", "folder"], () => {
    window.dispatchEvent(new CustomEvent("donder:copy-project"));
  }, hasProject),
  "file.importProfile": command("Import Profile...", "File", ["xlights", "vixen", "layout", "controllers", "create"], () => {
    window.dispatchEvent(new CustomEvent("donder:import-profile"));
  }),
  "file.newSequence": command("New Sequence...", "File", ["create", "document"], () => {
    window.dispatchEvent(new CustomEvent("donder:new-sequence"));
  }, hasProject),
  "file.openProject": command("Open Project...", "File", ["folder", "workspace"], async () => {
    await openProjectDialog();
  }, always, [shortcut("o", { primary: true })]),
  "file.save": command("Save All", "File", ["write"], async () => {
    await runSnapshotCommand(commands.saveAll);
  }, hasProject, [shortcut("s", { primary: true })]),
  "file.reloadFromDisk": command("Reload From Disk", "File", ["revert"], async () => {
    const path = useAppStore.getState().snapshot?.activeFile;
    if (path !== null && path !== undefined) await runWorkspaceTransition({ type: "reloadFile", path });
    useAppStore.getState().resetGuiLocalState();
  }, hasProject),
  "file.closeEditor": command("Close Editor", "File", ["tab", "file"], async () => {
    const path = useAppStore.getState().snapshot?.activeFile;
    if (path !== null && path !== undefined) await runWorkspaceTransition({ type: "closeFile", path });
  }, () => (useAppStore.getState().snapshot?.activeFile ?? null) !== null, [shortcut("w", { primary: true })]),
  "file.closeWindow": command("Close Window", "File", ["quit", "exit"], async () => {
    await getCurrentWindow().close();
  }, always, [shortcut("w", { primary: true, shift: true })]),
  "file.settings": command("Settings...", "File", ["preferences"], () => {
    window.dispatchEvent(new CustomEvent("donder:settings"));
  }, always, [shortcut(",", { primary: true })]),
  "edit.undo": command("Undo", "Edit", ["history"], async () => {
    if (effectiveEditorViewMode(useAppStore.getState().snapshot) !== "gui") return;
    await runSnapshotCommand(commands.undoActiveEdit);
    window.dispatchEvent(new Event(GUI_HISTORY_CHANGED_EVENT));
  }, hasProject, [shortcut("z", { primary: true })]),
  "edit.redo": command("Redo", "Edit", ["history"], async () => {
    if (effectiveEditorViewMode(useAppStore.getState().snapshot) !== "gui") return;
    await runSnapshotCommand(commands.redoActiveEdit);
    window.dispatchEvent(new Event(GUI_HISTORY_CHANGED_EVENT));
  }, hasProject, isMac ? [shortcut("z", { primary: true, shift: true })] : [shortcut("z", { primary: true, shift: true }), shortcut("y", { primary: true })]),
  "view.toggleGuiMode": {
    ...command("Toggle GUI / Text Mode", "View", ["editor"], async () => {
      const mode = (settings()?.editorViewMode ?? "gui") === "gui" ? "text" : "gui";
      const snapshot = await runSnapshotCommand(() => commands.setEditorViewMode(mode));
      if (mode === "gui" && snapshot.projectHealth !== "ready" && snapshot.activeFile !== null) {
        const diagnostic = snapshot.diagnostics.find((item) => item.severity === "error");
        await navigateToText(snapshot.activeFile, diagnostic?.range ?? null);
        focusSidebar("problems")();
      }
    }, hasProject),
    checked: () => (settings()?.editorViewMode ?? "gui") === "gui"
  },
  "view.toggleSpectrogram": {
    ...command("Show Spectrogram", "View", ["audio", "waveform", "frequency"], async () => {
      const current = settings();
      if (current === undefined) return;
      await runSnapshotCommand(() => commands.updateAppSettings({
        ...current,
        sequenceSpectrogramEnabled: !(current.sequenceSpectrogramEnabled ?? false)
      }));
    }, hasProject),
    checked: () => settings()?.sequenceSpectrogramEnabled ?? false
  },
  "view.toggleMarksLaneOnly": {
    ...command("Show Marks Only in Marks Lane", "View", ["marks", "lines", "lanes"], async () => {
      const current = settings();
      if (current === undefined) return;
      await runSnapshotCommand(() => commands.updateAppSettings({
        ...current,
        sequenceMarksLaneOnly: !(current.sequenceMarksLaneOnly ?? false)
      }));
    }, hasProject),
    checked: () => settings()?.sequenceMarksLaneOnly ?? false
  },
  "view.openLayerGraph": command("Layer Graph", "View", ["composition", "operators"], requestOpenLayerGraph, sequenceOpen),
  "view.toggleProjectTree": command("Toggle Side Bar", "View", ["collapse", "panel"], async () => {
    await runSnapshotCommand(commands.toggleProjectTree);
  }, always, [shortcut("b", { primary: true })]),
  "view.focusExplorer": command("Focus Explorer", "View", ["files", "sidebar"], focusSidebar("explorer")),
  "view.focusSearch": command("Focus Search", "View", ["find", "sidebar"], focusSidebar("search")),
  "view.focusProblems": command("Focus Problems", "View", ["diagnostics", "errors", "sidebar"], focusSidebar("problems")),
  "workbench.quickOpen": command("Quick Open...", "Workbench", ["file", "recent"], () => {
    window.dispatchEvent(new CustomEvent(OPEN_QUICK_OPEN_EVENT));
  }, hasProject, [shortcut("p", { primary: true })]),
  "workbench.commandPalette": command("Command Palette...", "Workbench", ["commands"], () => {
    window.dispatchEvent(new CustomEvent(OPEN_COMMAND_PALETTE_EVENT));
  }, always, [shortcut("p", { primary: true, shift: true })]),
  "project.reload": command("Reload / Check Project", "Project", ["refresh", "diagnostics"], async () => {
    await runWorkspaceTransition({ type: "reloadProject" });
  }, hasProject, [shortcut("r", { primary: true })]),
  "perform.stop": command("Stop Performing", "Perform", ["console", "live", "keyboard"], stopPerforming, isPerforming),
};

/** Commands the topmost editor surface implements; they are enabled while it does. */
const editorCommands: Record<EditorCommandId, CommandDefinition> = {
  "edit.cut": editorCommand("edit.cut", "Cut", "Edit", ["clipboard"], [shortcut("x", { primary: true })]),
  "edit.copy": editorCommand("edit.copy", "Copy", "Edit", ["clipboard"], [shortcut("c", { primary: true })]),
  "edit.paste": editorCommand("edit.paste", "Paste", "Edit", ["clipboard"], [shortcut("v", { primary: true })]),
  "edit.selectAll": editorCommand("edit.selectAll", "Select All", "Edit", ["selection"], [shortcut("a", { primary: true })]),
  "edit.duplicate": editorCommand("edit.duplicate", "Duplicate", "Edit", ["copy", "clone"], [shortcut("d", { primary: true })]),
  "edit.delete": editorCommand("edit.delete", "Delete", "Edit", ["remove"], [shortcut("Delete"), shortcut("Backspace")]),
  "edit.confirm": editorCommand("edit.confirm", "Confirm", "Edit", ["accept", "apply"], [shortcut("Enter")]),
  "edit.cancel": editorCommand("edit.cancel", "Cancel", "Edit", ["escape", "dismiss"], [shortcut("Escape")]),
  "edit.moveLeft": { ...editorCommand("edit.moveLeft", "Move Left", "Edit", ["nudge", "arrow"], [shortcut("ArrowLeft")]), shiftArgument: true },
  "edit.moveRight": { ...editorCommand("edit.moveRight", "Move Right", "Edit", ["nudge", "arrow"], [shortcut("ArrowRight")]), shiftArgument: true },
  "edit.moveUp": { ...editorCommand("edit.moveUp", "Move Up", "Edit", ["nudge", "arrow"], [shortcut("ArrowUp")]), shiftArgument: true },
  "edit.moveDown": { ...editorCommand("edit.moveDown", "Move Down", "Edit", ["nudge", "arrow"], [shortcut("ArrowDown")]), shiftArgument: true },
  "view.reset": editorCommand("view.reset", "Reset View", "View", ["zoom", "fit"], [shortcut("Home")]),
  "transport.togglePlayback": editorCommand("transport.togglePlayback", "Play / Pause", "Transport", ["audio", "start"], [shortcut(" ")]),
  "transport.stop": editorCommand("transport.stop", "Stop", "Transport", ["audio", "playback"], [shortcut("s")]),
  "transport.toggleLooping": editorCommand("transport.toggleLooping", "Toggle Looping", "Transport", ["loop", "repeat", "range"], [shortcut("l")]),
  "transport.cycleFollowMode": editorCommand("transport.cycleFollowMode", "Cycle Follow Mode", "Transport", ["playhead", "scroll"], [shortcut("f")]),
  "transport.tapMark": editorCommand("transport.tapMark", "Tap Mark", "Transport", ["beat", "marks"], [shortcut("m")]),
  // Mac keyboards have no Home key; Command-Left Arrow is the platform's line-start gesture.
  "transport.rewind": editorCommand("transport.rewind", "Rewind to Start", "Transport", ["beginning", "home"], isMac ? [shortcut("Home"), shortcut("ArrowLeft", { primary: true })] : [shortcut("Home")]),
  "sequence.toggleSliceTool": editorCommand("sequence.toggleSliceTool", "Toggle Slice Tool", "Sequence", ["cut", "split", "razor"], [shortcut("c")]),
  "sequence.selectMatching": editorCommand("sequence.selectMatching", "Select Matching", "Sequence", ["similar", "selection"], [shortcut("s", { shift: true })])
};

export const commandRegistry: Record<CommandId, CommandDefinition> = { ...appCommands, ...editorCommands };

const APP_COMMAND_IDS = Object.keys(appCommands) as AppCommandId[];
// In key-matching order: where two commands share a key, the first the surface implements runs.
const EDITOR_COMMAND_IDS = Object.keys(editorCommands) as EditorCommandId[];

const bindingLookups = new WeakMap<KeyBinding[], Map<CommandId, Shortcut[]>>();

/** The command's keys in the settings' current bindings. */
export function commandShortcuts(id: CommandId): Shortcut[] {
  return boundShortcuts(useAppStore.getState().snapshot?.settings.keybindings ?? [], id);
}

/** The command's keys under `bindings`: its binding, else its defaults. */
export function boundShortcuts(bindings: KeyBinding[], id: CommandId): Shortcut[] {
  let lookup = bindingLookups.get(bindings);
  if (lookup === undefined) {
    lookup = new Map(bindings.map((binding) => [binding.command, binding.shortcuts]));
    bindingLookups.set(bindings, lookup);
  }
  return lookup.get(id) ?? commandRegistry[id].defaultShortcuts;
}

/** Runs a command unless a workspace transition is in progress or the command is disabled. */
export function runCommand(id: CommandId) {
  if (useTransitionStore.getState().inProgress) return;
  const command = commandRegistry[id];
  if (!command.enabled()) return;
  void command.run();
}

/** The command's shortcuts as the platform writes them, such as "⇧⌘Z" or "Ctrl+Shift+Z / Ctrl+Y". */
export function shortcutLabel(id: CommandId): string | undefined {
  const shortcuts = commandShortcuts(id);
  return shortcuts.length === 0 ? undefined : shortcuts.map(formatShortcut).join(" / ");
}

// Widgets with their own keyboard navigation keep their unmodified keys while focused.
const KEYBOARD_WIDGETS = "[role=\"dialog\"], [role=\"alertdialog\"], [role=\"menu\"], [role=\"menubar\"], [role=\"listbox\"], [role=\"tree\"]";

function widgetOwnsKey(target: EventTarget | null, root: HTMLElement | null, pressed: Shortcut): boolean {
  if (pressed.primary || !(target instanceof Element)) return false;
  const widget = target.closest(KEYBOARD_WIDGETS);
  // A dialog that holds the surface, such as the graph editor, is the surface.
  return widget !== null && (root === null || !widget.contains(root));
}

/**
 * Runs the topmost editor surface's command for a key press. Text entry and the code editor keep
 * their keys, as do keyboard widgets outside the surface and handlers that already took the key.
 */
function dispatchEditorShortcut(event: KeyboardEvent): boolean {
  const root = editorSurfaceRoot();
  if (root === undefined || event.defaultPrevented || editTarget(event.target) !== "app") return false;
  for (const id of EDITOR_COMMAND_IDS) {
    const pressed = commandShortcuts(id).find((candidate) => matchesShortcut(event, candidate, editorCommands[id].shiftArgument));
    if (pressed === undefined || widgetOwnsKey(event.target, root, pressed)) continue;
    const command = activeEditorCommand(id);
    if (command === undefined) continue;
    event.preventDefault();
    command.run({ shift: event.shiftKey, repeat: event.repeat });
    return true;
  }
  return false;
}

/**
 * The window's single key handler: editor surfaces first, then app commands. On macOS the native
 * menu receives app shortcuts before the page, so only editor keys arrive here.
 */
export function installKeyboardShortcuts() {
  const onKeyDown = (event: KeyboardEvent) => {
    if (dispatchEditorShortcut(event) || isMac) return;
    const id = APP_COMMAND_IDS.find((candidate) => commandShortcuts(candidate).some((pressed) => matchesShortcut(event, pressed)));
    if (id === undefined || !commandRegistry[id].enabled()) return;
    // Text fields and the code editor keep their own undo history.
    if ((id === "edit.undo" || id === "edit.redo") && editTarget(event.target) !== "app") return;
    event.preventDefault();
    runCommand(id);
  };
  window.addEventListener("keydown", onKeyDown);
  return () => { window.removeEventListener("keydown", onKeyDown); };
}

function command(
  label: string,
  category: CommandDefinition["category"],
  keywords: string[],
  run: () => Promise<void> | void,
  enabled: () => boolean = always,
  defaultShortcuts: Shortcut[] = []
): CommandDefinition {
  return { label, category, keywords, defaultShortcuts, enabled, run };
}

/** Menus and the palette run an editor command as its key does, without Shift. */
function editorCommand(id: EditorCommandId, label: string, category: CommandDefinition["category"], keywords: string[], defaultShortcuts: Shortcut[]): CommandDefinition {
  return command(label, category, keywords, () => { activeEditorCommand(id)?.run({ shift: false, repeat: false }); }, () => activeEditorCommand(id) !== undefined, defaultShortcuts);
}

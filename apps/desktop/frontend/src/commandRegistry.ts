import { getCurrentWindow } from "@tauri-apps/api/window";
import { commands } from "./api";
import { effectiveEditorViewMode } from "./editorViewMode";
import { openProjectDialog, runWorkspaceTransition, useTransitionStore } from "./workspaceTransitions";
import { navigateToText } from "./workspace/navigation";
import { runSnapshotCommand, useAppStore } from "./store";
import type { SidebarView } from "./types";
import { editTarget, formatShortcut, isMac, matchesShortcut, type Shortcut } from "./platform";
import { dispatchEditorShortcut } from "./shortcuts";
import { requestOpenLayerGraph } from "./ui/uiEvents";

import { GUI_HISTORY_CHANGED_EVENT } from "./editor/host";

export const OPEN_COMMAND_PALETTE_EVENT = "donder:open-command-palette";
export const OPEN_QUICK_OPEN_EVENT = "donder:open-quick-open";
export const FOCUS_SIDEBAR_EVENT = "donder:focus-sidebar";

export type CommandId =
  | "file.newProject"
  | "file.copyProject"
  | "file.newSequence"
  | "file.openProject"
  | "file.save"
  | "file.reloadFromDisk"
  | "file.closeEditor"
  | "file.closeWindow"
  | "file.settings"
  | "edit.undo"
  | "edit.redo"
  | "view.toggleGuiMode"
  | "view.toggleSpectrogram"
  | "view.toggleMarksLaneOnly"
  | "view.openLayerGraph"
  | "view.toggleProjectTree"
  | "view.focusExplorer"
  | "view.focusSearch"
  | "view.focusProblems"
  | "workbench.quickOpen"
  | "workbench.commandPalette"
  | "project.reload";

export type CommandDefinition = {
  label: string;
  category: "File" | "Edit" | "View" | "Project" | "Workbench";
  keywords: string[];
  shortcuts: Shortcut[];
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

export const commandRegistry: Record<CommandId, CommandDefinition> = {
  "file.newProject": command("New Project...", "File", ["create"], () => {
    window.dispatchEvent(new CustomEvent("donder:new-project"));
  }),
  "file.copyProject": command("Create Standalone Project Copy...", "File", ["copy", "folder"], () => {
    window.dispatchEvent(new CustomEvent("donder:copy-project"));
  }, hasProject),
  "file.newSequence": command("New Sequence...", "File", ["create", "document"], () => {
    window.dispatchEvent(new CustomEvent("donder:new-sequence"));
  }, hasProject),
  "file.openProject": command("Open Project...", "File", ["folder", "workspace"], async () => {
    await openProjectDialog();
  }, always, [{ key: "o", primary: true }]),
  "file.save": command("Save All", "File", ["write"], async () => {
    await runSnapshotCommand(commands.saveAll);
  }, hasProject, [{ key: "s", primary: true }]),
  "file.reloadFromDisk": command("Reload From Disk", "File", ["revert"], async () => {
    const path = useAppStore.getState().snapshot?.activeFile;
    if (path !== null && path !== undefined) await runWorkspaceTransition({ type: "reloadFile", path });
    useAppStore.getState().resetGuiLocalState();
  }, hasProject),
  "file.closeEditor": command("Close Editor", "File", ["tab", "file"], async () => {
    const path = useAppStore.getState().snapshot?.activeFile;
    if (path !== null && path !== undefined) await runWorkspaceTransition({ type: "closeFile", path });
  }, () => (useAppStore.getState().snapshot?.activeFile ?? null) !== null, [{ key: "w", primary: true }]),
  "file.closeWindow": command("Close Window", "File", ["quit", "exit"], async () => {
    await getCurrentWindow().close();
  }, always, [{ key: "w", primary: true, shift: true }]),
  "file.settings": command("Settings...", "File", ["preferences"], () => {
    window.dispatchEvent(new CustomEvent("donder:settings"));
  }, always, [{ key: ",", primary: true }]),
  "edit.undo": command("Undo", "Edit", ["history"], async () => {
    if (effectiveEditorViewMode(useAppStore.getState().snapshot) !== "gui") return;
    await runSnapshotCommand(commands.undoActiveEdit);
    window.dispatchEvent(new Event(GUI_HISTORY_CHANGED_EVENT));
  }, hasProject, [{ key: "z", primary: true }]),
  "edit.redo": command("Redo", "Edit", ["history"], async () => {
    if (effectiveEditorViewMode(useAppStore.getState().snapshot) !== "gui") return;
    await runSnapshotCommand(commands.redoActiveEdit);
    window.dispatchEvent(new Event(GUI_HISTORY_CHANGED_EVENT));
  }, hasProject, isMac ? [{ key: "z", primary: true, shift: true }] : [{ key: "z", primary: true, shift: true }, { key: "y", primary: true }]),
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
  }, always, [{ key: "b", primary: true }]),
  "view.focusExplorer": command("Focus Explorer", "View", ["files", "sidebar"], focusSidebar("explorer")),
  "view.focusSearch": command("Focus Search", "View", ["find", "sidebar"], focusSidebar("search")),
  "view.focusProblems": command("Focus Problems", "View", ["diagnostics", "errors", "sidebar"], focusSidebar("problems")),
  "workbench.quickOpen": command("Quick Open...", "Workbench", ["file", "recent"], () => {
    window.dispatchEvent(new CustomEvent(OPEN_QUICK_OPEN_EVENT));
  }, hasProject, [{ key: "p", primary: true }]),
  "workbench.commandPalette": command("Command Palette...", "Workbench", ["commands"], () => {
    window.dispatchEvent(new CustomEvent(OPEN_COMMAND_PALETTE_EVENT));
  }, always, [{ key: "p", primary: true, shift: true }]),
  "project.reload": command("Reload / Check Project", "Project", ["refresh", "diagnostics"], async () => {
    await runWorkspaceTransition({ type: "reloadProject" });
  }, hasProject, [{ key: "r", primary: true }]),
};

/** Runs a command unless a workspace transition is in progress or the command is disabled. */
export function runCommand(id: CommandId) {
  if (useTransitionStore.getState().inProgress) return;
  const command = commandRegistry[id];
  if (!command.enabled()) return;
  void command.run();
}

/** The command's shortcuts as the platform writes them, such as "⇧⌘Z" or "Ctrl+Shift+Z / Ctrl+Y". */
export function shortcutLabel(id: CommandId): string | undefined {
  const { shortcuts } = commandRegistry[id];
  return shortcuts.length === 0 ? undefined : shortcuts.map(formatShortcut).join(" / ");
}

/**
 * The window's single key handler: editor surfaces first, then app commands. On macOS the native
 * menu receives app shortcuts before the page, so only editor keys arrive here.
 */
export function installKeyboardShortcuts() {
  const onKeyDown = (event: KeyboardEvent) => {
    if (dispatchEditorShortcut(event) || isMac) return;
    const id = (Object.keys(commandRegistry) as CommandId[]).find((candidate) => commandRegistry[candidate].shortcuts.some((shortcut) => matchesShortcut(event, shortcut)));
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
  shortcuts: Shortcut[] = []
): CommandDefinition {
  return { label, category, keywords, shortcuts, enabled, run };
}

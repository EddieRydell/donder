import { commands } from "./api";
import { effectiveEditorViewMode } from "./editorViewMode";
import { openProjectDialog, runWorkspaceTransition, useTransitionStore } from "./workspaceTransitions";
import { navigateToText } from "./workspace/navigation";
import { runSnapshotCommand, useAppStore } from "./store";
import type { SidebarView } from "./types";

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
  | "file.settings"
  | "edit.undo"
  | "edit.redo"
  | "view.toggleGuiMode"
  | "view.toggleSpectrogram"
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
  shortcut?: string;
  enabled: () => boolean;
  run: () => Promise<void> | void;
};

const always = () => true;
const hasProject = () => useAppStore.getState().snapshot?.projectRoot !== null;
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
  }, always, "Ctrl+O"),
  "file.save": command("Save All", "File", ["write"], async () => {
    await runSnapshotCommand(commands.saveAll);
  }, hasProject, "Ctrl+S"),
  "file.reloadFromDisk": command("Reload From Disk", "File", ["revert"], async () => {
    const path = useAppStore.getState().snapshot?.activeFile;
    if (path !== null && path !== undefined) await runWorkspaceTransition({ type: "reloadFile", path });
    useAppStore.getState().resetGuiLocalState();
  }, hasProject),
  "file.settings": command("Settings...", "File", ["preferences"], () => {
    window.dispatchEvent(new CustomEvent("donder:settings"));
  }),
  "edit.undo": command("Undo", "Edit", ["history"], async () => {
    if (effectiveEditorViewMode(useAppStore.getState().snapshot) !== "gui") return;
    await runSnapshotCommand(commands.undoActiveEdit);
    window.dispatchEvent(new Event(GUI_HISTORY_CHANGED_EVENT));
  }, hasProject, "Ctrl+Z"),
  "edit.redo": command("Redo", "Edit", ["history"], async () => {
    if (effectiveEditorViewMode(useAppStore.getState().snapshot) !== "gui") return;
    await runSnapshotCommand(commands.redoActiveEdit);
    window.dispatchEvent(new Event(GUI_HISTORY_CHANGED_EVENT));
  }, hasProject, "Ctrl+Shift+Z / Ctrl+Y"),
  "view.toggleGuiMode": command("Toggle GUI / Text Mode", "View", ["editor"], async () => {
    const mode = (useAppStore.getState().snapshot?.settings.editorViewMode ?? "gui") === "gui" ? "text" : "gui";
    const snapshot = await runSnapshotCommand(() => commands.setEditorViewMode(mode));
    if (mode === "gui" && snapshot.projectHealth !== "ready" && snapshot.activeFile !== null) {
      const diagnostic = snapshot.diagnostics.find((item) => item.severity === "error");
      await navigateToText(snapshot.activeFile, diagnostic?.range ?? null);
      focusSidebar("problems")();
    }
  }, hasProject),
  "view.toggleSpectrogram": command("Show Spectrogram", "View", ["audio", "waveform", "frequency"], async () => {
    const settings = useAppStore.getState().snapshot?.settings;
    if (settings === undefined) return;
    await runSnapshotCommand(() => commands.updateAppSettings({
      ...settings,
      sequenceSpectrogramEnabled: !(settings.sequenceSpectrogramEnabled ?? false)
    }));
  }, hasProject),
  "view.toggleProjectTree": command("Toggle Side Bar", "View", ["collapse", "panel"], async () => {
    await runSnapshotCommand(commands.toggleProjectTree);
  }, always, "Ctrl+B"),
  "view.focusExplorer": command("Focus Explorer", "View", ["files", "sidebar"], focusSidebar("explorer")),
  "view.focusSearch": command("Focus Search", "View", ["find", "sidebar"], focusSidebar("search")),
  "view.focusProblems": command("Focus Problems", "View", ["diagnostics", "errors", "sidebar"], focusSidebar("problems")),
  "workbench.quickOpen": command("Quick Open...", "Workbench", ["file", "recent"], () => {
    window.dispatchEvent(new CustomEvent(OPEN_QUICK_OPEN_EVENT));
  }, hasProject, "Ctrl+P"),
  "workbench.commandPalette": command("Command Palette...", "Workbench", ["commands"], () => {
    window.dispatchEvent(new CustomEvent(OPEN_COMMAND_PALETTE_EVENT));
  }, always, "Ctrl+Shift+P"),
  "project.reload": command("Reload / Check Project", "Project", ["refresh", "diagnostics"], async () => {
    await runWorkspaceTransition({ type: "reloadProject" });
  }, hasProject, "Ctrl+R"),
};

export function installGlobalShortcuts() {
  const onKeyDown = (event: KeyboardEvent) => {
    if (useTransitionStore.getState().inProgress) return;
    const ctrl = event.ctrlKey || event.metaKey;
    if (!ctrl) return;
    const key = event.key.toLowerCase();
    let id: CommandId | null = null;
    if (key === "p") id = event.shiftKey ? "workbench.commandPalette" : "workbench.quickOpen";
    else if (key === "z") id = event.shiftKey ? "edit.redo" : "edit.undo";
    else if (key === "y") id = "edit.redo";
    else if (key === "o") id = "file.openProject";
    else if (key === "s") id = "file.save";
    else if (key === "b") id = "view.toggleProjectTree";
    else if (key === "r") id = "project.reload";
    if (id === null || !commandRegistry[id].enabled()) return;
    event.preventDefault();
    void commandRegistry[id].run();
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
  shortcut?: string
): CommandDefinition {
  return shortcut === undefined
    ? { label, category, keywords, enabled, run }
    : { label, category, keywords, shortcut, enabled, run };
}

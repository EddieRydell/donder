import type { CommandId } from "./commandRegistry";

/**
 * The app menus, rendered as the in-window menu bar on Windows and Linux and as the system menu
 * bar on macOS. Each renderer adds only its platform's own items, such as Settings and Quit.
 */
export type AppMenuEntry = { type: "command"; id: CommandId } | { type: "separator" };

const item = (id: CommandId): AppMenuEntry => ({ type: "command", id });
const separator: AppMenuEntry = { type: "separator" };

export const FILE_MENU: AppMenuEntry[] = [
  item("file.newProject"),
  item("file.newSequence"),
  item("file.copyProject"),
  separator,
  item("file.openProject"),
  item("workbench.quickOpen"),
  separator,
  item("file.save"),
  item("file.reloadFromDisk"),
  separator,
  item("file.closeEditor")
];

export const EDIT_MENU: AppMenuEntry[] = [item("edit.undo"), item("edit.redo")];

export const VIEW_MENU: AppMenuEntry[] = [
  item("workbench.commandPalette"),
  separator,
  item("view.toggleGuiMode"),
  item("view.toggleProjectTree"),
  item("view.focusExplorer"),
  item("view.focusSearch"),
  item("view.focusProblems"),
  separator,
  item("view.toggleSpectrogram"),
  item("view.toggleMarksLaneOnly"),
  item("view.openLayerGraph"),
  separator,
  item("project.reload")
];

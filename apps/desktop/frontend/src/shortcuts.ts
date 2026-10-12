import { useLayoutEffect, useRef, type RefObject } from "react";
import { editTarget, isMac, matchesShortcut, type Shortcut } from "./platform";

/** Commands an editor surface implements; the keys for each are in `EDITOR_SHORTCUTS`. */
export type EditorCommandId =
  | "edit.cut"
  | "edit.copy"
  | "edit.paste"
  | "edit.selectAll"
  | "edit.duplicate"
  | "edit.delete"
  | "edit.confirm"
  | "edit.cancel"
  | "edit.moveLeft"
  | "edit.moveRight"
  | "edit.moveUp"
  | "edit.moveDown"
  | "view.reset"
  | "transport.togglePlayback"
  | "transport.stop"
  | "transport.toggleLooping"
  | "transport.cycleFollowMode"
  | "transport.tapMark"
  | "transport.rewind"
  | "sequence.toggleSliceTool";

/** The keymap of every editor surface. Surfaces implement commands, never keys. */
export const EDITOR_SHORTCUTS: Record<EditorCommandId, Shortcut[]> = {
  "edit.cut": [{ key: "x", primary: true }],
  "edit.copy": [{ key: "c", primary: true }],
  "edit.paste": [{ key: "v", primary: true }],
  "edit.selectAll": [{ key: "a", primary: true }],
  "edit.duplicate": [{ key: "d", primary: true }],
  "edit.delete": [{ key: "Delete" }, { key: "Backspace" }],
  "edit.confirm": [{ key: "Enter" }],
  "edit.cancel": [{ key: "Escape" }],
  "edit.moveLeft": [{ key: "ArrowLeft", shift: "either" }],
  "edit.moveRight": [{ key: "ArrowRight", shift: "either" }],
  "edit.moveUp": [{ key: "ArrowUp", shift: "either" }],
  "edit.moveDown": [{ key: "ArrowDown", shift: "either" }],
  "view.reset": [{ key: "Home" }],
  "transport.togglePlayback": [{ key: " " }],
  "transport.stop": [{ key: "s" }],
  "transport.toggleLooping": [{ key: "l" }],
  "transport.cycleFollowMode": [{ key: "f" }],
  "transport.tapMark": [{ key: "m" }],
  // Mac keyboards have no Home key; Command-Left Arrow is the platform's line-start gesture.
  "transport.rewind": isMac ? [{ key: "Home" }, { key: "ArrowLeft", primary: true }] : [{ key: "Home" }],
  "sequence.toggleSliceTool": [{ key: "c" }]
};

const EDITOR_COMMAND_IDS = Object.keys(EDITOR_SHORTCUTS) as EditorCommandId[];

export type ShortcutInput = { shift: boolean; repeat: boolean };
/** A command is skipped, and its key left alone, while it is not enabled. */
export type EditorCommand = { run: (input: ShortcutInput) => void; enabled?: () => boolean };
export type EditorCommands = Partial<Record<EditorCommandId, EditorCommand>>;

type Layer = { root: RefObject<HTMLElement | null>; commands: RefObject<EditorCommands> };

// Mounted editor surfaces in mount order. The last one covers the others, as the graph editor
// covers its sequence and an inline fixture editor covers its parent.
const layers: Layer[] = [];

/**
 * Makes an editor surface's commands answer its keys, whatever element has focus, while it is
 * the topmost mounted surface. `root` is the surface's element.
 */
export function useEditorCommands(root: RefObject<HTMLElement | null>, commands: EditorCommands) {
  const current = useRef(commands);
  useLayoutEffect(() => { current.current = commands; });
  useLayoutEffect(() => {
    const layer = { root, commands: current };
    layers.push(layer);
    return () => { layers.splice(layers.indexOf(layer), 1); };
  }, [root]);
}

function activeCommand(id: EditorCommandId): EditorCommand | undefined {
  const command = layers[layers.length - 1]?.commands.current[id];
  return command !== undefined && (command.enabled?.() ?? true) ? command : undefined;
}

/** Runs the topmost surface's command, as a menu item does. */
export function runEditorCommand(id: EditorCommandId) {
  activeCommand(id)?.run({ shift: false, repeat: false });
}

// Widgets with their own keyboard navigation keep their unmodified keys while focused.
const KEYBOARD_WIDGETS = "[role=\"dialog\"], [role=\"alertdialog\"], [role=\"menu\"], [role=\"menubar\"], [role=\"listbox\"], [role=\"tree\"]";

function widgetOwnsKey(target: EventTarget | null, root: HTMLElement | null, shortcut: Shortcut): boolean {
  if (shortcut.primary === true || !(target instanceof Element)) return false;
  const widget = target.closest(KEYBOARD_WIDGETS);
  // A dialog that holds the surface, such as the graph editor, is the surface.
  return widget !== null && (root === null || !widget.contains(root));
}

/**
 * Runs the topmost editor surface's command for a key press. Text entry and the code editor keep
 * their keys, as do keyboard widgets outside the surface and handlers that already took the key.
 */
export function dispatchEditorShortcut(event: KeyboardEvent): boolean {
  const layer = layers[layers.length - 1];
  if (layer === undefined || event.defaultPrevented || editTarget(event.target) !== "app") return false;
  for (const id of EDITOR_COMMAND_IDS) {
    const shortcut = EDITOR_SHORTCUTS[id].find((candidate) => matchesShortcut(event, candidate));
    if (shortcut === undefined || widgetOwnsKey(event.target, layer.root.current, shortcut)) continue;
    const command = activeCommand(id);
    if (command === undefined) continue;
    event.preventDefault();
    command.run({ shift: event.shiftKey, repeat: event.repeat });
    return true;
  }
  return false;
}

/** Sends key presses inside `scope` to the editor surfaces, for an editor embedded in another page. */
export function installEditorShortcuts(scope: HTMLElement) {
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.target instanceof Node && scope.contains(event.target)) dispatchEditorShortcut(event);
  };
  window.addEventListener("keydown", onKeyDown);
  return () => { window.removeEventListener("keydown", onKeyDown); };
}

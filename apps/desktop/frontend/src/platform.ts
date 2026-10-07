import { focusedCodeEditor } from "./ui/source/monaco";

export const isMac = navigator.userAgent.includes("Mac");

export type Shortcut = { key: string; shift?: boolean };

type ModifierState = { ctrlKey: boolean; metaKey: boolean };

/** The platform's command modifier: Command on macOS, Control elsewhere. */
export function hasPrimaryModifier(event: ModifierState): boolean {
  return isMac ? event.metaKey : event.ctrlKey;
}

const TEXT_INPUT_TYPES = new Set(["text", "search", "email", "url", "tel", "password", "number"]);

/**
 * What owns the Edit shortcuts (undo, redo, clipboard, select all) for an element: native text
 * editing in text fields, Monaco's own actions in the code editor, or Donder's commands elsewhere.
 */
export type EditTarget = "text" | "code" | "app";

export function editTarget(target: EventTarget | null): EditTarget {
  if (focusedCodeEditor() !== undefined) return "code";
  if (target instanceof HTMLInputElement) return TEXT_INPUT_TYPES.has(target.type) ? "text" : "app";
  if (target instanceof HTMLTextAreaElement || (target instanceof HTMLElement && target.isContentEditable)) return "text";
  return "app";
}

export type EditShortcut = "cut" | "copy" | "paste" | "selectAll";

export const EDIT_SHORTCUT_KEYS: Record<EditShortcut, string> = { cut: "x", copy: "c", paste: "v", selectAll: "a" };

/** Declares the Edit shortcuts an element's keydown handler implements, so the macOS menu can enable them. */
export function editShortcutTarget(shortcuts: EditShortcut[]) {
  return { "data-edit-shortcuts": shortcuts.join(" ") };
}

export function handledEditShortcuts(target: Element | null): EditShortcut[] {
  const declared = target?.closest("[data-edit-shortcuts]")?.getAttribute("data-edit-shortcuts") ?? "";
  return (Object.keys(EDIT_SHORTCUT_KEYS) as EditShortcut[]).filter((shortcut) => declared.split(" ").includes(shortcut));
}

/** Control-click is a secondary click on macOS, so it must not act as a modified primary click. */
export function isSecondaryClick(event: { button: number; ctrlKey: boolean }): boolean {
  return event.button === 2 || (isMac && event.button === 0 && event.ctrlKey);
}

export function formatShortcut(shortcut: Shortcut): string {
  const key = shortcut.key.length === 1 ? shortcut.key.toUpperCase() : shortcut.key;
  if (isMac) return `${shortcut.shift === true ? "⇧" : ""}⌘${key}`;
  return `Ctrl+${shortcut.shift === true ? "Shift+" : ""}${key}`;
}

/** Tauri menu accelerator for a shortcut. */
export function shortcutAccelerator(shortcut: Shortcut): string {
  const key = shortcut.key.length === 1 ? shortcut.key.toUpperCase() : shortcut.key;
  return `CmdOrCtrl+${shortcut.shift === true ? "Shift+" : ""}${key}`;
}

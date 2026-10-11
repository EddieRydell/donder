import { focusedCodeEditor } from "./ui/source/monaco";

export const isMac = navigator.userAgent.includes("Mac");

/** A key with the modifiers it needs. `primary` is Command on macOS and Control elsewhere; Shift is required, forbidden, or either. */
export type Shortcut = { key: string; primary?: boolean; shift?: boolean | "either" };

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

export function matchesShortcut(event: KeyboardEvent, shortcut: Shortcut): boolean {
  const key = shortcut.key.length === 1 ? event.key.toLowerCase() : event.key;
  const otherModifier = isMac ? event.ctrlKey : event.metaKey;
  return key === shortcut.key
    && hasPrimaryModifier(event) === (shortcut.primary === true)
    && !otherModifier && !event.altKey
    && (shortcut.shift === "either" || event.shiftKey === (shortcut.shift === true));
}

/** Control-click is a secondary click on macOS, so it must not act as a modified primary click. */
export function isSecondaryClick(event: { button: number; ctrlKey: boolean }): boolean {
  return event.button === 2 || (isMac && event.button === 0 && event.ctrlKey);
}

function shortcutKeyName(key: string): string {
  return key === " " ? "Space" : key.length === 1 ? key.toUpperCase() : key;
}

export function formatShortcut(shortcut: Shortcut): string {
  const key = shortcutKeyName(shortcut.key);
  const shift = shortcut.shift === true;
  if (isMac) return `${shift ? "⇧" : ""}${shortcut.primary === true ? "⌘" : ""}${key}`;
  return `${shortcut.primary === true ? "Ctrl+" : ""}${shift ? "Shift+" : ""}${key}`;
}

/** Tauri menu accelerator for a shortcut. */
export function shortcutAccelerator(shortcut: Shortcut): string {
  return `${shortcut.primary === true ? "CmdOrCtrl+" : ""}${shortcut.shift === true ? "Shift+" : ""}${shortcutKeyName(shortcut.key)}`;
}

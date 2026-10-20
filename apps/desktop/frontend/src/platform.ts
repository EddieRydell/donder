import type { Shortcut } from "./types";
import { focusedCodeEditor } from "./ui/source/monaco";

export const isMac = navigator.userAgent.includes("Mac");

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

/** A shortcut from a key and the modifiers it needs. */
export function shortcut(key: string, modifiers: { primary?: boolean; shift?: boolean; alt?: boolean } = {}): Shortcut {
  return { key, primary: modifiers.primary === true, shift: modifiers.shift === true, alt: modifiers.alt === true };
}

const PHYSICAL_LETTER = /^Key([A-Z])$/;
const PHYSICAL_DIGIT = /^Digit(\d)$/;

/**
 * The key a shortcut names for a key press. Option on macOS and Shift on the digit row change the
 * character typed, so those chords name the physical key instead.
 */
function shortcutKey(event: KeyboardEvent): string {
  const letter = PHYSICAL_LETTER.exec(event.code)?.[1];
  const digit = PHYSICAL_DIGIT.exec(event.code)?.[1];
  if (event.altKey && letter !== undefined) return letter.toLowerCase();
  if ((event.altKey || event.shiftKey) && digit !== undefined) return digit;
  return event.key.length === 1 ? event.key.toLowerCase() : event.key;
}

/** Whether a key press is the shortcut; `shiftFree` matches with or without Shift for commands that take it as an argument. */
export function matchesShortcut(event: KeyboardEvent, target: Shortcut, shiftFree = false): boolean {
  const otherModifier = isMac ? event.ctrlKey : event.metaKey;
  return shortcutKey(event) === target.key
    && hasPrimaryModifier(event) === target.primary
    && !otherModifier
    && event.altKey === target.alt
    && (shiftFree || event.shiftKey === target.shift);
}

const MODIFIER_KEYS = new Set(["Shift", "Control", "Alt", "Meta", "AltGraph", "CapsLock", "Fn"]);

/** The shortcut a key press makes, or null while only modifiers are down or the platform's other modifier is held. */
export function shortcutFromEvent(event: KeyboardEvent): Shortcut | null {
  if (MODIFIER_KEYS.has(event.key) || (isMac ? event.ctrlKey : event.metaKey)) return null;
  return shortcut(shortcutKey(event), { primary: hasPrimaryModifier(event), shift: event.shiftKey, alt: event.altKey });
}

export function sameShortcut(left: Shortcut, right: Shortcut): boolean {
  return left.key === right.key && left.primary === right.primary && left.shift === right.shift && left.alt === right.alt;
}

/** Control-click is a secondary click on macOS, so it must not act as a modified primary click. */
export function isSecondaryClick(event: { button: number; ctrlKey: boolean }): boolean {
  return event.button === 2 || (isMac && event.button === 0 && event.ctrlKey);
}

function shortcutKeyName(key: string): string {
  return key === " " ? "Space" : key.length === 1 ? key.toUpperCase() : key;
}

export function formatShortcut(target: Shortcut): string {
  const key = shortcutKeyName(target.key);
  if (isMac) return `${target.alt ? "⌥" : ""}${target.shift ? "⇧" : ""}${target.primary ? "⌘" : ""}${key}`;
  return `${target.primary ? "Ctrl+" : ""}${target.alt ? "Alt+" : ""}${target.shift ? "Shift+" : ""}${key}`;
}

/** Tauri menu accelerator for a shortcut. */
export function shortcutAccelerator(target: Shortcut): string {
  return `${target.primary ? "CmdOrCtrl+" : ""}${target.alt ? "Alt+" : ""}${target.shift ? "Shift+" : ""}${shortcutKeyName(target.key)}`;
}

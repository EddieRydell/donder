export const isMac = navigator.userAgent.includes("Mac");

export type Shortcut = { key: string; shift?: boolean };

type ModifierState = { ctrlKey: boolean; metaKey: boolean };

/** The platform's command modifier: Command on macOS, Control elsewhere. */
export function hasPrimaryModifier(event: ModifierState): boolean {
  return isMac ? event.metaKey : event.ctrlKey;
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

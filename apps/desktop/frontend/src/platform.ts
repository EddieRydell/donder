export const isMac = navigator.userAgent.includes("Mac");

export type Shortcut = { key: string; shift?: boolean };

type ModifierState = { ctrlKey: boolean; metaKey: boolean };

/** The platform's command modifier: Command on macOS, Control elsewhere. */
export function hasPrimaryModifier(event: ModifierState): boolean {
  return isMac ? event.metaKey : event.ctrlKey;
}

const TEXT_INPUT_TYPES = new Set(["text", "search", "email", "url", "tel", "password", "number"]);

/** Text fields and the code editor, where native text editing owns the Edit shortcuts. */
export function isTextEditingTarget(target: EventTarget | null): boolean {
  if (target instanceof HTMLInputElement) return TEXT_INPUT_TYPES.has(target.type);
  if (target instanceof HTMLTextAreaElement) return true;
  return target instanceof HTMLElement && (target.isContentEditable || target.closest(".cm-editor") !== null);
}

export type EditShortcut = "cut" | "copy" | "paste" | "selectAll";

/** Declares the Edit shortcuts an element's keydown handler implements, so the macOS menu can enable them. */
export function editShortcutTarget(shortcuts: EditShortcut[]) {
  return { "data-edit-shortcuts": shortcuts.join(" ") };
}

export function handledEditShortcuts(target: Element | null): EditShortcut[] {
  const declared = target?.closest("[data-edit-shortcuts]")?.getAttribute("data-edit-shortcuts") ?? "";
  return (["cut", "copy", "paste", "selectAll"] as const).filter((shortcut) => declared.split(" ").includes(shortcut));
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

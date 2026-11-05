import { AlertTriangle, Plus, RotateCcw, X } from "lucide-react";
import { useEffect, useEffectEvent, useState } from "react";
import { boundShortcuts, commandRegistry, type CommandDefinition } from "../commandRegistry";
import { RESERVED_MENU_SHORTCUTS, suspendMenuShortcuts } from "../nativeMenu";
import { formatShortcut, sameShortcut, shortcut, shortcutFromEvent } from "../platform";
import { THEME_METRICS } from "../theme";
import type { CommandId, KeyBinding, Shortcut } from "../types";
import { IconButton, Section, TextField, Tip } from "./kit";

const COMMAND_IDS = Object.keys(commandRegistry) as CommandId[];

/** Two keys collide when a press would match both; Shift is ignored for a command that takes it as an argument. */
function collide(left: Shortcut, right: Shortcut, shiftFree: boolean): boolean {
  return left.key === right.key && left.primary === right.primary && left.alt === right.alt && (shiftFree || left.shift === right.shift);
}

function sameShortcuts(left: Shortcut[], right: Shortcut[]): boolean {
  return left.length === right.length && left.every((entry, index) => {
    const other = right[index];
    return other !== undefined && sameShortcut(entry, other);
  });
}

function matchesQuery(definition: CommandDefinition, shortcuts: Shortcut[], query: string): boolean {
  if (query === "") return true;
  const text = [definition.label, definition.category, ...definition.keywords, ...shortcuts.map(formatShortcut)].join(" ").toLowerCase();
  return query.split(/\s+/).every((word) => text.includes(word));
}

/** Every command's keys, which a person can add to, remove, or reset to the defaults. */
export function KeybindingsSettings({ keybindings, onChange }: { keybindings: KeyBinding[]; onChange: (keybindings: KeyBinding[]) => void }) {
  const [query, setQuery] = useState("");
  const [recording, setRecording] = useState<CommandId | null>(null);

  const setShortcuts = (id: CommandId, shortcuts: Shortcut[]) => {
    const others = keybindings.filter((binding) => binding.command !== id);
    onChange(sameShortcuts(shortcuts, commandRegistry[id].defaultShortcuts) ? others : [...others, { command: id, shortcuts }]);
  };

  // While recording, the next key press is the new shortcut and reaches nothing else, the app
  // menu included; Escape cancels.
  const record = useEffectEvent((event: KeyboardEvent) => {
    if (recording === null) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    const pressed = shortcutFromEvent(event);
    if (pressed === null) return;
    setRecording(null);
    if (sameShortcut(pressed, shortcut("Escape"))) return;
    const chosen = commandRegistry[recording].shiftArgument === true ? { ...pressed, shift: false } : pressed;
    const current = boundShortcuts(keybindings, recording);
    if (!current.some((existing) => sameShortcut(existing, chosen))) setShortcuts(recording, [...current, chosen]);
  });
  useEffect(() => {
    if (recording === null) return;
    const onKeyDown = (event: KeyboardEvent) => { record(event); };
    const resumeMenuShortcuts = suspendMenuShortcuts();
    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () => {
      window.removeEventListener("keydown", onKeyDown, { capture: true });
      resumeMenuShortcuts();
    };
  }, [recording]);

  const words = query.trim().toLowerCase();
  const rows = COMMAND_IDS
    .map((id) => ({ id, definition: commandRegistry[id], shortcuts: boundShortcuts(keybindings, id) }))
    .filter((row) => matchesQuery(row.definition, row.shortcuts, words));
  /** Labels of the other commands, and the macOS menu items, a press of one of these keys would also match. */
  const sharing = (id: CommandId, shortcuts: Shortcut[]) => {
    const shiftFree = commandRegistry[id].shiftArgument === true;
    const commands = COMMAND_IDS.filter((other) => other !== id && boundShortcuts(keybindings, other).some((theirs) =>
      shortcuts.some((mine) => collide(mine, theirs, shiftFree || commandRegistry[other].shiftArgument === true))
    ));
    const reserved = RESERVED_MENU_SHORTCUTS.filter((item) => shortcuts.some((mine) => collide(mine, item.shortcut, shiftFree)));
    return { commands: commands.map((other) => commandRegistry[other].label), reserved: reserved.map((item) => item.label) };
  };

  return <Section
    title="Shortcuts"
    actions={<IconButton icon={RotateCcw} label="Reset all shortcuts" disabled={keybindings.length === 0} onClick={() => { onChange([]); }} />}
  >
    <TextField live name="Search commands or keys" placeholder="Search" value={query} onCommit={setQuery} />
    <div className="keybinding-list">
      {rows.map(({ id, definition, shortcuts }) => {
        const shared = sharing(id, shortcuts);
        const conflicts = [
          ...(shared.commands.length > 0 ? [`Also used by ${shared.commands.join(", ")}`] : []),
          ...(shared.reserved.length > 0 ? [`Used by macOS: ${shared.reserved.join(", ")}`] : [])
        ];
        const overridden = keybindings.some((binding) => binding.command === id);
        return <div key={id} className="keybinding-row">
          <span className="keybinding-label">
            <small>{definition.category}</small>
            {definition.label}
          </span>
          <span className="keybinding-keys">
            {conflicts.length > 0 && <Tip label={conflicts.join("; ")}>
              <span className="keybinding-shared" role="img" aria-label={conflicts.join("; ")}><AlertTriangle size={THEME_METRICS.iconSizeExtraSmall} aria-hidden /></span>
            </Tip>}
            {shortcuts.map((bound) => (
              <span key={formatShortcut(bound)} className="keybinding-key">
                <kbd>{formatShortcut(bound)}</kbd>
                <IconButton icon={X} label={`Remove ${formatShortcut(bound)}`} onClick={() => { setShortcuts(id, shortcuts.filter((other) => !sameShortcut(other, bound))); }} />
              </span>
            ))}
            {recording === id
              ? <span className="keybinding-recording">Press keys…</span>
              : <IconButton icon={Plus} label="Add shortcut (Escape cancels)" onClick={() => { setRecording(id); }} />}
            {overridden && <IconButton icon={RotateCcw} label="Reset to default" onClick={() => { onChange(keybindings.filter((binding) => binding.command !== id)); }} />}
          </span>
        </div>;
      })}
    </div>
  </Section>;
}

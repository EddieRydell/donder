import { useLayoutEffect, useRef, type RefObject } from "react";
import type { EditorCommandId } from "./types";

export type ShortcutInput = { shift: boolean; repeat: boolean };
/** A command is skipped, and its key left alone, while it is not enabled. */
export type EditorCommand = { run: (input: ShortcutInput) => void; enabled?: () => boolean };
/** The commands an editor surface implements; their keys are in the command registry. */
export type EditorCommands = Partial<Record<EditorCommandId, EditorCommand>>;

type Layer = { root: RefObject<HTMLElement | null>; commands: RefObject<EditorCommands> };

// Mounted editor surfaces in mount order. The last one covers the others, as the graph editor
// covers its sequence and an inline fixture editor covers its parent.
const layers: Layer[] = [];

/**
 * Makes an editor surface's commands answer their keys, whatever element has focus, while it is
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

/** The topmost surface's element, or undefined while no editor surface is mounted. */
export function editorSurfaceRoot(): HTMLElement | null | undefined {
  return layers[layers.length - 1]?.root.current;
}

/** The topmost surface's command, while it is enabled. */
export function activeEditorCommand(id: EditorCommandId): EditorCommand | undefined {
  const command = layers[layers.length - 1]?.commands.current[id];
  return command !== undefined && (command.enabled?.() ?? true) ? command : undefined;
}

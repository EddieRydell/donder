// Effect and operator pickers as nested folders: each script file is a
// submenu of its definitions, inside submenus for its folders.
import { useMemo } from "react";
import type { SequenceEffectDefinition, SequenceEffectReference } from "../../../types";
import { MenuSelect, type MenuEntry } from "../../kit";

/** A folder or script file, by its title-cased name. */
export type DefinitionFolder<T> = {
  key: string;
  name: string;
  folders: DefinitionFolder<T>[];
  /** Definitions in declaration order; only files have them. */
  items: T[];
  file: boolean;
};

const SCRIPT_EXTENSION = ".donder";

/** `impact-burst` → `Impact Burst`. */
function title(segment: string): string {
  return segment
    .split(/[-_\s]+/)
    .filter((word) => word !== "")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

function sorted<T>(folders: DefinitionFolder<T>[]): DefinitionFolder<T>[] {
  return folders
    .sort((left, right) => Number(left.file) - Number(right.file) || left.name.localeCompare(right.name))
    .map((folder) => ({ ...folder, folders: sorted(folder.folders) }));
}

/** The folder tree of definitions by their scripts' paths, without the folders every path shares. */
export function definitionTree<T>(items: readonly T[], path: (item: T) => string): DefinitionFolder<T>[] {
  const directories = items.map((item) => path(item).split("/").slice(0, -1));
  const first = directories[0] ?? [];
  let shared = 0;
  while (shared < first.length && directories.every((segments) => segments[shared] === first[shared])) shared += 1;
  const root: DefinitionFolder<T>[] = [];
  for (const item of items) {
    const segments = path(item).split("/");
    let level = root;
    let key = "";
    let folder: DefinitionFolder<T> | null = null;
    segments.slice(shared).forEach((segment, index, rest) => {
      const file = index === rest.length - 1;
      key = `${key}/${segment}`;
      const name = title(file && segment.endsWith(SCRIPT_EXTENSION) ? segment.slice(0, -SCRIPT_EXTENSION.length) : segment);
      let next = level.find((candidate) => candidate.key === key);
      if (next === undefined) {
        next = { key, name, folders: [], items: [], file };
        level.push(next);
      }
      folder = next;
      level = next.folders;
    });
    (folder as DefinitionFolder<T> | null)?.items.push(item);
  }
  return sorted(root);
}

export function effectReferencesEqual(left: SequenceEffectReference, right: SequenceEffectReference) {
  return left.path === right.path
    && left.effectName === right.effectName;
}

/** The folders as nested submenu entries; an empty tree is one disabled `empty` line. */
export function definitionEntries<T>(tree: DefinitionFolder<T>[], label: (item: T) => string, onSelect: (item: T) => void, empty: string): MenuEntry[] {
  if (tree.length === 0) return [{ type: "action", label: empty, disabled: true, onSelect: () => undefined }];
  const folder = (entry: DefinitionFolder<T>): MenuEntry => ({
    type: "submenu",
    label: entry.name,
    entries: [
      ...entry.folders.map(folder),
      ...entry.items.map((item): MenuEntry => ({ type: "action", label: label(item), onSelect: () => { onSelect(item); } }))
    ]
  });
  return tree.map(folder);
}

/** A dropdown of the project's effects in their script folders. */
export function EffectDefinitionPicker({ definitions, label, onSelect, disabled = false }: {
  definitions: SequenceEffectDefinition[];
  label: string;
  onSelect: (definition: SequenceEffectDefinition) => void;
  disabled?: boolean;
}) {
  const tree = useMemo(() => definitionTree(definitions, (definition) => definition.effect.path), [definitions]);
  return <MenuSelect label={label} name="Effect" disabled={disabled || definitions.length === 0}
    entries={definitionEntries(tree, (definition) => definition.name, onSelect, "No effects")} />;
}

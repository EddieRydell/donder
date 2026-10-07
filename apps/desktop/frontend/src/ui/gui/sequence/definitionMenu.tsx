// Effect and operator pickers as nested folders: each script file is a
// submenu of its definitions, inside submenus for its folders.
import type * as ContextMenu from "@radix-ui/react-context-menu";
import type * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { ChevronRight } from "lucide-react";
import { useContext } from "react";
import { THEME_METRICS } from "../../../theme";
import { OverlayPortal } from "../../OverlayPortal";

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

type MenuParts = typeof ContextMenu | typeof DropdownMenu;

export type DefinitionMenuItemsProps<T> = {
  /** The Radix menu the items belong to. */
  menu: MenuParts;
  tree: DefinitionFolder<T>[];
  label: (item: T) => string;
  itemKey: (item: T) => string;
  onSelect: (item: T) => void;
  empty: string;
  disabled?: boolean;
};

/** The folders as nested submenus of the menu they are placed in. */
export function DefinitionMenuItems<T>({ menu, tree, label, itemKey, onSelect, empty, disabled = false }: DefinitionMenuItemsProps<T>) {
  const container = useContext(OverlayPortal);
  const { Sub, SubTrigger, SubContent, Portal, Item } = menu;
  const folder = (entry: DefinitionFolder<T>) => (
    <Sub key={entry.key}>
      <SubTrigger className="menu-item" disabled={disabled}>
        {entry.name} <ChevronRight size={THEME_METRICS.iconSizeSmall} aria-hidden />
      </SubTrigger>
      <Portal container={container}>
        <SubContent className="menu-content">
          {entry.folders.map(folder)}
          {entry.items.map((item) => (
            <Item key={itemKey(item)} className="menu-item" disabled={disabled} onSelect={() => { onSelect(item); }}>
              {label(item)}
            </Item>
          ))}
        </SubContent>
      </Portal>
    </Sub>
  );
  if (tree.length === 0) {
    return <Item className="menu-item" disabled>{empty}</Item>;
  }
  return <>{tree.map(folder)}</>;
}

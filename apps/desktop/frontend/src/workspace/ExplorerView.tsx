import { runWorkspaceTransition } from "../workspaceTransitions";
import {
  ChevronDown,
  ChevronRight,
  File,
  FilePlus2,
  AudioLines,
  Blend,
  Boxes,
  Cable,
  ChartSpline,
  CircuitBoard,
  Folder,
  FolderOpen,
  FolderPlus,
  Keyboard,
  LayoutGrid,
  Lightbulb,
  Lock,
  Pencil,
  Play,
  RefreshCw,
  SlidersHorizontal,
  Sparkles,
  SquareStack,
  Trash2,
  Workflow
} from "lucide-react";
import {
  useMemo,
  useRef,
  useState,
  type CSSProperties
} from "react";
import type { NodeApi, TreeApi } from "react-arborist";
import { Tree } from "react-arborist";
import { useResizeObserver } from "use-resize-observer";
import { commands } from "../api";
import { runSnapshotCommand } from "../store";
import { THEME_METRICS } from "../theme";
import type {
  WorkspaceEntryRole,
  WorkspaceExplorerState,
  WorkspaceOperation
} from "../types";
import type { AppStaticSnapshot } from "../store";
import { buildSemanticTree, type WorkspaceTreeNode } from "./helpers";
import { startPerforming, usePerformStore } from "../perform";
import { useConfirmDialog, type Confirm } from "../ui/ConfirmDialog";
import { Badge, ContextMenuArea, IconButton, TextField, type MenuEntry } from "../ui/kit";

type InlineEdit =
  | { mode: "createFile" | "createDirectory"; parent: string; value: string; error: string | null }
  | { mode: "rename"; path: string; value: string; error: string | null };

export function ExplorerView({
  snapshot,
  onRequestPathChange
}: {
  snapshot: AppStaticSnapshot;
  onRequestPathChange: (source: string, destination: string) => Promise<void>;
}) {
  const data = useMemo(
    () => buildSemanticTree(snapshot.projectEntries, snapshot.diagnostics, snapshot.projectRoot),
    [snapshot.diagnostics, snapshot.projectEntries, snapshot.projectRoot]
  );
  const treeRef = useRef<TreeApi<WorkspaceTreeNode> | null>(null);
  const { ref: treeHostRef, width = 1, height = 1 } = useResizeObserver<HTMLDivElement>();
  const [inlineEdit, setInlineEdit] = useState<InlineEdit | null>(null);
  const { confirm, dialog: confirmDialog } = useConfirmDialog();
  const explorer = snapshot.workspaceExplorer;

  const saveExplorer = (next: WorkspaceExplorerState) => {
    void runSnapshotCommand(() => commands.saveViewState({ type: "workspaceExplorer", state: next }));
  };
  const beginCreate = (parent: string, directory: boolean) => {
    setInlineEdit({
      mode: directory ? "createDirectory" : "createFile",
      parent,
      value: "",
      error: null
    });
  };
  const commitInlineEdit = async (typed: string) => {
    if (inlineEdit === null) return;
    const value = typed.trim();
    const validation = validateName(value);
    if (validation !== null) {
      setInlineEdit({ ...inlineEdit, value: typed, error: validation });
      return;
    }
    try {
      if (inlineEdit.mode === "createFile") {
        await runSnapshotCommand(() => commands.createFile(inlineEdit.parent, value));
      } else if (inlineEdit.mode === "createDirectory") {
        await runSnapshotCommand(() => commands.createDirectory(inlineEdit.parent, value));
      } else {
        const rename = inlineEdit as Extract<InlineEdit, { mode: "rename" }>;
        const parent = rename.path.includes("/")
          ? rename.path.slice(0, rename.path.lastIndexOf("/"))
          : "";
        const destination = parent === "" ? value : `${parent}/${value}`;
        await onRequestPathChange(rename.path, destination);
      }
      setInlineEdit(null);
    } catch (error) {
      setInlineEdit({ ...inlineEdit, value: typed, error: String(error) });
    }
  };
  const reload = () => void runWorkspaceTransition({ type: "reloadProject" });
  const rootEntries: MenuEntry[] = [
    { type: "action", label: "New file", icon: FilePlus2, onSelect: () => { beginCreate("", false); } },
    { type: "action", label: "New folder", icon: FolderPlus, onSelect: () => { beginCreate("", true); } },
    { type: "separator" },
    { type: "action", label: "Collapse all", icon: SquareStack, onSelect: () => treeRef.current?.closeAll() },
    { type: "action", label: "Refresh", icon: RefreshCw, shortcut: "project.reload", onSelect: reload }
  ];

  return (
    <section className="sidebar-view explorer-view" aria-label="Explorer">
      <header className="sidebar-view-header">
        <h2>Explorer</h2>
        <div className="sidebar-toolbar">
          <IconButton icon={FilePlus2} label="New file" onClick={() => { beginCreate("", false); }} />
          <IconButton icon={FolderPlus} label="New folder" onClick={() => { beginCreate("", true); }} />
          <IconButton icon={SquareStack} label="Collapse all" onClick={() => treeRef.current?.closeAll()} />
          <IconButton icon={RefreshCw} label="Refresh" command="project.reload" onClick={reload} />
        </div>
      </header>
      <div className="explorer-tree-content">
        {inlineEdit !== null && inlineEdit.mode !== "rename" && (
          <InlineInput edit={inlineEdit} onSubmit={commitInlineEdit} onCancel={() => { setInlineEdit(null); }} />
        )}
        <ContextMenuArea entries={rootEntries}><div className="explorer-tree-host" ref={treeHostRef}>
          <Tree
            ref={treeRef}
            data={data}
            idAccessor={(node) => node.path}
            width={width}
            height={height}
            indent={THEME_METRICS.projectTreeIndent}
            rowHeight={THEME_METRICS.projectTreeRowHeight}
            openByDefault={explorer.expandedPaths.length === 0}
            initialOpenState={Object.fromEntries(explorer.expandedPaths.map((path) => [path, true]))}
            // react-arborist memoizes on the prop values, so the prop set must stay fixed; it deselects on "".
            selection={snapshot.activeFile ?? ""}
            onToggle={(id) => {
              const expanded = new Set(explorer.expandedPaths);
              if (expanded.has(id)) expanded.delete(id);
              else expanded.add(id);
              saveExplorer({ ...explorer, expandedPaths: [...expanded].sort() });
            }}
            onActivate={(node) => {
              if (node.data.kind === "file") void runSnapshotCommand(() => commands.openFile(node.data.path));
            }}
            disableDrag={(node) => !hasOperation(node, "move")}
            disableDrop={({ parentNode }) =>
              !parentNode.isRoot && !hasOperation(parentNode.data, "create")
            }
            onMove={async ({ dragIds, parentId }) => {
              const source = dragIds[0];
              if (source === undefined) return;
              const parts = source.split("/");
              const name = parts[parts.length - 1];
              if (name === undefined) return;
              const destination = parentId === null ? name : `${parentId}/${name}`;
              await onRequestPathChange(source, destination);
            }}
          >
            {(props) => (
              <TreeRow
                {...props}
                editing={inlineEdit}
                onCommit={commitInlineEdit}
                onCancel={() => { setInlineEdit(null); }}
                onBeginCreate={beginCreate}
                onBeginRename={(node) => { setInlineEdit({ mode: "rename", path: node.path, value: node.name, error: null }); }}
                onDelete={(node) => { void deletePath(node, confirm); }}
              />
            )}
          </Tree>
        </div></ContextMenuArea>
      </div>
      {confirmDialog}
    </section>
  );
}

function TreeRow({
  node,
  style,
  dragHandle,
  editing,
  onCommit,
  onCancel,
  onBeginCreate,
  onBeginRename,
  onDelete
}: {
  node: NodeApi<WorkspaceTreeNode>;
  style: CSSProperties;
  dragHandle?: (element: HTMLDivElement | null) => void;
  editing: InlineEdit | null;
  onCommit: (value: string) => Promise<void>;
  onCancel: () => void;
  onBeginCreate: (parent: string, directory: boolean) => void;
  onBeginRename: (node: WorkspaceTreeNode) => void;
  onDelete: (node: WorkspaceTreeNode) => void;
}) {
  const isRenaming = editing?.mode === "rename" && editing.path === node.data.path;
  const consoles = usePerformStore((store) => store.perform?.consoles);
  const performable = node.data.kind === "file" ? (consoles ?? []).filter((performed) => performed.path === node.data.path) : [];
  const entries: MenuEntry[] = [
    ...performable.map((performed): MenuEntry => ({ type: "action", label: `Perform ${performed.name}`, icon: Play, onSelect: () => { startPerforming(performed.index); } })),
    ...(performable.length > 0 ? [{ type: "separator" } as const] : []),
    ...(node.data.kind === "directory" && hasOperation(node.data, "create") ? [
      { type: "action", label: "New file", icon: FilePlus2, onSelect: () => { onBeginCreate(node.data.path, false); } },
      { type: "action", label: "New folder", icon: FolderPlus, onSelect: () => { onBeginCreate(node.data.path, true); } }
    ] satisfies MenuEntry[] : []),
    { type: "action", label: "Rename", icon: Pencil, disabled: !hasOperation(node.data, "rename"), onSelect: () => { onBeginRename(node.data); } },
    { type: "action", label: "Delete", icon: Trash2, danger: true, disabled: !hasOperation(node.data, "delete"), onSelect: () => { onDelete(node.data); } }
  ];
  return (
    <ContextMenuArea entries={entries}>
      <div
        ref={dragHandle}
        style={style}
        className={`tree-row role-${node.data.role} ${node.isSelected ? "selected" : ""}`}
        onClick={() => {
          if (node.data.kind === "directory") node.toggle();
        }}
      >
        <span className="tree-row-chevron">
          {node.data.kind === "directory" && (node.data.children?.length ?? 0) > 0
            ? node.isOpen
              ? <ChevronDown size={THEME_METRICS.iconSizeExtraSmall} />
              : <ChevronRight size={THEME_METRICS.iconSizeExtraSmall} />
            : null}
        </span>
        {roleIcon(node.data.role, node.data.kind === "directory" && node.isOpen)}
        {isRenaming ? (
          <InlineInput edit={editing} onSubmit={onCommit} onCancel={onCancel} compact />
        ) : (
          <span className="tree-row-name">{node.data.name}</span>
        )}
        {!hasOperation(node.data, "rename") && node.data.operationExplanation !== null && <Badge icon={Lock} tip={node.data.operationExplanation} />}
        {node.data.errorCount > 0 && <span className="diagnostic-badge error">{node.data.errorCount}</span>}
        {node.data.warningCount > 0 && <span className="diagnostic-badge warning">{node.data.warningCount}</span>}
      </div>
    </ContextMenuArea>
  );
}

function InlineInput({
  edit,
  onSubmit,
  onCancel,
  compact = false
}: {
  edit: InlineEdit;
  onSubmit: (value: string) => Promise<void>;
  onCancel: () => void;
  compact?: boolean;
}) {
  return (
    <div className={`inline-tree-edit ${compact ? "compact" : ""}`}>
      <TextField
        autoFocus
        value={edit.value}
        name={edit.mode === "rename" ? "New name" : "Name"}
        invalid={edit.error !== null}
        onCommit={(value) => void onSubmit(value)}
        onCancel={onCancel}
      />
      {edit.error !== null && <span className="inline-edit-error">{edit.error}</span>}
    </div>
  );
}

/** Deleting from disk cannot be undone, so it asks first. */
async function deletePath(node: WorkspaceTreeNode, confirm: Confirm) {
  const confirmed = await confirm({
    title: `Delete ${node.name}`,
    description: node.kind === "directory" ? "Deletes the folder and its contents from disk." : "Deletes the file from disk.",
    confirmLabel: "Delete",
    danger: true
  });
  if (confirmed) await runSnapshotCommand(() => commands.deletePath(node.path));
}

function hasOperation(node: WorkspaceTreeNode, operation: WorkspaceOperation): boolean {
  return node.operations.includes(operation);
}

function validateName(value: string): string | null {
  if (value === "") return "Name is required.";
  if (value === "." || value === ".." || value.includes("/") || value.includes("\\")) {
    return "Use a single file or directory name.";
  }
  return null;
}

function roleIcon(role: WorkspaceEntryRole, open: boolean) {
  const size = THEME_METRICS.iconSizeCompact;
  switch (role) {
    case "directory": return open ? <FolderOpen size={size} /> : <Folder size={size} />;
    case "asset": return <AudioLines size={size} />;
    case "project":
    case "entrypoint": return <Workflow size={size} />;
    case "setup": return <Boxes size={size} />;
    case "controller": return <CircuitBoard size={size} />;
    case "inputDevice": return <Keyboard size={size} />;
    case "layout": return <LayoutGrid size={size} />;
    case "fixture": return <Lightbulb size={size} />;
    case "patch": return <Cable size={size} />;
    case "curve": return <ChartSpline size={size} />;
    case "gradient": return <Blend size={size} />;
    case "effect": return <Sparkles size={size} />;
    case "operator": return <Workflow size={size} />;
    case "sequence": return <SquareStack size={size} />;
    case "console": return <SlidersHorizontal size={size} />;
    case "file": return <File size={size} />;
  }
}

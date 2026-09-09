import * as AlertDialog from "@radix-ui/react-alert-dialog";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { runWorkspaceTransition } from "../workspaceTransitions";
import {
  ChevronDown,
  ChevronRight,
  File,
  FileAudio,
  FileJson2,
  FileLock2,
  FilePlus2,
  Folder,
  FolderOpen,
  FolderPlus,
  Image,
  Layers3,
  Pencil,
  RefreshCw,
  SearchCode,
  SquareStack,
  Trash2,
  Workflow
} from "lucide-react";
import {
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type SyntheticEvent
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
  const [pendingDelete, setPendingDelete] = useState<WorkspaceTreeNode | null>(null);
  const explorer = snapshot.workspaceExplorer;

  const saveExplorer = (next: WorkspaceExplorerState) => {
    void runSnapshotCommand(() => commands.saveWorkspaceExplorerState(next));
  };
  const beginCreate = (parent: string, directory: boolean) => {
    setInlineEdit({
      mode: directory ? "createDirectory" : "createFile",
      parent,
      value: "",
      error: null
    });
  };
  const commitInlineEdit = async () => {
    if (inlineEdit === null) return;
    const value = inlineEdit.value.trim();
    const validation = validateName(value);
    if (validation !== null) {
      setInlineEdit({ ...inlineEdit, error: validation });
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
      setInlineEdit({ ...inlineEdit, error: String(error) });
    }
  };

  return (
    <section className="sidebar-view explorer-view" aria-label="Explorer">
      <header className="sidebar-view-header">
        <h2>Explorer</h2>
        <div className="sidebar-toolbar">
          <button type="button" title="New file" aria-label="New file" onClick={() => { beginCreate("", false); }}>
            <FilePlus2 size={THEME_METRICS.iconSizeCompact} />
          </button>
          <button type="button" title="New folder" aria-label="New folder" onClick={() => { beginCreate("", true); }}>
            <FolderPlus size={THEME_METRICS.iconSizeCompact} />
          </button>
          <button type="button" title="Collapse all" aria-label="Collapse all" onClick={() => treeRef.current?.closeAll()}>
            <SquareStack size={THEME_METRICS.iconSizeCompact} />
          </button>
          <button type="button" title="Refresh" aria-label="Refresh project" onClick={() => void runWorkspaceTransition({ type: "reloadProject" })}>
            <RefreshCw size={THEME_METRICS.iconSizeCompact} />
          </button>
        </div>
      </header>
      <div className="explorer-tree-content">
        {inlineEdit !== null && inlineEdit.mode !== "rename" && (
          <InlineInput edit={inlineEdit} onChange={setInlineEdit} onSubmit={commitInlineEdit} onCancel={() => { setInlineEdit(null); }} />
        )}
        <div className="explorer-tree-host" ref={treeHostRef}>
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
            {...(snapshot.activeFile === null ? {} : { selection: snapshot.activeFile })}
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
                onEditChange={setInlineEdit}
                onCommit={commitInlineEdit}
                onCancel={() => { setInlineEdit(null); }}
                onBeginCreate={beginCreate}
                onBeginRename={(node) => { setInlineEdit({ mode: "rename", path: node.path, value: node.name, error: null }); }}
                onDelete={setPendingDelete}
              />
            )}
          </Tree>
        </div>
      </div>
      <DeleteDialog node={pendingDelete} onClose={() => { setPendingDelete(null); }} />
    </section>
  );
}

function TreeRow({
  node,
  style,
  dragHandle,
  editing,
  onEditChange,
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
  onEditChange: (edit: InlineEdit) => void;
  onCommit: () => Promise<void>;
  onCancel: () => void;
  onBeginCreate: (parent: string, directory: boolean) => void;
  onBeginRename: (node: WorkspaceTreeNode) => void;
  onDelete: (node: WorkspaceTreeNode) => void;
}) {
  const isRenaming = editing?.mode === "rename" && editing.path === node.data.path;
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>
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
            <InlineInput edit={editing} onChange={onEditChange} onSubmit={onCommit} onCancel={onCancel} compact />
          ) : (
            <span className="tree-row-name">{node.data.name}</span>
          )}
          {node.data.errorCount > 0 && <span className="diagnostic-badge error">{node.data.errorCount}</span>}
          {node.data.warningCount > 0 && <span className="diagnostic-badge warning">{node.data.warningCount}</span>}
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="menu-content">
          {node.data.kind === "directory" && hasOperation(node.data, "create") && (
            <>
              <ContextMenu.Item className="menu-item" onSelect={() => { onBeginCreate(node.data.path, false); }}>
                <FilePlus2 size={THEME_METRICS.iconSizeSmall} /> New File
              </ContextMenu.Item>
              <ContextMenu.Item className="menu-item" onSelect={() => { onBeginCreate(node.data.path, true); }}>
                <FolderPlus size={THEME_METRICS.iconSizeSmall} /> New Folder
              </ContextMenu.Item>
            </>
          )}
          <ContextMenu.Item
            className="menu-item"
            disabled={!hasOperation(node.data, "rename")}
            title={node.data.operationExplanation ?? undefined}
            onSelect={() => { onBeginRename(node.data); }}
          >
            <Pencil size={THEME_METRICS.iconSizeSmall} /> Rename
          </ContextMenu.Item>
          <ContextMenu.Item
            className="menu-item danger"
            disabled={!hasOperation(node.data, "delete")}
            title={node.data.operationExplanation ?? undefined}
            onSelect={() => { onDelete(node.data); }}
          >
            <Trash2 size={THEME_METRICS.iconSizeSmall} /> Delete
          </ContextMenu.Item>
          {!hasOperation(node.data, "rename") && node.data.operationExplanation !== null && (
            <ContextMenu.Label className="menu-explanation">{node.data.operationExplanation}</ContextMenu.Label>
          )}
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

function InlineInput({
  edit,
  onChange,
  onSubmit,
  onCancel,
  compact = false
}: {
  edit: InlineEdit;
  onChange: (edit: InlineEdit) => void;
  onSubmit: () => Promise<void>;
  onCancel: () => void;
  compact?: boolean;
}) {
  const submit = (event: SyntheticEvent<HTMLFormElement>) => {
    event.preventDefault();
    void onSubmit();
  };
  return (
    <form className={`inline-tree-edit ${compact ? "compact" : ""}`} onSubmit={submit}>
      <input
        autoFocus
        value={edit.value}
        aria-label={edit.mode === "rename" ? "New name" : "Name"}
        onChange={(event) => { onChange({ ...edit, value: event.currentTarget.value, error: null }); }}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onCancel();
          }
        }}
      />
      {edit.error !== null && <span className="inline-edit-error">{edit.error}</span>}
    </form>
  );
}

function DeleteDialog({ node, onClose }: { node: WorkspaceTreeNode | null; onClose: () => void }) {
  return (
    <AlertDialog.Root open={node !== null} onOpenChange={(open) => { if (!open) onClose(); }}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="dialog-overlay" />
        <AlertDialog.Content className="dialog-content">
          <AlertDialog.Title>Delete {node?.name}</AlertDialog.Title>
          <AlertDialog.Description>
            This permanently removes {node?.kind === "directory" ? "the directory and its contents" : "the file"} from the project.
          </AlertDialog.Description>
          <div className="dialog-actions">
            <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
            <AlertDialog.Action
              onClick={() => {
                if (node !== null) void runSnapshotCommand(() => commands.deletePath(node.path));
              }}
            >
              Delete
            </AlertDialog.Action>
          </div>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
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
    case "directory":
    case "pathDependency": return open ? <FolderOpen size={size} /> : <Folder size={size} />;
    case "manifest": return <FileJson2 size={size} />;
    case "lockfile": return <FileLock2 size={size} />;
    case "asset": return <FileAudio size={size} />;
    case "project":
    case "entrypoint": return <Workflow size={size} />;
    case "setup":
    case "layout":
    case "fixture":
    case "patch": return <Layers3 size={size} />;
    case "curve":
    case "gradient": return <Image size={size} />;
    case "effect":
    case "operator": return <SearchCode size={size} />;
    case "sequence": return <SquareStack size={size} />;
    case "file": return <File size={size} />;
  }
}

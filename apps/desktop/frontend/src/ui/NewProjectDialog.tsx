import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { FolderOpen } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { SyntheticEvent } from "react";
import { commands } from "../api";
import { useAppStore } from "../store";
import { runWorkspaceTransition } from "../workspaceTransitions";
import { THEME_METRICS } from "../theme";

const NEW_PROJECT_EVENT = "donder:new-project";

export function NewProjectDialog() {
  const [mode, setMode] = useState<"createProject" | "copyProject">("createProject");
  const [open, setOpen] = useState(false);
  const [directoryName, setDirectoryName] = useState("");
  const [parentPath, setParentPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    const onNewProject = () => {
      setMode("createProject");
      setOpen(true);
      setError(null);
    };
    const onCopyProject = () => {
      setMode("copyProject");
      setOpen(true);
      setError(null);
    };
    window.addEventListener("donder:copy-project", onCopyProject);
    window.addEventListener(NEW_PROJECT_EVENT, onNewProject);
    return () => {
      window.removeEventListener(NEW_PROJECT_EVENT, onNewProject);
      window.removeEventListener("donder:copy-project", onCopyProject);
    };
  }, []);

  const nameError = validateDirectoryName(directoryName);
  const projectPath = useMemo(() => {
    if (parentPath === "" || directoryName === "") return "";
    return `${parentPath.replace(/[\\/]+$/, "")}/${directoryName}`;
  }, [directoryName, parentPath]);
  const createDisabled = creating || parentPath === "" || directoryName === "" || nameError !== null;

  async function browseParent() {
    try {
      const selected = await commands.chooseNewProjectParentDirectory();
      if (selected !== null) {
        setParentPath(selected);
        setError(null);
      }
    } catch (caught) {
      setError(errorMessage(caught));
    }
  }

  async function createProject(event: SyntheticEvent<HTMLFormElement>) {
    event.preventDefault();
    if (createDisabled) return;
    setCreating(true);
    setError(null);
    useAppStore.getState().setError(null);
    try {
      const applied = await runWorkspaceTransition({ type: mode, parentPath, directoryName });
      if (!applied) {
        setError(useAppStore.getState().error);
        return;
      }
      useAppStore.getState().setError(null);
      setOpen(false);
      setDirectoryName("");
      setParentPath("");
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setCreating(false);
    }
  }

  return (
    <AlertDialog.Root open={open} onOpenChange={(value) => { if (!creating) setOpen(value); }}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="dialog-overlay" />
        <AlertDialog.Content className="dialog-content new-project-dialog">
          <AlertDialog.Title>{mode === "copyProject" ? "Create Editable Project Copy" : "New Project"}</AlertDialog.Title>
          {mode === "copyProject" && <AlertDialog.Description>Create and open a separate project containing the current show, imported definitions, and audio. Package files are preserved. Choose a new folder; existing folders cannot be overwritten.</AlertDialog.Description>}
          <form className="new-project-form" onSubmit={(event) => void createProject(event)}>
            <label>
              <span>Project folder name</span>
              <input
                autoFocus
                disabled={creating}
                value={directoryName}
                onChange={(event) => {
                  setDirectoryName(event.target.value);
                  setError(null);
                }}
              />
            </label>
            <label>
              <span>Parent location</span>
              <div className="new-project-location-row">
                <input readOnly value={parentPath} />
                <button type="button" disabled={creating} onClick={() => void browseParent()}>
                  <FolderOpen size={THEME_METRICS.iconSizeSmall} />
                  Browse...
                </button>
              </div>
            </label>
            <div className="new-project-path">
              <span>New project will be created in:</span>
              <strong>{projectPath === "" ? "-" : projectPath}</strong>
            </div>
            {(nameError !== null || error !== null) && (
              <div className="new-project-error">{error ?? nameError}</div>
            )}
            <div className="dialog-actions">
              <AlertDialog.Cancel disabled={creating}>Cancel</AlertDialog.Cancel>
              <button type="submit" disabled={createDisabled}>
                {creating ? "Creating..." : "Create"}
              </button>
            </div>
          </form>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}

function validateDirectoryName(name: string): string | null {
  if (name === "") return null;
  if (name.trim() !== name) return "Project folder name cannot start or end with whitespace.";
  if (name === "." || name === "..") return "Project folder name cannot be . or ..";
  if (name.includes("/") || name.includes("\\")) return "Project folder name must be a folder name, not a path.";
  return null;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

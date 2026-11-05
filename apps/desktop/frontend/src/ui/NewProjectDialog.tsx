import { FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";
import { commands } from "../api";
import { useAppStore } from "../store";
import { runWorkspaceTransition } from "../workspaceTransitions";
import { THEME_COLORS } from "../theme";
import { Button, Dialog, Field, TextField } from "./kit";

const NEW_PROJECT_EVENT = "donder:new-project";
const DEFAULT_DIRECTORY_NAME = "New Project";

type Mode = "createProject" | "copyProject" | "importProfile";

const TITLES: Record<Mode, string> = { createProject: "New Project", copyProject: "Copy Project", importProfile: "Import Profile" };
const ACTIONS: Record<Mode, string> = { createProject: "Create", copyProject: "Copy", importProfile: "Import" };

export function NewProjectDialog() {
  const [mode, setMode] = useState<Mode>("createProject");
  const [open, setOpen] = useState(false);
  const [directoryName, setDirectoryName] = useState(DEFAULT_DIRECTORY_NAME);
  const [parentPath, setParentPath] = useState("");
  const [profilePath, setProfilePath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    const opener = (next: Mode) => () => {
      setMode(next);
      setDirectoryName(DEFAULT_DIRECTORY_NAME);
      // Beside the open project unless a location was already chosen.
      const root = useAppStore.getState().snapshot?.projectRoot ?? null;
      if (root !== null) setParentPath((current) => current === "" ? parentDirectory(root) : current);
      setError(null);
      setOpen(true);
    };
    const onNewProject = opener("createProject");
    const onCopyProject = opener("copyProject");
    const onImportProfile = opener("importProfile");
    window.addEventListener("donder:copy-project", onCopyProject);
    window.addEventListener("donder:import-profile", onImportProfile);
    window.addEventListener(NEW_PROJECT_EVENT, onNewProject);
    return () => {
      window.removeEventListener(NEW_PROJECT_EVENT, onNewProject);
      window.removeEventListener("donder:import-profile", onImportProfile);
      window.removeEventListener("donder:copy-project", onCopyProject);
    };
  }, []);

  const nameError = validateDirectoryName(directoryName);
  const projectPath = parentPath === "" || directoryName === "" ? "" : `${parentPath.replace(/[\\/]+$/, "")}/${directoryName}`;
  const createDisabled = creating || parentPath === "" || nameError !== null
    || (mode === "importProfile" && profilePath === "");

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

  async function browseProfile() {
    try {
      const selected = await commands.chooseProfileDirectory();
      if (selected !== null) {
        setProfilePath(selected);
        setError(null);
      }
    } catch (caught) {
      setError(errorMessage(caught));
    }
  }

  async function createProject() {
    if (createDisabled) return;
    setCreating(true);
    setError(null);
    useAppStore.getState().setError(null);
    try {
      const applied = await runWorkspaceTransition(mode === "createProject"
        ? { type: mode, parentPath, directoryName, initialColor: THEME_COLORS.defaultSequenceColor }
        : mode === "importProfile"
          ? { type: mode, parentPath, directoryName, initialColor: THEME_COLORS.defaultSequenceColor, profilePath }
          : { type: mode, parentPath, directoryName });
      if (!applied) {
        setError(useAppStore.getState().error);
        return;
      }
      useAppStore.getState().setError(null);
      setOpen(false);
      setProfilePath("");
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setCreating(false);
    }
  }

  const shownError = error ?? nameError;
  return <Dialog
    open={open}
    onOpenChange={(value) => { if (!creating) setOpen(value); }}
    title={TITLES[mode]}
    actions={<>
      <Button disabled={creating} onClick={() => { setOpen(false); }}>Cancel</Button>
      <Button variant="primary" disabled={createDisabled} onClick={() => void createProject()}>
        {creating ? "Creating…" : ACTIONS[mode]}
      </Button>
    </>}
  >
    {mode === "importProfile" && <Field label="Profile" tip="The xLights show folder (holds xlights_rgbeffects.xml) or the Vixen profile folder (holds SystemData)">
      <FolderButton path={profilePath} disabled={creating} onClick={() => void browseProfile()} />
    </Field>}
    <Field label="Name" tip={mode === "copyProject" ? "The copy's folder; it holds the show, imported definitions and audio, and edits independently" : "The project's folder name"}>
      <TextField
        autoFocus
        name="Project folder name"
        disabled={creating}
        invalid={nameError !== null}
        value={directoryName}
        onCommit={(value) => {
          setDirectoryName(value);
          setError(null);
        }}
      />
    </Field>
    <Field label="Location">
      <FolderButton path={parentPath} tip={projectPath} disabled={creating} onClick={() => void browseParent()} />
    </Field>
    {shownError !== null && <div className="kit-status-error" role="alert">{shownError}</div>}
  </Dialog>;
}

/** A folder choice that shows the chosen folder's name; the full path is its tooltip. */
function FolderButton({ path, tip, disabled, onClick }: { path: string; tip?: string; disabled: boolean; onClick: () => void }) {
  const label = tip !== undefined && tip !== "" ? tip : path === "" ? "Choose folder" : path;
  return <Button icon={FolderOpen} tip={label} disabled={disabled} onClick={onClick}>
    {path === "" ? "Choose…" : path.replace(/[\\/]+$/, "").split(/[\\/]/).pop()}
  </Button>;
}

function parentDirectory(path: string): string {
  return path.replace(/[\\/]+$/, "").replace(/[\\/][^\\/]*$/, "");
}

function validateDirectoryName(name: string): string | null {
  if (name.trim() === "") return "Name required";
  if (name.trim() !== name) return "No leading or trailing spaces";
  if (name === "." || name === "..") return "Not a folder name";
  if (name.includes("/") || name.includes("\\")) return "Name, not a path";
  return null;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

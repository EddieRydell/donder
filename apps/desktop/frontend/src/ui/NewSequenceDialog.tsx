import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { useEffect, useMemo, useState } from "react";
import type { SyntheticEvent } from "react";
import { commands } from "../api";
import { useAppStore } from "../store";
import { THEME_COLORS } from "../theme";
import { navigateToGuiObject } from "../workspace/navigation";
import type { NewSequenceStorage } from "../types";

const NEW_SEQUENCE_EVENT = "donder:new-sequence";

export function NewSequenceDialog() {
  const snapshot = useAppStore((store) => store.snapshot);
  const [open, setOpen] = useState(false);
  const [declarationName, setDeclarationName] = useState("Sequence");
  const [storage, setStorage] = useState<NewSequenceStorage["type"]>("inline");
  const [durationSeconds, setDurationSeconds] = useState("60");
  const [frameRate, setFrameRate] = useState("60");
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    const onNewSequence = () => {
      setDeclarationName("Sequence");
      setStorage("inline");
      setDurationSeconds("60");
      setFrameRate("60");
      setError(null);
      setOpen(true);
    };
    window.addEventListener(NEW_SEQUENCE_EVENT, onNewSequence);
    return () => {
      window.removeEventListener(NEW_SEQUENCE_EVENT, onNewSequence);
    };
  }, []);

  const validationError = useMemo(
    () => validateRequest(snapshot?.projectRoot ?? null, storage, declarationName, durationSeconds, frameRate),
    [snapshot?.projectRoot, storage, declarationName, durationSeconds, frameRate]
  );
  const createDisabled = creating || validationError !== null;

  async function createSequence(event: SyntheticEvent<HTMLFormElement>) {
    event.preventDefault();
    if (createDisabled) return;
    setCreating(true);
    setError(null);
    try {
      const next = await commands.createSequence({
        storage: storage === "inline" ? { type: "inline" } : { type: storage, name: declarationName.trim() },
        initialColor: THEME_COLORS.defaultSequenceColor,
        durationSeconds: Number(durationSeconds),
        frameRate: Number(frameRate)
      });
      useAppStore.getState().setSnapshot(next.snapshot);
      useAppStore.getState().setError(null);
      setOpen(false);
      await navigateToGuiObject(next.source);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setCreating(false);
    }
  }

  return (
    <AlertDialog.Root open={open} onOpenChange={setOpen}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="dialog-overlay" />
        <AlertDialog.Content className="dialog-content new-sequence-dialog">
          <AlertDialog.Title>New Sequence</AlertDialog.Title>
          <form className="new-sequence-form" onSubmit={(event) => void createSequence(event)}>
            <details className="composition-add-advanced">
              <summary>Advanced settings</summary>
              <label>
                <span>Storage</span>
                <select value={storage} onChange={(event) => { setStorage(event.target.value as NewSequenceStorage["type"]); }}>
                  <option value="inline">Inline in this project</option>
                  <option value="sameFile">Reusable declaration in this file</option>
                  <option value="newFile">Reusable declaration in a new file</option>
                </select>
              </label>
              {storage !== "inline" && (
                <label>
                  <span>Declaration name</span>
                  <input value={declarationName} onChange={(event) => { setDeclarationName(event.target.value); }} />
                </label>
              )}
            </details>
            <div className="new-sequence-grid">
              <label>
                <span>Duration seconds</span>
                <input
                  inputMode="decimal"
                  value={durationSeconds}
                  onChange={(event) => {
                    setDurationSeconds(event.target.value);
                    setError(null);
                  }}
                />
              </label>
              <label>
                <span>Frame rate</span>
                <input
                  inputMode="numeric"
                  value={frameRate}
                  onChange={(event) => {
                    setFrameRate(event.target.value);
                    setError(null);
                  }}
                />
              </label>
            </div>
            {(validationError !== null || error !== null) && (
              <div className="new-project-error">{error ?? validationError}</div>
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

function validateRequest(
  projectRoot: string | null,
  storage: NewSequenceStorage["type"],
  declarationName: string,
  durationSeconds: string,
  frameRate: string
): string | null {
  if (projectRoot === null) return "Open or create a project before adding a sequence.";
  if (storage !== "inline" && declarationName.trim() === "") return "Enter a name for the reusable declaration.";
  const duration = Number(durationSeconds);
  if (!Number.isFinite(duration) || duration <= 0) return "Duration must be greater than zero.";
  const rate = Number(frameRate);
  if (!Number.isInteger(rate) || rate <= 0) return "Frame rate must be a positive whole number.";
  return null;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

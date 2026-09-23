import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { useEffect, useMemo, useState } from "react";
import type { SyntheticEvent } from "react";
import { commands } from "../api";
import { useAppStore } from "../store";

const NEW_SEQUENCE_EVENT = "donder:new-sequence";

export function NewSequenceDialog() {
  const snapshot = useAppStore((store) => store.snapshot);
  const [open, setOpen] = useState(false);
  const [objectKey, setObjectKey] = useState("main");
  const [filePath, setFilePath] = useState("sequences/main.sequence.donder");
  const [durationSeconds, setDurationSeconds] = useState("60");
  const [frameRate, setFrameRate] = useState("60");
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [pathEdited, setPathEdited] = useState(false);

  useEffect(() => {
    const onNewSequence = () => {
      const nextKey = nextSequenceKey(snapshot?.projectEntries.map((entry) => entry.path) ?? []);
      setObjectKey(nextKey);
      setFilePath(`sequences/${nextKey}.sequence.donder`);
      setDurationSeconds("60");
      setFrameRate("60");
      setPathEdited(false);
      setError(null);
      setOpen(true);
    };
    window.addEventListener(NEW_SEQUENCE_EVENT, onNewSequence);
    return () => {
      window.removeEventListener(NEW_SEQUENCE_EVENT, onNewSequence);
    };
  }, [snapshot?.projectEntries]);

  const validationError = useMemo(
    () => validateRequest(snapshot?.projectRoot ?? null, objectKey, filePath, durationSeconds, frameRate),
    [snapshot?.projectRoot, objectKey, filePath, durationSeconds, frameRate]
  );
  const createDisabled = creating || validationError !== null;

  async function createSequence(event: SyntheticEvent<HTMLFormElement>) {
    event.preventDefault();
    if (createDisabled) return;
    setCreating(true);
    setError(null);
    try {
      const next = await commands.createSequence({
        filePath,
        objectKey,
        durationSeconds: Number(durationSeconds),
        frameRate: Number(frameRate)
      });
      useAppStore.getState().setSnapshot(next);
      useAppStore.getState().setError(null);
      setOpen(false);
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
            <label>
              <span>Sequence id</span>
              <input
                autoFocus
                value={objectKey}
                onChange={(event) => {
                  const next = event.target.value;
                  setObjectKey(next);
                  if (!pathEdited) setFilePath(`sequences/${next}.sequence.donder`);
                  setError(null);
                }}
              />
            </label>
            <label>
              <span>File path</span>
              <input
                value={filePath}
                onChange={(event) => {
                  setFilePath(event.target.value);
                  setPathEdited(true);
                  setError(null);
                }}
              />
            </label>
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
  objectKey: string,
  filePath: string,
  durationSeconds: string,
  frameRate: string
): string | null {
  if (projectRoot === null) return "Open or create a project before adding a sequence.";
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(objectKey)) {
    return "Sequence id must start with a letter or underscore and contain only letters, digits, or underscores.";
  }
  if (!filePath.startsWith("sequences/") || !filePath.endsWith(".sequence.donder") || filePath.includes("..")) {
    return "File path must be under sequences/ and end with .sequence.donder.";
  }
  const duration = Number(durationSeconds);
  if (!Number.isFinite(duration) || duration <= 0) return "Duration must be greater than zero.";
  const rate = Number(frameRate);
  if (!Number.isInteger(rate) || rate <= 0) return "Frame rate must be a positive whole number.";
  return null;
}

function nextSequenceKey(paths: string[]): string {
  let index = 1;
  while (paths.includes(`sequences/sequence_${index}.sequence.donder`)) index += 1;
  return `sequence_${index}`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

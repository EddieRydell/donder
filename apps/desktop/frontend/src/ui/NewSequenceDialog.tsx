import { useEffect, useState } from "react";
import { commands } from "../api";
import { useAppStore } from "../store";
import { THEME_COLORS } from "../theme";
import { navigateToGuiObject } from "../workspace/navigation";
import type { NewSequenceStorage } from "../types";
import { Button, Dialog, Field, NumberField, Select, TextField, type SelectOption } from "./kit";

const NEW_SEQUENCE_EVENT = "donder:new-sequence";
const DEFAULT_NAME = "Sequence";
const DEFAULT_DURATION_SECONDS = 60;
const DEFAULT_FRAME_RATE = 60;

const STORAGE_OPTIONS: SelectOption<NewSequenceStorage["type"]>[] = [
  { value: "inline", label: "Inline" },
  { value: "sameFile", label: "This file" },
  { value: "newFile", label: "New file" }
];

export function NewSequenceDialog() {
  const projectRoot = useAppStore((store) => store.snapshot?.projectRoot ?? null);
  const [open, setOpen] = useState(false);
  const [declarationName, setDeclarationName] = useState(DEFAULT_NAME);
  const [storage, setStorage] = useState<NewSequenceStorage["type"]>("inline");
  const [durationSeconds, setDurationSeconds] = useState(DEFAULT_DURATION_SECONDS);
  const [frameRate, setFrameRate] = useState(DEFAULT_FRAME_RATE);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    const onNewSequence = () => {
      setDeclarationName(DEFAULT_NAME);
      setStorage("inline");
      setDurationSeconds(DEFAULT_DURATION_SECONDS);
      setFrameRate(DEFAULT_FRAME_RATE);
      setError(null);
      setOpen(true);
    };
    window.addEventListener(NEW_SEQUENCE_EVENT, onNewSequence);
    return () => {
      window.removeEventListener(NEW_SEQUENCE_EVENT, onNewSequence);
    };
  }, []);

  const validationError = projectRoot === null
    ? "No project open"
    : storage !== "inline" && declarationName.trim() === "" ? "Name required" : null;
  const createDisabled = creating || validationError !== null;

  async function createSequence() {
    if (createDisabled) return;
    setCreating(true);
    setError(null);
    try {
      const next = await commands.createSequence({
        storage: storage === "inline" ? { type: "inline" } : { type: storage, name: declarationName.trim() },
        initialColor: THEME_COLORS.defaultSequenceColor,
        durationSeconds,
        frameRate
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

  const shownError = error ?? validationError;
  return <Dialog
    open={open}
    onOpenChange={setOpen}
    title="New Sequence"
    actions={<>
      <Button disabled={creating} onClick={() => { setOpen(false); }}>Cancel</Button>
      <Button variant="primary" disabled={createDisabled} onClick={() => void createSequence()}>
        {creating ? "Creating…" : "Create"}
      </Button>
    </>}
  >
    <Field label="Duration">
      <NumberField name="Duration" value={durationSeconds} step={1} min={1} defaultValue={DEFAULT_DURATION_SECONDS} unit="s" onCommit={(value) => { setDurationSeconds(value); setError(null); }} />
    </Field>
    <Field label="Frame rate">
      <NumberField name="Frame rate" value={frameRate} step={1} min={1} integer defaultValue={DEFAULT_FRAME_RATE} unit="fps" onCommit={(value) => { setFrameRate(value); setError(null); }} />
    </Field>
    <Field label="Storage" tip="Inline in this project, or a reusable declaration in this file or a new file">
      <Select name="Storage" value={storage} options={STORAGE_OPTIONS} onChange={setStorage} />
    </Field>
    {storage !== "inline" && <Field label="Name" tip="The reusable declaration's name">
      <TextField name="Declaration name" value={declarationName} invalid={declarationName.trim() === ""} onCommit={setDeclarationName} />
    </Field>}
    {shownError !== null && <div className="kit-status-error" role="alert">{shownError}</div>}
  </Dialog>;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

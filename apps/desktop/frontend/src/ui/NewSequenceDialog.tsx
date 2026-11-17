import { useEffect, useState, type ReactNode } from "react";
import { commands, errorMessage } from "../api";
import { showRefusedEdit, useAppStore } from "../store";
import { THEME_COLORS } from "../theme";
import { navigateToGuiObject } from "../workspace/navigation";
import type { GuiField, GuiProblem, NewObjectStorage } from "../types";
import { Button, Dialog, Field, NumberField, TextField } from "./kit";
import { DEFAULT_OBJECT_STORAGE, NEW_OBJECT_STORAGE, ObjectStorageField } from "./gui/objectStorage";
import { listenUiEvent } from "./uiEvents";
import { DraftProblem, OBJECT, useFieldProblem } from "./gui/problems";

const DEFAULT_NAME = "Sequence";
const DEFAULT_DURATION_SECONDS = 60;
const DEFAULT_FRAME_RATE = 60;


export function NewSequenceDialog() {
  const projectRoot = useAppStore((store) => store.snapshot?.projectRoot ?? null);
  const [open, setOpen] = useState(false);
  const [declarationName, setDeclarationName] = useState(DEFAULT_NAME);
  const [storage, setStorage] = useState<NewObjectStorage>(DEFAULT_OBJECT_STORAGE);
  const [durationSeconds, setDurationSeconds] = useState(DEFAULT_DURATION_SECONDS);
  const [frameRate, setFrameRate] = useState(DEFAULT_FRAME_RATE);
  // Why the last Create was refused; its fields show it.
  const [problem, setProblem] = useState<GuiProblem | null>(null);
  const [creating, setCreating] = useState(false);

  useEffect(() => listenUiEvent("newSequence", () => {
      setDeclarationName(DEFAULT_NAME);
      setStorage(DEFAULT_OBJECT_STORAGE);
      setDurationSeconds(DEFAULT_DURATION_SECONDS);
      setFrameRate(DEFAULT_FRAME_RATE);
      setProblem(null);
      setOpen(true);
  }), []);

  const named = storage !== "inline";
  const nameMissing = named && declarationName.trim() === "";
  const createDisabled = creating || projectRoot === null || nameMissing;

  async function createSequence() {
    if (createDisabled) return;
    setCreating(true);
    setProblem(null);
    try {
      const next = await commands.createSequence({
        name: declarationName.trim(),
        storage,
        initialColor: THEME_COLORS.defaultSequenceColor,
        durationSeconds,
        frameRate
      });
      if (next.type === "refused") {
        // A problem no field of the dialog shows goes to the status bar.
        if (next.problem.site === null) showRefusedEdit(next.problem);
        else setProblem(next.problem);
        return;
      }
      useAppStore.getState().setSnapshot(next.snapshot);
      setOpen(false);
      await navigateToGuiObject(next.source);
    } catch (error) {
      useAppStore.getState().reportFailure("command", errorMessage(error));
    } finally {
      setCreating(false);
    }
  }

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
  ><DraftProblem problem={problem}>
    <SequenceField label="Name" field={{ type: "name" }} missing={nameMissing ? "Name required" : null}>{(invalid) =>
      <TextField name="Sequence name" value={declarationName} disabled={!named} invalid={invalid} onCommit={(value) => { setDeclarationName(value); setProblem(null); }} />}
    </SequenceField>
    <SequenceField label="Duration" field={{ type: "duration" }}>{(invalid) =>
      <NumberField name="Duration" value={durationSeconds} step={1} min={1} defaultValue={DEFAULT_DURATION_SECONDS} unit="s" invalid={invalid} onCommit={(value) => { setDurationSeconds(value); setProblem(null); }} />}
    </SequenceField>
    <SequenceField label="Frame rate" field={{ type: "frameRate" }}>{(invalid) =>
      <NumberField name="Frame rate" value={frameRate} step={1} min={1} integer defaultValue={DEFAULT_FRAME_RATE} unit="fps" invalid={invalid} onCommit={(value) => { setFrameRate(value); setProblem(null); }} />}
    </SequenceField>
    <ObjectStorageField value={storage} choices={NEW_OBJECT_STORAGE} onChange={setStorage} />
  </DraftProblem></Dialog>;
}

/** A field of the new sequence, marked when Create was refused for it, or when it is `missing` a value.
 * An object stored inline takes no name; its field stays so the dialog keeps its shape. */
function SequenceField({ label, field, missing = null, children }: { label: string; field: GuiField; missing?: string | null; children: (invalid: boolean) => ReactNode }) {
  const error = useFieldProblem(OBJECT, field) ?? missing;
  return <Field label={label} error={error}>{children(error !== null)}</Field>;
}

import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import { useState } from "react";
import type { SequenceGuiDocument } from "../../../types";
import { Button, Field, NumberField, Section, Select, TextField } from "../../kit";
import { defaultMarkColor, subdivisionTimes } from "./marks";

const MIN_DIVISIONS = 2;
const MAX_DIVISIONS = 32;
/** New marks this close to an existing mark in the target collection are skipped. */
const DUPLICATE_MARK_SECONDS = 1e-4;
/** The target value for adding the subdivisions as a new collection. */
const NEW_COLLECTION = "";

/** Divides every gap between the selected marks into equal parts. */
export function MarkSubdivision({
  document,
  selectedSeconds,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys,
  clearMarkSelection
}: {
  document: SequenceGuiDocument;
  selectedSeconds: number[];
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
  clearMarkSelection: () => void;
}) {
  const [divisions, setDivisions] = useState(MIN_DIVISIONS);
  const [target, setTarget] = useState(NEW_COLLECTION);
  const [name, setName] = useState("subdivisions");
  const divisionsValid = Number.isInteger(divisions) && divisions >= MIN_DIVISIONS && divisions <= MAX_DIVISIONS;
  const targetCollection = document.markCollections.find((collection) => collection.key === target);
  const valid = divisionsValid && (targetCollection !== undefined || name.trim().length > 0);

  const subdivide = () => {
    const existing = targetCollection?.marksSeconds ?? [];
    const timesSeconds = subdivisionTimes(selectedSeconds, divisions).filter(
      (time) => !existing.some((mark) => Math.abs(mark - time) < DUPLICATE_MARK_SECONDS)
    );
    if (targetCollection === undefined) {
      void runGuiEditCommand((request) =>
        commands.applyGuiEdit(request, { type: "sequence", edit: {
          type: "createMarkCollections",
          collections: [{ name, color: defaultMarkColor(document.markCollections.length), marksSeconds: timesSeconds }]
        } })
      ).then(({ created }) => {
        setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, ...created]));
      });
      return;
    }
    // Inserting marks renumbers the target collection, so the selection's indices would go stale.
    void runGuiEditCommand((request) =>
      commands.applyGuiEdit(request, { type: "sequence", edit: { type: "addMarks", collectionKey: targetCollection.key, timesSeconds } })
    ).then(clearMarkSelection);
  };

  return (
    <Section title="Subdivide" defaultOpen={false}>
      <Field label="Divisions" tip="Equal parts per gap between selected marks">
        <NumberField name="Divisions" value={divisions} min={MIN_DIVISIONS} max={MAX_DIVISIONS} step={1} integer defaultValue={MIN_DIVISIONS} onCommit={setDivisions} />
      </Field>
      <Field label="Add to">
        <Select
          name="Add to"
          value={target}
          options={[
            { value: NEW_COLLECTION, label: "New collection" },
            ...document.markCollections.map((collection) => ({ value: collection.key, label: collection.key }))
          ]}
          onChange={setTarget}
        />
      </Field>
      {targetCollection === undefined && (
        <Field label="Name">
          <TextField name="New collection name" value={name} onCommit={setName} />
        </Field>
      )}
      <Button disabled={!valid} onClick={subdivide}>Subdivide</Button>
    </Section>
  );
}

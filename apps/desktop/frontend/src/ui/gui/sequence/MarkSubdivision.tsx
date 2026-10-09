import { useState } from "react";
import { useSequenceEditorHost } from "../../../editor/host";
import type { SequenceEditorDocument } from "../../../editor/types";
import { defaultMarkColor, nextCollectionKey, subdivisionTimes } from "./marks";

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
  document: SequenceEditorDocument;
  selectedSeconds: number[];
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
  clearMarkSelection: () => void;
}) {
  const { commands, runGuiEditCommand } = useSequenceEditorHost();
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
      const key = nextCollectionKey(name, document.markCollections);
      void runGuiEditCommand((request) =>
        commands.applySequenceGuiEdit(request, {
          type: "createMarkCollections",
          collections: [{ name: key, color: defaultMarkColor(document.markCollections.length), marksSeconds: timesSeconds }]
        })
      ).then(() => {
        setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, key]));
      });
      return;
    }
    // Inserting marks renumbers the target collection, so the selection's indices would go stale.
    void runGuiEditCommand((request) =>
      commands.applySequenceGuiEdit(request, { type: "addMarks", collectionKey: targetCollection.key, timesSeconds })
    ).then(clearMarkSelection);
  };

  return (
    <div className="mark-subdivision">
      <h4>Subdivide</h4>
      <label>
        Divisions per gap
        <input
          type="number"
          min={MIN_DIVISIONS}
          max={MAX_DIVISIONS}
          step={1}
          value={divisions}
          onChange={(event) => { setDivisions(event.currentTarget.valueAsNumber); }}
        />
      </label>
      <label>
        Add to
        <select value={target} onChange={(event) => { setTarget(event.currentTarget.value); }}>
          <option value={NEW_COLLECTION}>New collection</option>
          {document.markCollections.map((collection) => (
            <option key={collection.key} value={collection.key}>{collection.key}</option>
          ))}
        </select>
      </label>
      {targetCollection === undefined && (
        <label>
          Name
          <input type="text" value={name} onChange={(event) => { setName(event.currentTarget.value); }} />
        </label>
      )}
      <button type="button" className="neutral-button" disabled={!valid} onClick={subdivide}>
        Subdivide {selectedSeconds.length} marks
      </button>
    </div>
  );
}

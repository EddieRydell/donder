import type { NewObjectStorage, ReusableStorage } from "../../types";
import { Field, Section, Select } from "../kit";

/** A new object gets its own file and name, so other objects can use it; storing it inside its parent is the
 * advanced choice. */
export const DEFAULT_OBJECT_STORAGE = "newFile" satisfies NewObjectStorage;

/** The choices for a new object, and for one moved out of its parent. */
export const NEW_OBJECT_STORAGE: readonly NewObjectStorage[] = ["newFile", "sameFile", "inline"];
export const REUSABLE_STORAGE: readonly ReusableStorage[] = ["newFile", "sameFile"];

const LABELS: Record<NewObjectStorage, string> = { newFile: "Own file", sameFile: "This file", inline: "Inline" };
const TIPS: Record<NewObjectStorage, string> = {
  newFile: "A file of its own, named after it. Other objects can use it too.",
  sameFile: "A named declaration in this file. Other objects can use it too.",
  inline: "Stored inside its parent. Duplicating the parent copies it; removing it from the parent removes it."
};

/** The advanced choice of where an object is saved, from `choices`. */
export function ObjectStorageField<T extends NewObjectStorage>({ value, choices, disabled = false, onChange }: {
  value: T; choices: readonly T[]; disabled?: boolean; onChange: (value: T) => void;
}) {
  return <Section title="Advanced" defaultOpen={false}>
    <Field label="Save in" tip={TIPS[value]}>
      <Select name="Save in" value={value} disabled={disabled} options={choices.map((choice) => ({ value: choice, label: LABELS[choice] }))} onChange={onChange} />
    </Field>
  </Section>;
}

import { ExternalLink } from "lucide-react";
import { guiObjectKey, sameGuiObject } from "../../../workspace/guiIdentity";
import { navigateToGuiObject } from "../../../workspace/navigation";
import type { GuiObjectRef } from "../../../types";
import { Field, IconButton, Select } from "../../kit";

const MISSING = "missing";

/** A choice of declaration, and a button that opens the chosen one. */
export function ReferenceInput({ label, value, choices, disabled = false, error = null, onChange }: { label: string; value: GuiObjectRef; choices: { reference: GuiObjectRef; label: string }[]; disabled?: boolean; error?: string | null; onChange: (value: GuiObjectRef) => void }) {
  const current = choices.find((choice) => sameGuiObject(choice.reference, value));
  const options = [
    ...(current === undefined ? [{ value: MISSING, label: `${value.objectKey} (missing)`, disabled: true }] : []),
    ...choices.map((choice) => ({ value: guiObjectKey(choice.reference), label: choice.label }))
  ];
  return <Field label={label} error={error}>
    <Select name={label} value={current === undefined ? MISSING : guiObjectKey(current.reference)} options={options} disabled={disabled} invalid={current === undefined || error !== null} onChange={(key) => {
      const next = choices.find((choice) => guiObjectKey(choice.reference) === key);
      if (next !== undefined) onChange(next.reference);
    }} />
    <IconButton icon={ExternalLink} label="Open declaration" onClick={() => void navigateToGuiObject(value)} />
  </Field>;
}

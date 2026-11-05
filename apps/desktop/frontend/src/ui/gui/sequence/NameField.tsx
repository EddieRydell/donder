import { Field, TextField } from "../../kit";

/** An object's name, which references to it use; renaming updates them. An empty name reverts. */
export function NameField({ name, label, commit }: { name: string; label: string; commit: (name: string) => Promise<unknown> }) {
  return <Field label={label}>
    <TextField name={label} value={name} onCommit={(text) => {
      const next = text.trim();
      if (next === "" || next === name) return;
      void commit(next).catch(() => undefined);
    }} />
  </Field>;
}

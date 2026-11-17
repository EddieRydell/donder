import type { GuiMember } from "../../../types";
import { Field, TextField } from "../../kit";
import { useFieldProblem } from "../problems";

/** The name of `member` of the open object, which references to it use; renaming updates them. An empty name
 * reverts, and a refused one marks the field. */
export function NameField({ name, label, member, commit }: { name: string; label: string; member: GuiMember; commit: (name: string) => Promise<unknown> }) {
  const problem = useFieldProblem(member, { type: "name" });
  return <Field label={label} error={problem}>
    <TextField name={label} value={name} invalid={problem !== null} onCommit={(text) => {
      const next = text.trim();
      if (next === "" || next === name) return;
      void commit(next);
    }} />
  </Field>;
}

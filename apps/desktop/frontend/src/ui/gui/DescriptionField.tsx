import type { GuiMember } from "../../types";
import { Field, Problem, TextField } from "../kit";
import { useFieldProblem } from "./problems";

type DescriptionProps = {
  description: string | null;
  name: string;
  disabled: boolean;
  className?: string | undefined;
  onCommit: (description: string | null) => Promise<unknown>;
};

/** An object's description, prose saved with the object in its document; an empty one is none. */
function DescriptionInput({ description, name, disabled, className, onCommit, problem }: DescriptionProps & { problem: string | null }) {
  return <TextField
    multiline
    name={name}
    placeholder={name}
    value={description ?? ""}
    disabled={disabled}
    invalid={problem !== null}
    {...(className === undefined ? {} : { className })}
    onCommit={(text) => {
      const next = text.trim();
      if (next === (description ?? "")) return;
      void onCommit(next === "" ? null : next);
    }}
  />;
}

/** A description without a label. `member` is the part of the open object it describes, where a refused edit of
 * it shows; a draft's description has none. */
export function DescriptionText({ description, member = null, name = "Description", disabled = false, className, onCommit }: {
  description: string | null;
  member?: GuiMember | null;
  name?: string;
  disabled?: boolean;
  className?: string;
  onCommit: (description: string | null) => Promise<unknown>;
}) {
  const problem = useFieldProblem(member ?? [], { type: "description" });
  return <Problem error={problem}>
    <DescriptionInput description={description} name={name} disabled={disabled} className={className} onCommit={onCommit} problem={problem} />
  </Problem>;
}

/** The description of `member` of the open object as a labelled field. */
export function DescriptionField({ description, member, label = "Description", disabled = false, onCommit }: {
  description: string | null;
  member: GuiMember;
  label?: string;
  disabled?: boolean;
  onCommit: (description: string | null) => Promise<unknown>;
}) {
  const problem = useFieldProblem(member, { type: "description" });
  return <Field label={label} error={problem}>
    <DescriptionInput description={description} name={label} disabled={disabled} onCommit={onCommit} problem={problem} />
  </Field>;
}

import { useMemo, useState } from "react";
import { useAppStore } from "../../store";
import type { GuiEditCommand, GuiEditUpdate } from "../../types";
import { Button, Dialog, Field, TextField } from "../kit";
import { DraftProblem, OBJECT, useDraftCheck, useFieldProblem } from "./problems";

/** Naming something new, or renaming it: one Name field, prefilled with `initial`. `draft` is the edit an add makes,
 * checked as the name changes; `submit` applies the name and resolves to null when it was refused. */
export function NameDialog({ title, action, initial, draft, submit, onClose }: {
  title: string;
  action: string;
  initial: string;
  draft?: (name: string) => GuiEditCommand;
  submit: (name: string) => Promise<GuiEditUpdate | null>;
  onClose: () => void;
}) {
  const [name, setName] = useState(initial);
  const request = useAppStore((state) => state.guiRequest);
  const pending = useAppStore((state) => state.guiEditPending);
  const trimmed = name.trim();
  const edit = useMemo(() => draft === undefined || trimmed === "" ? null : draft(trimmed), [draft, trimmed]);
  const { checking, problem } = useDraftCheck(request, edit);
  const blocked = pending || checking || problem !== null || trimmed === "";
  const apply = () => {
    if (!blocked) void submit(trimmed).then((result) => { if (result !== null) onClose(); });
  };
  return <Dialog
    open
    onOpenChange={(open) => { if (!open) onClose(); }}
    title={title}
    actions={<>
      <Button onClick={onClose}>Cancel</Button>
      <Button variant="primary" disabled={blocked} {...(problem === null ? {} : { tip: problem.message })} onClick={apply}>{action}</Button>
    </>}
  ><DraftProblem problem={problem}>
    <NameField name={name} disabled={pending} onChange={setName} />
  </DraftProblem></Dialog>;
}

function NameField({ name, disabled, onChange }: { name: string; disabled: boolean; onChange: (name: string) => void }) {
  const problem = useFieldProblem(OBJECT, { type: "name" });
  return <Field label="Name" error={problem}>
    <TextField name="Name" live autoFocus disabled={disabled} value={name} invalid={problem !== null || name.trim() === ""} onCommit={onChange} />
  </Field>;
}

import { Field, TextField } from "../kit";

/** An object's description, prose saved with the object in its document; an empty one is none. */
export function DescriptionText({ description, name = "Description", disabled = false, className, onCommit }: {
  description: string | null;
  name?: string;
  disabled?: boolean;
  className?: string;
  onCommit: (description: string | null) => Promise<unknown>;
}) {
  return <TextField
    multiline
    name={name}
    placeholder={name}
    value={description ?? ""}
    disabled={disabled}
    {...(className === undefined ? {} : { className })}
    onCommit={(text) => {
      const next = text.trim();
      if (next === (description ?? "")) return;
      void onCommit(next === "" ? null : next).catch(() => undefined);
    }}
  />;
}

/** An object's description as a labelled field. */
export function DescriptionField({ description, label = "Description", disabled = false, onCommit }: {
  description: string | null;
  label?: string;
  disabled?: boolean;
  onCommit: (description: string | null) => Promise<unknown>;
}) {
  return <Field label={label}><DescriptionText description={description} name={label} disabled={disabled} onCommit={onCommit} /></Field>;
}

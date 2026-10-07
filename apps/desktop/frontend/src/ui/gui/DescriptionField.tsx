/** An object's description: prose saved with the object in its document. */
export function DescriptionField({
  description,
  label = "Description",
  disabled = false,
  onCommit
}: {
  description: string | null;
  label?: string;
  disabled?: boolean;
  onCommit: (description: string | null) => Promise<unknown>;
}) {
  return (
    <label className="description-field">
      {label}
      <textarea
        key={description ?? ""}
        defaultValue={description ?? ""}
        disabled={disabled}
        rows={2}
        onBlur={(event) => {
          const next = event.currentTarget.value.trim();
          if (next === (description ?? "")) return;
          void onCommit(next === "" ? null : next);
        }}
      />
    </label>
  );
}

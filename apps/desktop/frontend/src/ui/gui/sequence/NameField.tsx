/** An object's name, which references to it use; renaming updates them. */
export function NameField({ name, label, commit }: { name: string; label: string; commit: (name: string) => Promise<unknown> }) {
  return (
    <label>
      {label}
      <input
        key={name}
        defaultValue={name}
        required
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
          if (event.key === "Escape") {
            event.stopPropagation();
            event.currentTarget.value = name;
            event.currentTarget.blur();
          }
        }}
        onBlur={(event) => {
          const input = event.currentTarget;
          const next = input.value.trim();
          if (next === "") {
            input.value = name;
            return;
          }
          if (next === name) return;
          void commit(next).catch(() => {
            input.value = name;
          });
        }}
      />
    </label>
  );
}

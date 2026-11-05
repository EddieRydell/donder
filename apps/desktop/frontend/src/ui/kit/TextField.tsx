import { useRef, useState, type KeyboardEvent } from "react";

type TextFieldProps = {
  value: string;
  onCommit: (value: string) => void;
  /** The accessible name. */
  name: string;
  placeholder?: string;
  multiline?: boolean;
  disabled?: boolean;
  invalid?: boolean;
  /** Commit every keystroke, for a filter that narrows a list as it is typed. */
  live?: boolean;
  autoFocus?: boolean;
  /** Hide what is typed, as for a password; the browser does not offer to fill it. */
  secret?: boolean;
  /** Escape was pressed: the edit was abandoned. */
  onCancel?: () => void;
  className?: string;
};

/** The app's text control: edits locally, commits on Enter or blur, Escape reverts. */
export function TextField({ value, onCommit, name, placeholder, multiline = false, disabled = false, invalid = false, live = false, autoFocus = false, secret = false, onCancel, className }: TextFieldProps) {
  const [draft, setDraft] = useState<string | null>(null);
  // Escape blurs the field before its reset renders, so the blur must know not to commit.
  const cancelled = useRef(false);
  const shown = draft ?? value;
  const finish = () => {
    if (!cancelled.current && draft !== null && draft !== value) onCommit(draft);
    cancelled.current = false;
    setDraft(null);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancelled.current = true;
      setDraft(null);
      event.currentTarget.blur();
      onCancel?.();
    } else if (event.key === "Enter" && !(multiline && event.shiftKey)) {
      event.preventDefault();
      event.currentTarget.blur();
    }
  };
  const props = {
    className: ["kit-text", className].filter(Boolean).join(" "),
    value: shown,
    placeholder,
    disabled,
    autoFocus,
    "aria-label": name,
    "aria-invalid": invalid,
    onChange: (event: { target: { value: string } }) => { if (live) onCommit(event.target.value); else setDraft(event.target.value); },
    onKeyDown,
    onBlur: finish
  };
  return multiline ? <textarea rows={2} {...props} /> : <input spellCheck={false} {...(secret ? { type: "password", autoComplete: "off" } : {})} {...props} />;
}

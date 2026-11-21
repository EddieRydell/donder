import { useCallback, useState, type ReactNode } from "react";
import { Button, Dialog } from "./kit";

/** `details` lists what else the action changes, a sentence each. */
export type ConfirmRequest = { title: string; description: string; details?: string[]; confirmLabel: string; danger: boolean };
export type Confirm = (request: ConfirmRequest) => Promise<boolean>;
type Pending = ConfirmRequest & { resolve: (confirmed: boolean) => void };

/**
 * The app's confirmation for actions undo cannot restore: `confirm` resolves with the answer, and
 * the caller renders `dialog`. Undoable actions never ask; an edit that changes other objects asks
 * through `EditConfirmationDialog`.
 */
export function useConfirmDialog(): { confirm: Confirm; dialog: ReactNode } {
  const [pending, setPending] = useState<Pending | null>(null);
  const confirm = useCallback<Confirm>((request) => new Promise((resolve) => { setPending({ ...request, resolve }); }), []);
  // A promise settles once, so the close that follows an answer cannot change it.
  const answer = (confirmed: boolean) => {
    pending?.resolve(confirmed);
    setPending(null);
  };
  const dialog = <Dialog
    open={pending !== null}
    onOpenChange={(open) => { if (!open) answer(false); }}
    title={pending?.title ?? ""}
    actions={<>
      <Button onClick={() => { answer(false); }}>Cancel</Button>
      <Button autoFocus variant={pending?.danger === true ? "danger" : "primary"} onClick={() => { answer(true); }}>{pending?.confirmLabel}</Button>
    </>}
  >
    <p>{pending?.description}</p>
    {pending?.details !== undefined && pending.details.length > 0 && <ul className="kit-change-list">{pending.details.map((detail) => <li key={detail}>{detail}</li>)}</ul>}
  </Dialog>;
  return { confirm, dialog };
}

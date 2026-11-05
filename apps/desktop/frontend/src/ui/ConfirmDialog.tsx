import { useCallback, useState, type ReactNode } from "react";
import { Button, Dialog } from "./kit";

export type ConfirmRequest = { title: string; description: string; confirmLabel: string; danger: boolean };
export type Confirm = (request: ConfirmRequest) => Promise<boolean>;
type Pending = ConfirmRequest & { resolve: (confirmed: boolean) => void };

/**
 * The app's confirmation for actions undo cannot restore: `confirm` resolves with the answer, and
 * the caller renders `dialog`. Undoable actions never ask.
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
  </Dialog>;
  return { confirm, dialog };
}

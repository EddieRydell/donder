import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { useCallback, useContext, useState, type ReactNode } from "react";
import { OverlayPortal } from "./OverlayPortal";

export type ConfirmRequest = { title: string; description: string; confirmLabel: string; danger: boolean };
export type Confirm = (request: ConfirmRequest) => Promise<boolean>;
type Pending = ConfirmRequest & { resolve: (confirmed: boolean) => void };

/** The app's confirmation dialog: `confirm` resolves with the answer, and the caller renders `dialog`. */
export function useConfirmDialog(): { confirm: Confirm; dialog: ReactNode } {
  const container = useContext(OverlayPortal);
  const [pending, setPending] = useState<Pending | null>(null);
  const confirm = useCallback<Confirm>((request) => new Promise((resolve) => { setPending({ ...request, resolve }); }), []);
  // A promise settles once, so the close that follows an answer cannot change it.
  const answer = (confirmed: boolean) => {
    pending?.resolve(confirmed);
    setPending(null);
  };
  const dialog = <AlertDialog.Root open={pending !== null} onOpenChange={(open) => { if (!open) answer(false); }}>
    <AlertDialog.Portal container={container}>
      <AlertDialog.Overlay className="dialog-overlay" />
      <AlertDialog.Content className="dialog-content">
        <AlertDialog.Title>{pending?.title}</AlertDialog.Title>
        <AlertDialog.Description>{pending?.description}</AlertDialog.Description>
        <div className="dialog-actions">
          <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
          <AlertDialog.Action className={pending?.danger === true ? "danger-button" : ""} onClick={() => { answer(true); }}>{pending?.confirmLabel}</AlertDialog.Action>
        </div>
      </AlertDialog.Content>
    </AlertDialog.Portal>
  </AlertDialog.Root>;
  return { confirm, dialog };
}

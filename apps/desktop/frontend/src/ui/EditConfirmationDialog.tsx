import { useAppStore } from "../store";
import { Button, Dialog } from "./kit";

/** Confirms a GUI edit that also removes or changes objects the person did not act on. */
export function EditConfirmationDialog() {
  const pending = useAppStore((store) => store.editConfirmation);
  return <Dialog
    open={pending !== null}
    onOpenChange={(open) => { if (!open) pending?.decide(false); }}
    title="Change other objects"
    actions={<>
      <Button onClick={() => pending?.decide(false)}>Cancel</Button>
      <Button autoFocus variant="danger" onClick={() => pending?.decide(true)}>Apply</Button>
    </>}
  >
    <ul className="kit-change-list">{pending?.changes.map((change) => <li key={change}>{change}</li>)}</ul>
  </Dialog>;
}

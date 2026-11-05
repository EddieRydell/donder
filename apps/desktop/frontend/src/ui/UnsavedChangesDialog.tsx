import { useTransitionStore } from "../workspaceTransitions";
import { Button, Dialog } from "./kit";

export function UnsavedChangesDialog() {
  const pending = useTransitionStore((store) => store.pending);
  return <Dialog
    open={pending !== null}
    onOpenChange={(open) => { if (!open) pending?.decide("cancel"); }}
    title="Unsaved changes"
    actions={<>
      <Button onClick={() => pending?.decide("cancel")}>Cancel</Button>
      <Button variant="danger" onClick={() => pending?.decide("discard")}>Discard</Button>
      <Button autoFocus variant="primary" onClick={() => pending?.decide("saveAll")}>Save All</Button>
    </>}
  >
    <ul className="kit-path-list">{pending?.paths.map((path) => <li key={path}>{path}</li>)}</ul>
  </Dialog>;
}

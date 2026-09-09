import { useState, type ReactNode } from "react";
import { useAppStore } from "../../../store";
import type { GuiDocumentRequest, GuiFixtureDefinition } from "../../../types";
import { FixtureDefinitionInput } from "../setup/FixtureProfileInputs";

export function FixtureProfileForm({ initial, onSave, children }: {
  initial: GuiFixtureDefinition;
  onSave: (definition: GuiFixtureDefinition, origin: GuiDocumentRequest) => Promise<void>;
  children?: ReactNode;
}) {
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const pending = useAppStore((state) => state.guiEditPending);
  const [origin, setOrigin] = useState(request);
  const [definition, setDefinition] = useState(() => structuredClone(initial));
  const [error, setError] = useState<string | null>(null);
  const stale = origin !== request;
  return <form className="setup-patch-editor" onSubmit={(event) => {
    event.preventDefault();
    if (origin === null) return;
    void onSave(definition, origin).then(() => {
      setOrigin(useAppStore.getState().guiRequest);
      setError(null);
    }).catch((error: unknown) => { setError(String(error)); });
  }}>
    {stale && <p role="alert">The project changed. Discard this draft to load the current definition.</p>}
    {error !== null && <p role="alert">{error}</p>}
    <fieldset disabled={stale || pending || request === null || revision !== request.projectRevision}>
      {children}
      <FixtureDefinitionInput value={definition} onChange={setDefinition} />
      <button type="submit">Save fixture profile</button>
    </fieldset>
    <button type="button" disabled={pending} onClick={() => { setDefinition(structuredClone(initial)); setOrigin(request); setError(null); }}>Discard draft</button>
  </form>;
}

import { useEffect, useState } from "react";
import { useSequenceEditorHost } from "../../../editor/host";
import type { SequenceEditorDocument, SequenceEffectDetails } from "../../../editor/types";

// A sequence document carries every clip's summary; the parameters the
// inspector edits are fetched for the clips it shows.

type LoadedDetails = { key: string; details: Map<number, SequenceEffectDetails> };

/** The details of `effectIds`, or `null` until they first arrive. After an
 *  edit the previous details stay until the new ones replace them. */
export function useSequenceEffectDetails(document: SequenceEditorDocument, effectIds: number[]): Map<number, SequenceEffectDetails> | null {
  const { commands, store: useAppStore } = useSequenceEditorHost();
  const projectRevision = useAppStore((store) => store.snapshot?.projectRevision ?? null);
  const setError = useAppStore((store) => store.setError);
  const key = effectIds.join(",");
  const [loaded, setLoaded] = useState<LoadedDetails | null>(null);
  const { ownedPath } = document.sourceRef;
  const { path, objectKey } = document;

  useEffect(() => {
    if (projectRevision === null || key === "") return;
    let cancelled = false;
    const ids = key.split(",").map(Number);
    commands
      .getSequenceEffectDetails({ ownedPath, projectRevision, path, view: "sequence", objectKey }, ids)
      .then((result) => {
        if (!cancelled) setLoaded({ key, details: new Map(result.details.map((details) => [details.id, details])) });
      })
      .catch((error: unknown) => {
        if (!cancelled) setError(String(error));
      });
    return () => {
      cancelled = true;
    };
  }, [commands, key, objectKey, ownedPath, path, projectRevision, setError]);

  if (key === "") return new Map();
  return loaded?.key === key ? loaded.details : null;
}

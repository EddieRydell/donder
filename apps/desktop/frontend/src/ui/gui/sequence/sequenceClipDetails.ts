import { useEffect, useState } from "react";
import { useSequenceEditorHost } from "../../../editor/host";
import type { SequenceGuiDocument, SequenceClipDetails } from "../../../editor/types";

// A sequence document carries every clip's summary; the parameters the
// inspector edits are fetched for the clips it shows.

type LoadedDetails = { key: string; details: Map<number, SequenceClipDetails> };

/** The details of `clipIds`, or `null` until they first arrive. After an
 *  edit the previous details stay until the new ones replace them. */
export function useSequenceClipDetails(document: SequenceGuiDocument, clipIds: number[]): Map<number, SequenceClipDetails> | null {
  const { commands, store: useAppStore } = useSequenceEditorHost();
  const projectRevision = useAppStore((store) => store.snapshot?.projectRevision ?? null);
  const setError = useAppStore((store) => store.setError);
  const key = clipIds.join(",");
  const [loaded, setLoaded] = useState<LoadedDetails | null>(null);
  const { ownedPath } = document.sourceRef;
  const { path, objectKey } = document;

  useEffect(() => {
    if (projectRevision === null || key === "") return;
    let cancelled = false;
    const ids = key.split(",").map(Number);
    commands
      .getSequenceClipDetails({ ownedPath, projectRevision, path, objectKey }, ids)
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

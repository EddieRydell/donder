import { commands, errorMessage } from "../../../api";
import { useAppStore } from "../../../store";
import { useEffect, useState } from "react";
import type { GuiObjectRef, SequenceClipDetails } from "../../../types";

// A sequence or console document carries every clip's summary; the
// parameters the inspector edits are fetched for the clips it shows.

type LoadedDetails = { key: string; details: Map<number, SequenceClipDetails> };

/** The details of `clipIds`, or `null` until they first arrive. After an
 *  edit the previous details stay until the new ones replace them. */
export function useSequenceClipDetails(document: { sourceRef: GuiObjectRef }, clipIds: number[]): Map<number, SequenceClipDetails> | null {
  const projectRevision = useAppStore((store) => store.snapshot?.projectRevision ?? null);
  const reportFailure = useAppStore((store) => store.reportFailure);
  const key = clipIds.join(",");
  const [loaded, setLoaded] = useState<LoadedDetails | null>(null);
  const { path, objectKey } = document.sourceRef;

  useEffect(() => {
    if (projectRevision === null || key === "") return;
    let cancelled = false;
    const ids = key.split(",").map(Number);
    commands
      .getSequenceClipDetails({ projectRevision, path, objectKey }, ids)
      .then((result) => {
        if (!cancelled) setLoaded({ key, details: new Map(result.details.map((details) => [details.id, details])) });
        reportFailure("command", null);
      })
      .catch((error: unknown) => {
        if (!cancelled) reportFailure("command", errorMessage(error));
      });
    return () => {
      cancelled = true;
    };
  }, [key, objectKey, path, projectRevision, reportFailure]);

  if (key === "") return new Map();
  return loaded?.key === key ? loaded.details : null;
}

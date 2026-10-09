import * as Dialog from "@radix-ui/react-dialog";
import { AudioWaveform } from "lucide-react";
import { useState } from "react";
import { useSequenceEditorHost } from "../../../editor/host";
import type { NewMarkCollection, SequenceEditorDocument, SequenceMarkCollection } from "../../../editor/types";
import { sameGuiDocument } from "../../../snapshotState";
import { THEME_METRICS } from "../../../theme";
import { defaultMarkColor, nextCollectionKey } from "./marks";

/**
 * Detects beats and bars in the sequence's audio and adds them as new mark collections in one
 * undoable edit. Shown only when the host can analyze audio.
 */
export function BeatDetectionDialog({
  document,
  setActiveMarkCollectionKey,
  visibleMarkCollectionKeys,
  setVisibleMarkCollectionKeys
}: {
  document: SequenceEditorDocument;
  setActiveMarkCollectionKey: (key: string | null) => void;
  visibleMarkCollectionKeys: Set<string>;
  setVisibleMarkCollectionKeys: (keys: Set<string>) => void;
}) {
  const host = useSequenceEditorHost();
  const { commands, runGuiEditCommand, detectSequenceBeats } = host;
  const request = host.store((state) => state.guiRequest);
  const [open, setOpen] = useState(false);
  const [beats, setBeats] = useState(true);
  const [bars, setBars] = useState(true);
  const [beatsName, setBeatsName] = useState("beats");
  const [barsName, setBarsName] = useState("bars");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (detectSequenceBeats === undefined) return null;
  const hasAudio = document.audio?.exists === true;
  const options = [
    { label: "Beats", enabled: beats, setEnabled: setBeats, name: beatsName, setName: setBeatsName },
    { label: "Bars (the first beat of each bar)", enabled: bars, setEnabled: setBars, name: barsName, setName: setBarsName }
  ];
  const chosen = options.filter((option) => option.enabled);
  const namesValid = chosen.every((option) => option.name.trim().length > 0);

  const detect = async () => {
    if (request === null) return;
    setPending(true);
    setError(null);
    try {
      const detection = await detectSequenceBeats(request);
      if (!sameGuiDocument(host.store.getState().guiRequest, request)) {
        throw new Error("The open sequence changed during beat detection.");
      }
      const wanted: [string, number[]][] = [];
      if (beats) wanted.push([beatsName, detection.beatsSeconds]);
      if (bars) wanted.push([barsName, detection.downbeatsSeconds]);
      const existing: Pick<SequenceMarkCollection, "key">[] = [...document.markCollections];
      const collections: NewMarkCollection[] = wanted.map(([name, marksSeconds]) => {
        const key = nextCollectionKey(name, existing);
        existing.push({ key });
        return { name: key, color: defaultMarkColor(existing.length - 1), marksSeconds };
      });
      await runGuiEditCommand((current) =>
        commands.applySequenceGuiEdit(current, { type: "createMarkCollections", collections })
      );
      const keys = collections.map((collection) => collection.name);
      setActiveMarkCollectionKey(keys[0] ?? null);
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, ...keys]));
      setOpen(false);
    } catch (detectError: unknown) {
      setError(String(detectError));
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog.Root open={open} onOpenChange={(next) => { if (!pending) { setOpen(next); setError(null); } }}>
      <Dialog.Trigger asChild>
        <button type="button" className="neutral-button icon-text-button">
          <AudioWaveform size={THEME_METRICS.iconSizeSmall} />
          Detect beats
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content beat-detection-dialog">
          <Dialog.Title>Detect beats</Dialog.Title>
          <Dialog.Description>
            Analyze the sequence&apos;s audio and add the beats and bars it finds as new mark collections. This takes several seconds.
          </Dialog.Description>
          {!hasAudio && <p role="alert">Choose the sequence&apos;s audio first.</p>}
          {error !== null && <p role="alert">{error}</p>}
          <fieldset className="beat-detection-options" disabled={pending || !hasAudio}>
            <legend>Collections to add</legend>
            {options.map((option) => (
              <div key={option.label} className="beat-detection-option">
                <label>
                  <input type="checkbox" checked={option.enabled} onChange={(event) => { option.setEnabled(event.currentTarget.checked); }} />
                  {option.label}
                </label>
                <input
                  type="text"
                  aria-label={`${option.label} collection name`}
                  value={option.name}
                  disabled={!option.enabled}
                  onChange={(event) => { option.setName(event.currentTarget.value); }}
                />
              </div>
            ))}
          </fieldset>
          <div className="dialog-actions">
            <button type="button" disabled={pending} onClick={() => { setOpen(false); }}>Cancel</button>
            <button type="button" disabled={pending || !hasAudio || request === null || chosen.length === 0 || !namesValid} onClick={() => { void detect(); }}>
              {pending ? "Analyzing audio…" : "Detect"}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

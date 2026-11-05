import { AudioWaveform } from "lucide-react";
import { useState } from "react";
import { useSequenceEditorHost } from "../../../editor/host";
import type { NewMarkCollection, SequenceGuiDocument } from "../../../editor/types";
import { sameGuiDocument } from "../../../snapshotState";
import { Button, Dialog, Field, TextField, Toggle } from "../../kit";
import { defaultMarkColor } from "./marks";

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
  document: SequenceGuiDocument;
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
    { label: "Beats", tip: "A mark collection of every beat", enabled: beats, setEnabled: setBeats, name: beatsName, setName: setBeatsName },
    { label: "Bars", tip: "A mark collection of the first beat of each bar", enabled: bars, setEnabled: setBars, name: barsName, setName: setBarsName }
  ];
  const chosen = options.filter((option) => option.enabled);
  const namesValid = chosen.every((option) => option.name.trim().length > 0);
  const invalid = !hasAudio ? "No audio" : chosen.length === 0 ? "Nothing to add" : !namesValid ? "Name required" : null;
  const problem = error ?? invalid;

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
      const collections: NewMarkCollection[] = wanted.map(([name, marksSeconds], index) =>
        ({ name, color: defaultMarkColor(document.markCollections.length + index), marksSeconds }));
      const { created: keys } = await runGuiEditCommand((current) =>
        commands.applyGuiEdit(current, { type: "sequence", edit: { type: "createMarkCollections", collections } })
      );
      setActiveMarkCollectionKey(keys[0] ?? null);
      setVisibleMarkCollectionKeys(new Set([...visibleMarkCollectionKeys, ...keys]));
      setOpen(false);
    } catch (detectError: unknown) {
      setError(String(detectError));
    } finally {
      setPending(false);
    }
  };

  return <>
    <Button icon={AudioWaveform} tip="Add the song's beats and bars as mark collections" onClick={() => { setError(null); setOpen(true); }}>Detect beats</Button>
    <Dialog
      open={open}
      onOpenChange={(next) => { if (!pending) { setOpen(next); setError(null); } }}
      title="Detect Beats"
      actions={<>
        <Button disabled={pending} onClick={() => { setOpen(false); }}>Cancel</Button>
        <Button variant="primary" tip="Analyzing takes a few seconds" disabled={pending || request === null || invalid !== null} onClick={() => { void detect(); }}>
          {pending ? "Analyzing…" : "Detect"}
        </Button>
      </>}
    >
      {options.map((option) => <Field key={option.label} label={option.label} tip={option.tip}>
        <Toggle name={`Add ${option.label.toLowerCase()}`} checked={option.enabled} disabled={pending || !hasAudio} onChange={option.setEnabled} />
        <TextField
          name={`${option.label} collection name`}
          value={option.name}
          disabled={pending || !hasAudio || !option.enabled}
          invalid={option.enabled && option.name.trim() === ""}
          onCommit={option.setName}
        />
      </Field>)}
      {problem !== null && <div className="kit-status-error" role="alert">{problem}</div>}
    </Dialog>
  </>;
}

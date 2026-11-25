import { Activity, Eye, Play, Plus, Trash2, X } from "lucide-react";
import { useMemo } from "react";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import type { ConsoleGuiDocument, SequenceClipDetails } from "../../../types";
import { ColorPicker } from "../../ColorPicker";
import { PerformButton } from "../../PerformBar";
import { AddRow, Badge, Button, Field, IconButton, NumberField, Tip } from "../../kit";
import { DescriptionField } from "../DescriptionField";
import { OBJECT, useFieldProblem } from "../problems";
import { clipFocus, focusedClipId, setDocumentUi, useDocumentUi } from "../documentUi";
import { GraphEditorModal } from "../sequence/GraphEditorModal";
import { NameField } from "../sequence/NameField";
import { EffectDefinitionPicker } from "../sequence/definitionMenu";
import { defaultMarkColor } from "../sequence/marks";
import { ConsoleControlGrid } from "./ConsoleControlGrid";
import { consoleComposition } from "./consoleComposition";
import { applyConsoleEdit, focusOnEffect, marksReaders } from "./consoleEditing";
import { useSequenceClipDetails } from "../sequence/sequenceClipDetails";
import { describe, put, recolor, remove, rename, site } from "../edits";

/** Who reads a live mark collection, as a count whose tip names them. */
export function MarksReadersBadge({ document, id }: { document: ConsoleGuiDocument; id: number }) {
  const details = useSequenceClipDetails(document, document.clips.map((clip) => clip.id)) ?? new Map<number, SequenceClipDetails>();
  const readers = marksReaders(document, details, id);
  return <Badge icon={Eye} tip={readers.length === 0 ? "No effect reads these marks" : `Read by ${readers.join(", ")}`}>{readers.length}</Badge>;
}

/** The console's frame rate, marked when a rate is refused. */
function FrameRateField({ frameRate }: { frameRate: number }) {
  const problem = useFieldProblem(OBJECT, { type: "frameRate" });
  return <Field label="Frame rate" error={problem}>
    <NumberField name="Frame rate" value={frameRate} step={1} min={1} integer unit="fps" invalid={problem !== null} onCommit={(next) => void put(site(OBJECT, { type: "frameRate" }), { type: "count", count: next })} />
  </Field>;
}

export function ConsoleEditor({ document }: { document: ConsoleGuiDocument }) {
  const composition = useMemo(() => consoleComposition(document), [document]);
  const graphOpen = useDocumentUi((ui) => ui.graphOpen);
  const focused = focusedClipId(useDocumentUi((ui) => ui.focus));
  const envelopes = document.clips.filter((clip) => clip.content.type === "envelope");
  const target = document.targets[0];
  const performIndex = document.performIndex;
  const iconSize = THEME_METRICS.iconSizeSmall;
  const addLiveMarks = () => void applyConsoleEdit({ type: "composition", edit: { type: "addMarkCollections", collections: [{ name: "Live marks", color: defaultMarkColor(document.markCollections.length), marksSeconds: [] }] } });
  return <main className="object-editor console-editor">
    <header className="object-overview-header">
      <div><span className="object-overview-eyebrow">Console</span><h2>{document.name}</h2></div>
      <div className="object-overview-header-actions">
        <FrameRateField frameRate={document.frameRate} />
        <PerformButton index={performIndex} />
      </div>
    </header>
    <div className="console-sections">
      <section className="console-section">
        <div className="object-overview-group-header">
          <Tip label="Plays from the moment you perform"><h3>Always playing</h3></Tip>
          <EffectDefinitionPicker definitions={document.effectDefinitions} label="Add effect" disabled={target === undefined} onSelect={(definition) => {
            if (target !== undefined) void applyConsoleEdit({ type: "addAlwaysEffect", initialColor: THEME_COLORS.white, effect: definition.effect, target: target.target })
              .then((update) => { if (update !== null) setDocumentUi({ focus: focusOnEffect(update, update.created[0]) }); });
          }} />
        </div>
        <div className="chip-list">
          {document.always.map((id) => {
            const clip = document.clips.find((candidate) => candidate.id === id);
            if (clip === undefined) return null;
            const isSelected = focused === id;
            return <span key={id} className={`action-chip${isSelected ? " selected" : ""}`}>
              <Button variant="ghost" className="action-chip-body" aria-pressed={isSelected} onClick={() => { setDocumentUi({ focus: clipFocus(id) }); }}><Play size={iconSize} aria-hidden /><span>{clip.name}</span></Button>
              <IconButton icon={X} label="Delete effect" danger className="action-chip-remove" onClick={() => {
                void remove(site({ type: "clip", id })).then((update) => { if (update !== null && isSelected) setDocumentUi({ focus: null }); });
              }} />
            </span>;
          })}
        </div>
      </section>

      <section className="console-section">
        <h3>Controls</h3>
        <ConsoleControlGrid document={document} />
      </section>

      {envelopes.length > 0 && <section className="console-section">
        <Tip label="Play on the session's clock from the moment you perform"><h3>Envelopes</h3></Tip>
        <div className="chip-list">
          {envelopes.map((clip) => <span key={clip.id} className={`action-chip${focused === clip.id ? " selected" : ""}`}>
            <Button variant="ghost" className="action-chip-body" aria-pressed={focused === clip.id} onClick={() => { setDocumentUi({ focus: clipFocus(clip.id) }); }}><Activity size={iconSize} aria-hidden /><span>{clip.name}</span></Button>
          </span>)}
        </div>
      </section>}

      <section className="console-section">
        <div className="object-overview-group-header">
          <Tip label="Each session starts them empty; mark actions add to them, and marks parameters follow them"><h3>Live marks</h3></Tip>
          {document.markCollections.length > 0 && <IconButton icon={Plus} label="Add live marks" onClick={addLiveMarks} />}
        </div>
        {document.markCollections.length === 0 && <AddRow label="Add live marks" onAdd={addLiveMarks} />}
        {document.markCollections.map((marks) => {
          const member = { type: "marks", id: marks.id } as const;
          return <div className="console-item" key={marks.id}>
            <div className="inspector-inline-row">
              <ColorPicker value={marks.color} label={`${marks.name} color`} commit={(color) => recolor(member, color).then(() => undefined)} />
              <NameField name={marks.name} label="Name" member={member} commit={(name) => rename(member, name)} />
              <MarksReadersBadge document={document} id={marks.id} />
              <IconButton icon={Trash2} label="Delete live marks" danger onClick={() => void remove(site(member))} />
            </div>
            <DescriptionField description={marks.description} member={member} onCommit={(description) => describe(member, description)} />
          </div>;
        })}
      </section>

    </div>
    {graphOpen && <GraphEditorModal composition={composition} description="Console layers and operators" />}
  </main>;
}

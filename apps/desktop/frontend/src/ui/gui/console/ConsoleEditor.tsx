import { Eye, Play, Plus, Trash2, X } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { commands } from "../../../api";
import { startPerforming, usePerformStore } from "../../../perform";
import { runSnapshotCommand, useAppStore } from "../../../store";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import type { ConsoleGuiDocument } from "../../../types";
import { ColorPicker } from "../../ColorPicker";
import { AddRow, Badge, Button, Field, IconButton, NumberField, Tip } from "../../kit";
import { DescriptionField } from "../DescriptionField";
import type { GuiFocus } from "../shared";
import { GraphEditorModal } from "../sequence/GraphEditorModal";
import { NameField } from "../sequence/NameField";
import { EffectDefinitionPicker } from "../sequence/definitionMenu";
import { defaultMarkColor } from "../sequence/marks";
import { ConsoleControlGrid } from "./ConsoleControlGrid";
import { consoleComposition } from "./consoleComposition";
import { applyConsoleEdit, applyConsoleItemEdit, focusOnEffect, marksReaders } from "./consoleEditing";

/** Who reads a live mark collection, as a count whose tip names them. */
export function MarksReadersBadge({ readers }: { readers: string[] }) {
  return <Badge icon={Eye} tip={readers.length === 0 ? "No effect reads these marks" : `Read by ${readers.join(", ")}`}>{readers.length}</Badge>;
}

export function ConsoleEditor({ document, selected, setSelected }: {
  document: ConsoleGuiDocument;
  selected: GuiFocus;
  setSelected: (focus: GuiFocus) => void;
}) {
  const composition = useMemo(() => consoleComposition(document), [document]);
  const graphOpen = useAppStore((state) => state.compositionGraphEditing);
  const setGraphOpen = useAppStore((state) => state.setCompositionGraphEditing);
  const [, setSelectedGraphItem] = useState<unknown>(null);
  const performing = usePerformStore((state) => state.perform?.session !== null && state.perform?.session !== undefined);
  const closeGraph = useCallback(() => {
    setGraphOpen(false);
    void runSnapshotCommand(commands.finishCompositionGraphEditing);
  }, [setGraphOpen]);
  const target = document.targets[0];
  const performIndex = document.performIndex;
  const iconSize = THEME_METRICS.iconSizeSmall;
  const addLiveMarks = () => void applyConsoleEdit({ type: "addLiveMarks", name: "Live marks", color: defaultMarkColor(document.liveMarks.length) });
  return <main className="setup-editor console-editor">
    <header className="object-overview-header">
      <div><span className="object-overview-eyebrow">Console · {document.sourceRef.path}</span><h2>{document.name}</h2></div>
      <div className="object-overview-header-actions">
        <Field label="Frame rate">
          <NumberField name="Frame rate" value={document.frameRate} step={1} min={1} integer unit="fps" onCommit={(frameRate) => void applyConsoleEdit({ type: "setFrameRate", frameRate })} />
        </Field>
        {performIndex !== null && <Button icon={Play} disabled={performing} onClick={() => { startPerforming(performIndex); }}>Perform</Button>}
      </div>
    </header>
    <div className="console-sections">
      <section className="console-section">
        <div className="object-overview-group-header">
          <Tip label="Plays from the moment you perform"><h3>Always playing</h3></Tip>
          <EffectDefinitionPicker definitions={document.effectDefinitions} label="Add effect" disabled={target === undefined} onSelect={(definition) => {
            if (target !== undefined) void applyConsoleEdit({ type: "addAlwaysEffect", initialColor: THEME_COLORS.white, effect: definition.effect, target: target.target })
              .then((update) => { setSelected(focusOnEffect(update, update.created[0])); });
          }} />
        </div>
        <div className="chip-list">
          {document.always.map((id) => {
            const clip = document.clips.find((candidate) => candidate.id === id);
            if (clip === undefined) return null;
            const isSelected = selected?.type === "consoleEffect" && selected.id === id;
            return <span key={id} className={`action-chip${isSelected ? " selected" : ""}`}>
              <Button variant="ghost" className="action-chip-body" aria-pressed={isSelected} onClick={() => { setSelected({ type: "consoleEffect", id }); }}><Play size={iconSize} aria-hidden /><span>{clip.name}</span></Button>
              <IconButton icon={X} label="Delete effect" danger className="action-chip-remove" onClick={() => {
                void applyConsoleItemEdit({ type: "delete", item: { type: "clip", id } }).then(() => { if (isSelected) setSelected(null); });
              }} />
            </span>;
          })}
        </div>
      </section>

      <section className="console-section">
        <h3>Controls</h3>
        <ConsoleControlGrid document={document} selected={selected} setSelected={setSelected} />
      </section>

      <section className="console-section">
        <div className="object-overview-group-header">
          <Tip label="Each session starts them empty; mark actions add to them, and marks parameters follow them"><h3>Live marks</h3></Tip>
          {document.liveMarks.length > 0 && <IconButton icon={Plus} label="Add live marks" onClick={addLiveMarks} />}
        </div>
        {document.liveMarks.length === 0 && <AddRow label="Add live marks" onAdd={addLiveMarks} />}
        {document.liveMarks.map((marks) => {
          const item = { type: "markCollection", key: marks.key } as const;
          return <div className="console-item" key={marks.key}>
            <div className="inspector-inline-row">
              <ColorPicker value={marks.color} label={`${marks.key} color`} commit={(color) => applyConsoleItemEdit({ type: "setColor", item, color })} />
              <NameField name={marks.key} label="Name" commit={(name) => applyConsoleItemEdit({ type: "rename", item, name })} />
              <MarksReadersBadge readers={marksReaders(document, marks.key)} />
              <IconButton icon={Trash2} label="Delete live marks" danger onClick={() => void applyConsoleItemEdit({ type: "delete", item })} />
            </div>
            <DescriptionField description={marks.description} onCommit={(description) => applyConsoleItemEdit({ type: "setDescription", item, description })} />
          </div>;
        })}
      </section>

    </div>
    {graphOpen && <GraphEditorModal composition={composition} description="Console layers and operators" setSelectedItem={setSelectedGraphItem} onClose={closeGraph} />}
  </main>;
}

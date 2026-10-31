import { Play, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { commands } from "../../../api";
import { startPerforming, usePerformStore } from "../../../perform";
import { runSnapshotCommand, useAppStore } from "../../../store";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import type { ConsoleGuiDocument } from "../../../types";
import { ColorPicker } from "../../ColorPicker";
import { DescriptionField } from "../DescriptionField";
import type { GuiFocus } from "../shared";
import { GraphEditorModal } from "../sequence/GraphEditorModal";
import { NameField } from "../sequence/NameField";
import { EffectDefinitionPicker } from "../sequence/definitionMenu";
import { defaultMarkColor } from "../sequence/marks";
import { ConsoleControlGrid } from "./ConsoleControlGrid";
import { applyConsoleEdit, applyConsoleItemEdit, consoleComposition, focusOnEffect, marksReaders } from "./consoleEditing";

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
  useEffect(() => () => { setGraphOpen(false); }, [setGraphOpen]);
  const target = document.targets[0];
  const performIndex = document.performIndex;
  const iconSize = THEME_METRICS.iconSizeSmall;
  return <main className="setup-editor">
    <header className="object-overview-header">
      <div><span className="object-overview-eyebrow">Console · {document.path}</span><h2>{document.name}</h2></div>
      {performIndex !== null && <button type="button" disabled={performing} onClick={() => { startPerforming(performIndex); }}><Play aria-hidden="true" /> Perform</button>}
    </header>
    <div className="setup-sections">
      <section className="setup-section">
        <div className="object-overview-group-header"><h3>Always playing</h3>
          <EffectDefinitionPicker definitions={document.effectDefinitions} label="Add effect" disabled={target === undefined} onSelect={(definition) => {
            if (target !== undefined) void applyConsoleEdit({ type: "addAlwaysEffect", initialColor: THEME_COLORS.white, effect: definition.effect, target: target.target })
              .then((update) => { setSelected(focusOnEffect(update, update.created[0])); });
          }} />
        </div>
        <div className="chip-list">
          {document.always.length === 0 && <p className="object-overview-empty">Nothing plays on its own. Effects added here play from the moment you perform.</p>}
          {document.always.map((id) => {
            const clip = document.clips.find((candidate) => candidate.id === id);
            if (clip === undefined) return null;
            const isSelected = selected?.type === "consoleEffect" && selected.id === id;
            return <span key={id} className={`action-chip${isSelected ? " selected" : ""}`}>
              <button type="button" className="action-chip-body" aria-pressed={isSelected} onClick={() => { setSelected({ type: "consoleEffect", id }); }}><Play size={iconSize} aria-hidden /><span>{clip.name}</span></button>
              <button type="button" className="action-chip-remove" aria-label="Delete effect" title="Delete effect" onClick={() => {
                void applyConsoleItemEdit({ type: "delete", item: { type: "clip", id } }).then(() => { if (isSelected) setSelected(null); });
              }}><X size={iconSize} aria-hidden /></button>
            </span>;
          })}
        </div>
      </section>

      <section className="setup-section">
        <h3>Controls</h3>
        <ConsoleControlGrid document={document} selected={selected} setSelected={setSelected} />
      </section>

      <section className="setup-section">
        <div className="object-overview-group-header"><h3>Live marks</h3>
          <button type="button" onClick={() => void applyConsoleEdit({ type: "addLiveMarks", name: "Live marks", color: defaultMarkColor(document.liveMarks.length) })}>Add live marks</button>
        </div>
        <p className="setup-section-intro">Each session starts them empty; mark actions add to them, and marks parameters follow them.</p>
        {document.liveMarks.map((marks) => {
          const item = { type: "markCollection", key: marks.key } as const;
          const readers = marksReaders(document, marks.key);
          return <div className="console-item" key={marks.key}>
            <div className="inspector-inline-row">
              <ColorPicker value={marks.color} label={`${marks.key} color`} commit={(color) => applyConsoleItemEdit({ type: "setColor", item, color })} />
              <NameField name={marks.key} label="Name" commit={(name) => applyConsoleItemEdit({ type: "rename", item, name })} />
              <button type="button" onClick={() => void applyConsoleItemEdit({ type: "delete", item })}>Delete</button>
            </div>
            <DescriptionField description={marks.description} onCommit={(description) => applyConsoleItemEdit({ type: "setDescription", item, description })} />
            <p className="setup-section-intro">{readers.length === 0 ? "No effect reads these marks." : `Read by ${readers.join(", ")}.`}</p>
          </div>;
        })}
      </section>

      <details className="setup-section">
        <summary><h3>Settings</h3></summary>
        <div className="inspector-inline-row">
          <label>Frame rate<input key={document.frameRate} type="number" min={1} step={1} defaultValue={document.frameRate} onBlur={(event) => {
            const frameRate = Number(event.currentTarget.value);
            if (frameRate !== document.frameRate) void applyConsoleEdit({ type: "setFrameRate", frameRate });
          }} /></label>
          <button type="button" onClick={() => { setGraphOpen(true); }}>Edit layers and operators</button>
        </div>
      </details>
    </div>
    {graphOpen && <GraphEditorModal composition={composition} description="Console layers and operators" setSelectedItem={setSelectedGraphItem} onClose={closeGraph} />}
  </main>;
}

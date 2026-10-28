import { Play } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { commands } from "../../../api";
import { startPerforming, usePerformStore } from "../../../perform";
import { runSnapshotCommand, useAppStore } from "../../../store";
import { THEME_COLORS } from "../../../theme";
import type { RigGuiDocument } from "../../../types";
import { ColorPicker } from "../../ColorPicker";
import { DescriptionField } from "../DescriptionField";
import { GraphEditorModal } from "../sequence/GraphEditorModal";
import { NameField } from "../sequence/NameField";
import { EffectDefinitionPicker } from "../sequence/definitionMenu";
import { defaultMarkColor } from "../sequence/marks";
import { LayerList } from "../sequence/sequenceLayers";
import { RigEffectFields, RigInputPicker } from "./RigFields";
import { RigMappingCard } from "./RigMappingCard";
import { applyRigEdit, applyRigItemEdit, firstInput, rigComposition } from "./rigEditing";

export function RigEditor({ document }: { document: RigGuiDocument }) {
  const composition = useMemo(() => rigComposition(document), [document]);
  const graphOpen = useAppStore((state) => state.compositionGraphEditing);
  const setGraphOpen = useAppStore((state) => state.setCompositionGraphEditing);
  const [, setSelectedGraphItem] = useState<unknown>(null);
  const performing = usePerformStore((state) => state.perform?.session !== null && state.perform?.session !== undefined);
  const closeGraph = useCallback(() => {
    setGraphOpen(false);
    void runSnapshotCommand(commands.finishCompositionGraphEditing);
  }, [setGraphOpen]);
  useEffect(() => () => { setGraphOpen(false); }, [setGraphOpen]);
  const button = firstInput(document.inputDevices, "switch");
  const played = firstInput(document.inputDevices, "trigger");
  const target = document.targets[0];
  const performIndex = document.performIndex;
  return <main className="setup-editor">
    <header className="object-overview-header">
      <div><span className="object-overview-eyebrow">Rig · {document.path}</span><h2>{document.name}</h2></div>
      {performIndex !== null && <button type="button" disabled={performing} onClick={() => { startPerforming(performIndex); }}><Play aria-hidden="true" /> Perform</button>}
    </header>
    <div className="setup-sections">
      <section className="setup-section">
        <h3>Settings</h3>
        <div className="inspector-inline-row">
          <label>Frame rate<input key={document.frameRate} type="number" min={1} step={1} defaultValue={document.frameRate} onBlur={(event) => {
            const frameRate = Number(event.currentTarget.value);
            if (frameRate !== document.frameRate) void applyRigEdit({ type: "setFrameRate", frameRate });
          }} /></label>
          <button type="button" onClick={() => { setGraphOpen(true); }}>Edit composition graph</button>
        </div>
      </section>

      <section className="setup-section">
        <h3>Layers</h3>
        <LayerList composition={composition} />
      </section>

      <section className="setup-section">
        <div className="object-overview-group-header"><h3>Live marks</h3>
          <button type="button" onClick={() => void applyRigEdit({ type: "addLiveMarks", name: "Live marks", color: defaultMarkColor(document.liveMarks.length) })}>Add live marks</button>
        </div>
        <p className="setup-section-intro">Each session starts them empty; mark mappings add to them, and marks parameters follow them.</p>
        {document.liveMarks.map((marks) => {
          const item = { type: "markCollection", key: marks.key } as const;
          return <div className="rig-item" key={marks.key}>
            <div className="inspector-inline-row">
              <ColorPicker value={marks.color} label={`${marks.key} color`} commit={(color) => applyRigItemEdit({ type: "setColor", item, color })} />
              <NameField name={marks.key} label="Name" commit={(name) => applyRigItemEdit({ type: "rename", item, name })} />
              <button type="button" onClick={() => void applyRigItemEdit({ type: "delete", item })}>Delete</button>
            </div>
            <DescriptionField description={marks.description} onCommit={(description) => applyRigItemEdit({ type: "setDescription", item, description })} />
          </div>;
        })}
      </section>

      <section className="setup-section">
        <div className="object-overview-group-header"><h3>Scene</h3>
          <EffectDefinitionPicker definitions={document.effectDefinitions} label="Add voice" disabled={target === undefined} onSelect={(definition) => {
            if (target !== undefined) void applyRigEdit({ type: "addVoice", initialColor: THEME_COLORS.white, effect: definition.effect, target: target.target });
          }} />
        </div>
        <p className="setup-section-intro">Voices play for the whole session.</p>
        {document.voices.map((voice) => {
          const item = { type: "voice", id: voice.id } as const;
          return <div className="rig-item" key={voice.id}>
            <div className="inspector-inline-row">
              <NameField name={voice.name} label="Name" commit={(name) => applyRigItemEdit({ type: "rename", item, name })} />
              <button type="button" onClick={() => void applyRigItemEdit({ type: "delete", item })}>Delete voice</button>
            </div>
            <DescriptionField description={voice.description} onCommit={(description) => applyRigItemEdit({ type: "setDescription", item, description })} />
            <RigEffectFields document={document} owner={item} effect={voice.effect} />
          </div>;
        })}
      </section>

      <section className="setup-section">
        <div className="object-overview-group-header"><h3>Switches</h3>
          <button type="button" disabled={button === null} onClick={() => { if (button !== null) void applyRigEdit({ type: "addSwitch", input: button }); }}>Add switch</button>
        </div>
        <p className="setup-section-intro">Each press toggles a switch; every session starts with it off.</p>
        {document.switches.map((item) => {
          const reference = { type: "switch", id: item.id } as const;
          return <div className="rig-item" key={item.id}>
            <div className="inspector-inline-row">
              <NameField name={item.name} label="Name" commit={(name) => applyRigItemEdit({ type: "rename", item: reference, name })} />
              <button type="button" onClick={() => void applyRigItemEdit({ type: "delete", item: reference })}>Delete switch</button>
            </div>
            <DescriptionField description={item.description} onCommit={(description) => applyRigItemEdit({ type: "setDescription", item: reference, description })} />
            <RigInputPicker document={document} input={item.input} use="switch" onChange={(input) => void applyRigEdit({ type: "setSwitchInput", id: item.id, input })} />
          </div>;
        })}
      </section>

      <section className="setup-section">
        <div className="object-overview-group-header"><h3>Mappings</h3>
          <EffectDefinitionPicker definitions={document.effectDefinitions} label="Add mapping" disabled={played === null || target === undefined} onSelect={(definition) => {
            if (played !== null && target !== undefined) void applyRigEdit({ type: "addMapping", input: played, action: { type: "trigger", initialColor: THEME_COLORS.white, effect: definition.effect, target: target.target } });
          }} />
        </div>
        {played === null && <p className="object-overview-empty">Add an input device with buttons or keys to the project's setup to play this rig.</p>}
        {document.mappings.map((mapping) => <RigMappingCard key={mapping.id} document={document} mapping={mapping} />)}
      </section>
    </div>
    {graphOpen && <GraphEditorModal composition={composition} description="Rig layers and operators" setSelectedItem={setSelectedGraphItem} onClose={closeGraph} />}
  </main>;
}

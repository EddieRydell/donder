import { Play } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { commands } from "../../../api";
import { startPerforming, usePerformStore } from "../../../perform";
import { runSnapshotCommand, useAppStore } from "../../../store";
import { THEME_COLORS } from "../../../theme";
import type { ConsoleAutomation, ConsoleGuiDocument, SequenceAutomationTarget } from "../../../types";
import { automationTargetsEqual } from "../shared";
import { ColorPicker } from "../../ColorPicker";
import { DescriptionField } from "../DescriptionField";
import { GraphEditorModal } from "../sequence/GraphEditorModal";
import { NameField } from "../sequence/NameField";
import { EffectDefinitionPicker } from "../sequence/definitionMenu";
import { defaultMarkColor } from "../sequence/marks";
import { LayerList } from "../sequence/sequenceLayers";
import { ConsoleClipFields } from "./ConsoleFields";
import { ConsoleEventCard } from "./ConsoleEventCard";
import { applyConsoleEdit, applyConsoleItemEdit, clipUse, consoleComposition, firstControl } from "./consoleEditing";

export function ConsoleEditor({ document }: { document: ConsoleGuiDocument }) {
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
  const button = firstControl(document.inputDevices, false);
  const axis = firstControl(document.inputDevices, true);
  const target = document.targets[0];
  const performIndex = document.performIndex;
  return <main className="setup-editor">
    <header className="object-overview-header">
      <div><span className="object-overview-eyebrow">Console Â· {document.path}</span><h2>{document.name}</h2></div>
      {performIndex !== null && <button type="button" disabled={performing} onClick={() => { startPerforming(performIndex); }}><Play aria-hidden="true" /> Perform</button>}
    </header>
    <div className="setup-sections">
      <section className="setup-section">
        <h3>Settings</h3>
        <div className="inspector-inline-row">
          <label>Frame rate<input key={document.frameRate} type="number" min={1} step={1} defaultValue={document.frameRate} onBlur={(event) => {
            const frameRate = Number(event.currentTarget.value);
            if (frameRate !== document.frameRate) void applyConsoleEdit({ type: "setFrameRate", frameRate });
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
          <button type="button" onClick={() => void applyConsoleEdit({ type: "addLiveMarks", name: "Live marks", color: defaultMarkColor(document.liveMarks.length) })}>Add live marks</button>
        </div>
        <p className="setup-section-intro">Each session starts them empty; mark actions add to them, and marks parameters follow them.</p>
        {document.liveMarks.map((marks) => {
          const item = { type: "markCollection", key: marks.key } as const;
          return <div className="console-item" key={marks.key}>
            <div className="inspector-inline-row">
              <ColorPicker value={marks.color} label={`${marks.key} color`} commit={(color) => applyConsoleItemEdit({ type: "setColor", item, color })} />
              <NameField name={marks.key} label="Name" commit={(name) => applyConsoleItemEdit({ type: "rename", item, name })} />
              <button type="button" onClick={() => void applyConsoleItemEdit({ type: "delete", item })}>Delete</button>
            </div>
            <DescriptionField description={marks.description} onCommit={(description) => applyConsoleItemEdit({ type: "setDescription", item, description })} />
          </div>;
        })}
      </section>

      <section className="setup-section">
        <div className="object-overview-group-header"><h3>Live automations</h3>
          <button type="button" onClick={() => void applyConsoleEdit({ type: "addAutomation", name: "Automation" })}>Add live automation</button>
        </div>
        <p className="setup-section-intro">Set actions write a value from 0 to 1, which each bound parameter follows across its range.</p>
        {document.automations.map((automation) => <AutomationItem key={automation.id} document={document} automation={automation} />)}
      </section>

      <section className="setup-section">
        <div className="object-overview-group-header"><h3>Clips</h3>
          <EffectDefinitionPicker definitions={document.effectDefinitions} label="Add clip" disabled={target === undefined} onSelect={(definition) => {
            if (target !== undefined) void applyConsoleEdit({ type: "addClip", initialColor: THEME_COLORS.white, effect: definition.effect, target: target.target });
          }} />
        </div>
        <p className="setup-section-intro">Actions play clips. A clip is played in copies by Play actions, or held by Start, Toggle and playing for the whole session.</p>
        {document.clips.map((clip) => {
          const item = { type: "clip", id: clip.id } as const;
          const always = document.always.includes(clip.id);
          return <div className="console-item" key={clip.id}>
            <div className="inspector-inline-row">
              <NameField name={clip.name} label="Name" commit={(name) => applyConsoleItemEdit({ type: "rename", item, name })} />
              <label><input type="checkbox" checked={always} disabled={!always && clipUse(document, clip.id) === "played"} onChange={() => void applyConsoleEdit({
                type: "setAlways",
                clipIds: always ? document.always.filter((id) => id !== clip.id) : [...document.always, clip.id]
              })} /> Plays for the whole session</label>
              <button type="button" onClick={() => void applyConsoleItemEdit({ type: "delete", item })}>Delete clip</button>
            </div>
            <DescriptionField description={clip.description} onCommit={(description) => applyConsoleItemEdit({ type: "setDescription", item, description })} />
            <ConsoleClipFields document={document} clip={clip} />
          </div>;
        })}
      </section>

      <section className="setup-section">
        <div className="object-overview-group-header"><h3>Events</h3>
          <button type="button" disabled={button === null && axis === null} onClick={() => {
            if (button !== null) void applyConsoleEdit({ type: "addEvent", event: "press", input: button });
            else if (axis !== null) void applyConsoleEdit({ type: "addEvent", event: "move", input: axis });
          }}>Add event</button>
        </div>
        {button === null && axis === null && <p className="object-overview-empty">Add an input device with buttons or axes to the project's setup to play this console.</p>}
        {document.events.map((on) => <ConsoleEventCard key={on.id} document={document} on={on} />)}
      </section>
    </div>
    {graphOpen && <GraphEditorModal composition={composition} description="Console layers and operators" setSelectedItem={setSelectedGraphItem} onClose={closeGraph} />}
  </main>;
}

/** A parameter a live automation can set, and how the picker names it. */
type BindingChoice = { target: SequenceAutomationTarget; label: string };

/** The automatable parameters of the console's clips, then of its graph's operator nodes. */
function bindingChoices(document: ConsoleGuiDocument): { clips: BindingChoice[]; nodes: BindingChoice[] } {
  const clips = document.clips.flatMap((clip) => clip.params
    .filter((param) => param.supportsAutomation)
    .map((param): BindingChoice => ({ target: { type: "clipParam", clipId: clip.id, param: param.name }, label: `${clip.name}.${param.name}` })));
  const nodes = document.compositionGraph.nodes.flatMap((node) => {
    const kind = node.kind;
    if (kind.type !== "operator") return [];
    return kind.params
      .filter((param) => param.supportsAutomation)
      .map((param): BindingChoice => ({ target: { type: "compositionNodeParam", nodeId: node.id, param: param.name }, label: `${kind.name}.${param.name}` }));
  });
  return { clips, nodes };
}

function bindingKey(target: SequenceAutomationTarget) {
  return target.type === "clipParam" ? `clip:${target.clipId}:${target.param}` : `${target.nodeId}:${target.param}`;
}

/** A live automation: its name, description, starting value and the clip and operator node parameters it sets. */
function AutomationItem({ document, automation }: { document: ConsoleGuiDocument; automation: ConsoleAutomation }) {
  const item = { type: "automation", id: automation.id } as const;
  // A parameter that actions play, or another automation sets, cannot be bound.
  const bound = (target: SequenceAutomationTarget) => document.automations.some((other) => other.bindings.some((candidate) => automationTargetsEqual(candidate, target)));
  const played = (target: SequenceAutomationTarget) => target.type === "clipParam" && document.events.some((on) => on.actions.some((action) =>
    (action.type === "play" || action.type === "start" || action.type === "toggle") && action.clipId === target.clipId && action.values.some((value) => value.param === target.param)));
  const groups = bindingChoices(document);
  const choices = [...groups.clips, ...groups.nodes];
  const free = choices.find((choice) => !bound(choice.target) && !played(choice.target));
  const setBindings = (bindings: SequenceAutomationTarget[]) => void applyConsoleEdit({ type: "setAutomationBindings", id: automation.id, bindings });
  const options = (group: BindingChoice[], binding: SequenceAutomationTarget) => group.map((choice) =>
    <option key={bindingKey(choice.target)} value={bindingKey(choice.target)} disabled={bindingKey(choice.target) !== bindingKey(binding) && (bound(choice.target) || played(choice.target))}>{choice.label}</option>);
  return <div className="console-item">
    <div className="inspector-inline-row">
      <NameField name={automation.name} label="Name" commit={(name) => applyConsoleItemEdit({ type: "rename", item, name })} />
      <label>Starting value<input key={automation.value} type="number" min={0} max={1} step="any" defaultValue={automation.value} onBlur={(event) => {
        const value = Number(event.currentTarget.value);
        if (value !== automation.value) void applyConsoleEdit({ type: "setAutomationValue", id: automation.id, value });
      }} /></label>
      <button type="button" onClick={() => void applyConsoleItemEdit({ type: "delete", item })}>Delete</button>
    </div>
    <DescriptionField description={automation.description} onCommit={(description) => applyConsoleItemEdit({ type: "setDescription", item, description })} />
    {automation.bindings.map((binding, index) => <div key={bindingKey(binding)} className="inspector-inline-row">
      <label>Sets<select value={bindingKey(binding)} onChange={(event) => {
        const choice = choices.find((candidate) => bindingKey(candidate.target) === event.currentTarget.value);
        if (choice !== undefined) setBindings(automation.bindings.map((other, i) => i === index ? choice.target : other));
      }}>
        {!choices.some((choice) => bindingKey(choice.target) === bindingKey(binding)) && <option value={bindingKey(binding)} disabled>{binding.param}</option>}
        {groups.clips.length > 0 && <optgroup label="Clips">{options(groups.clips, binding)}</optgroup>}
        {groups.nodes.length > 0 && <optgroup label="Graph operators">{options(groups.nodes, binding)}</optgroup>}
      </select></label>
      <button type="button" onClick={() => { setBindings(automation.bindings.filter((_, i) => i !== index)); }}>Remove</button>
    </div>)}
    <button type="button" disabled={free === undefined} onClick={() => {
      if (free !== undefined) setBindings([...automation.bindings, free.target]);
    }}>Add parameter</button>
  </div>;
}

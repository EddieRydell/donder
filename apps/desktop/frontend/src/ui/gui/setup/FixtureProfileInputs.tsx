import type { GuiFixtureBehavior, GuiFixtureChannelRole, GuiFixtureDefinition, GuiFixtureEntry, GuiFixtureFunction, GuiFixtureFunctionKind } from "../../../types";
import { THEME_COLORS } from "../../../theme";
import { DimmingCurveInput, NumberField } from "./PatchInputs";

function Choice<T extends string>({ label, value, options, onChange }: { label: string; value: T; options: readonly T[]; onChange: (value: T) => void }) {
  return <label>{label}<select value={value} onChange={(event) => { const choice = options.find((option) => option === event.target.value); if (choice !== undefined) onChange(choice); }}>{options.map((option) => <option key={option} value={option}>{option}</option>)}</select></label>;
}

function FunctionChoice({ value, functions, onChange }: { value: number; functions: GuiFixtureFunction[]; onChange: (value: number) => void }) {
  return <label>Function<select value={value} onChange={(event) => { onChange(Number(event.target.value)); }}>
    {!functions.some((item) => item.id === value) && <option value={value}>Missing function {value}</option>}
    {functions.map((item) => <option key={item.id} value={item.id}>{item.name} ({item.id})</option>)}
  </select></label>;
}

function EntryChoice({ label, value, fn, onChange }: { label: string; value: number; fn: GuiFixtureFunction | undefined; onChange: (value: number) => void }) {
  const entries = fn?.kind.type === "indexed" || fn?.kind.type === "colorWheel" ? fn.kind.entries : [];
  return <label>{label}<select value={value} onChange={(event) => { onChange(Number(event.target.value)); }}>
    {!entries.some((entry) => entry.id === value) && <option value={value}>Missing entry {value}</option>}
    {entries.map((entry) => <option key={entry.id} value={entry.id}>{entry.name} ({entry.id})</option>)}
  </select></label>;
}

function newEntry(id: number): GuiFixtureEntry { return { id, name: "New entry", dmxMin: 0, dmxMax: 255, curveControl: false, color: null, tag: null }; }

export function FixtureDefinitionInput({ value, onChange }: { value: GuiFixtureDefinition; onChange: (value: GuiFixtureDefinition) => void }) {
  return <div className="setup-patch-list">
    <h4>Functions</h4>
    {value.functions.map((fn, index) => <section className="setup-patch-node" key={index}>
      <FunctionInput value={fn} onChange={(fn) => { onChange({ ...value, functions: value.functions.map((item, i) => i === index ? fn : item) }); }} />
      <button type="button" onClick={() => { onChange({ ...value, functions: value.functions.filter((_, i) => i !== index) }); }}>Remove function</button>
    </section>)}
    <button type="button" onClick={() => {
      const id = Math.max(0, ...value.functions.map((fn) => fn.id)) + 1;
      const slot = Math.max(-1, ...value.channels.map((channel) => channel.slot)) + 1;
      onChange({ ...value, functions: [...value.functions, { id, name: "New function", tag: null, kind: { type: "range" }, curve: { type: "linear" } }], channels: [...value.channels, { slot, role: { type: "coarse", function: id }, curve: { type: "linear" } }] });
    }}>Add function with channel</button>
    <h4>Channels</h4>
    <p>Channel numbers are relative to the fixture's first assigned controller channel. Use coarse and fine for 16-bit values, or one channel per color component.</p>
    {value.channels.map((channel, index) => <section className="setup-patch-node" key={index}>
      <div className="setup-patch-fields">
        <NumberField label="Channel" min={1} max={65536} value={channel.slot + 1} onChange={(slot) => { onChange({ ...value, channels: value.channels.map((item, i) => i === index ? { ...item, slot: slot - 1 } : item) }); }} />
        <ChannelRoleInput value={channel.role} functions={value.functions} onChange={(role) => { onChange({ ...value, channels: value.channels.map((item, i) => i === index ? { ...item, role } : item) }); }} />
        <DimmingCurveInput value={channel.curve} onChange={(curve) => { onChange({ ...value, channels: value.channels.map((item, i) => i === index ? { ...item, curve } : item) }); }} />
        <button type="button" onClick={() => { onChange({ ...value, channels: value.channels.filter((_, i) => i !== index) }); }}>Remove channel</button>
      </div>
    </section>)}
    <button type="button" onClick={() => { onChange({ ...value, channels: [...value.channels, { slot: Math.max(-1, ...value.channels.map((channel) => channel.slot)) + 1, role: { type: "ignored" }, curve: { type: "linear" } }] }); }}>Add channel</button>
    <h4>Behavior rules</h4>
    {value.behaviorRules.map((rule, index) => <section className="setup-patch-node" key={index}>
      <BehaviorInput value={rule} functions={value.functions} onChange={(rule) => { onChange({ ...value, behaviorRules: value.behaviorRules.map((item, i) => i === index ? rule : item) }); }} />
      <button type="button" onClick={() => { onChange({ ...value, behaviorRules: value.behaviorRules.filter((_, i) => i !== index) }); }}>Remove behavior</button>
    </section>)}
    <button type="button" disabled={value.functions.length === 0} onClick={() => { const fn = value.functions[0]; if (fn !== undefined) onChange({ ...value, behaviorRules: [...value.behaviorRules, { type: "dimmer", function: fn.id, off: 0, on: 1 }] }); }}>Add behavior rule</button>
  </div>;
}

function FunctionInput({ value, onChange }: { value: GuiFixtureFunction; onChange: (value: GuiFixtureFunction) => void }) {
  const changeKind = (type: GuiFixtureFunctionKind["type"]) => {
    switch (type) {
      case "range": onChange({ ...value, kind: { type } }); break;
      case "colorMixing": onChange({ ...value, kind: { type, model: "rgb" } }); break;
      case "indexed": case "colorWheel": onChange({ ...value, kind: { type, entries: value.kind.type === "indexed" || value.kind.type === "colorWheel" ? value.kind.entries : [newEntry(1)] } }); break;
    }
  };
  return <div className="setup-patch-list">
    <div className="setup-patch-fields">
      <NumberField label="Function identifier" value={value.id} max={4294967295} onChange={(id) => { onChange({ ...value, id }); }} />
      <label>Name<input required value={value.name} onChange={(event) => { onChange({ ...value, name: event.target.value }); }} /></label>
      <Choice label="Type" value={value.kind.type} options={["range", "indexed", "colorWheel", "colorMixing"]} onChange={changeKind} />
      <Choice label="Tag" value={value.tag ?? "none"} options={["none", "pan", "tilt", "dimmer", "shutter", "zoom", "gobo", "frost", "prism", "colorWheel", "colorMixing"]} onChange={(tag) => { onChange({ ...value, tag: tag === "none" ? null : tag }); }} />
      <DimmingCurveInput value={value.curve} onChange={(curve) => { onChange({ ...value, curve }); }} />
    </div>
    {value.kind.type === "colorMixing" && <Choice label="Color model" value={value.kind.model} options={["rgb", "rgbw"]} onChange={(model) => { onChange({ ...value, kind: { type: "colorMixing", model } }); }} />}
    {(value.kind.type === "indexed" || value.kind.type === "colorWheel") && <EntriesInput value={value.kind.entries} onChange={(entries) => { if (value.kind.type === "indexed" || value.kind.type === "colorWheel") onChange({ ...value, kind: { ...value.kind, entries } }); }} />}
  </div>;
}

function EntriesInput({ value, onChange }: { value: GuiFixtureEntry[]; onChange: (value: GuiFixtureEntry[]) => void }) {
  return <div className="setup-patch-list">
    {value.map((entry, index) => {
      const update = (next: Partial<GuiFixtureEntry>) => { onChange(value.map((item, i) => i === index ? { ...item, ...next } : item)); };
      return <div className="setup-patch-fields" key={index}>
        <NumberField label="Entry identifier" value={entry.id} max={4294967295} onChange={(id) => { update({ id }); }} />
        <label>Name<input required value={entry.name} onChange={(event) => { update({ name: event.target.value }); }} /></label>
        <NumberField label="DMX minimum" value={entry.dmxMin} max={65535} onChange={(dmxMin) => { update({ dmxMin }); }} />
        <NumberField label="DMX maximum" value={entry.dmxMax} max={65535} onChange={(dmxMax) => { update({ dmxMax }); }} />
        <label><input type="checkbox" checked={entry.curveControl} onChange={(event) => { update({ curveControl: event.target.checked }); }} />Animate within range</label>
        <label><input type="checkbox" checked={entry.color !== null} onChange={(event) => { update({ color: event.target.checked ? THEME_COLORS.white : null }); }} />Entry has a color</label>
        {entry.color !== null && <label>Color<input type="color" value={entry.color} onChange={(event) => { update({ color: event.target.value }); }} /></label>}
        <Choice label="Tag" value={entry.tag ?? "none"} options={["none", "shutterOpen", "shutterClosed", "strobe", "prismOpen", "prismClosed", "goboOpen"]} onChange={(tag) => { update({ tag: tag === "none" ? null : tag }); }} />
        <button type="button" onClick={() => { onChange(value.filter((_, i) => i !== index)); }}>Remove entry</button>
      </div>;
    })}
    <button type="button" onClick={() => { onChange([...value, newEntry(Math.max(0, ...value.map((entry) => entry.id)) + 1)]); }}>Add entry</button>
  </div>;
}

function ChannelRoleInput({ value, functions, onChange }: { value: GuiFixtureChannelRole; functions: GuiFixtureFunction[]; onChange: (value: GuiFixtureChannelRole) => void }) {
  return <>
    <Choice label="Role" value={value.type} options={["ignored", "coarse", "fine", "colorComponent"]} onChange={(type) => {
      const fn = value.type === "ignored" ? functions[0]?.id ?? 0 : value.function;
      switch (type) {
        case "ignored": onChange({ type }); break;
        case "coarse": case "fine": onChange({ type, function: fn }); break;
        case "colorComponent": onChange({ type, function: fn, component: "red" }); break;
      }
    }} />
    {value.type !== "ignored" && <FunctionChoice value={value.function} functions={functions} onChange={(fn) => { onChange({ ...value, function: fn }); }} />}
    {value.type === "colorComponent" && <Choice label="Component" value={value.component} options={["red", "green", "blue", "white"]} onChange={(component) => { onChange({ ...value, component }); }} />}
  </>;
}

function BehaviorInput({ value, functions, onChange }: { value: GuiFixtureBehavior; functions: GuiFixtureFunction[]; onChange: (value: GuiFixtureBehavior) => void }) {
  const fn = functions.find((fn) => fn.id === value.function);
  return <div className="setup-patch-fields">
    <Choice label="Behavior" value={value.type} options={["dimmer", "shutter", "colorWheel", "prismGate"]} onChange={(type) => {
      switch (type) {
        case "dimmer": onChange({ type, function: value.function, off: 0, on: 1 }); break;
        case "shutter": onChange({ type, function: value.function, closed: 1, open: 2 }); break;
        case "prismGate": onChange({ type, function: value.function, disabled: 1, enabled: 2 }); break;
        case "colorWheel": onChange({ type, function: value.function, entries: [] }); break;
      }
    }} />
    <FunctionChoice value={value.function} functions={functions} onChange={(fn) => { onChange({ ...value, function: fn }); }} />
    {value.type === "dimmer" && <><NumberField label="Off level" value={value.off} max={1} step="any" onChange={(off) => { onChange({ ...value, off }); }} /><NumberField label="On level" value={value.on} max={1} step="any" onChange={(on) => { onChange({ ...value, on }); }} /></>}
    {value.type === "shutter" && <><EntryChoice label="Closed entry" value={value.closed} fn={fn} onChange={(closed) => { onChange({ ...value, closed }); }} /><EntryChoice label="Open entry" value={value.open} fn={fn} onChange={(open) => { onChange({ ...value, open }); }} /></>}
    {value.type === "prismGate" && <><EntryChoice label="Disabled entry" value={value.disabled} fn={fn} onChange={(disabled) => { onChange({ ...value, disabled }); }} /><EntryChoice label="Enabled entry" value={value.enabled} fn={fn} onChange={(enabled) => { onChange({ ...value, enabled }); }} /></>}
    {value.type === "colorWheel" && <div className="setup-patch-list">
      {value.entries.map((entry, index) => <div className="setup-patch-fields" key={index}>
        <label>Color<input type="color" value={entry.color} onChange={(event) => { onChange({ ...value, entries: value.entries.map((item, i) => i === index ? { ...item, color: event.target.value } : item) }); }} /></label>
        <EntryChoice label="Entry" value={entry.entry} fn={fn} onChange={(id) => { onChange({ ...value, entries: value.entries.map((item, i) => i === index ? { ...item, entry: id } : item) }); }} />
        <button type="button" onClick={() => { onChange({ ...value, entries: value.entries.filter((_, i) => i !== index) }); }}>Remove color mapping</button>
      </div>)}
      <button type="button" onClick={() => { const entry = fn?.kind.type === "colorWheel" || fn?.kind.type === "indexed" ? fn.kind.entries[0] : undefined; onChange({ ...value, entries: [...value.entries, { color: THEME_COLORS.white, entry: entry?.id ?? 0 }] }); }}>Add color mapping</button>
    </div>}
  </div>;
}

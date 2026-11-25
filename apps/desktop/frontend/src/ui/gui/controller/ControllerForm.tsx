import { ArrowDown, ArrowUp, X } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { useAppStore } from "../../../store";
import { applyEdit } from "../edits";
import type { GuiEdit, GuiField, GuiMember, ProjectGuiDocument, ProjectGuiEdit } from "../../../types";
import { AddRow, Button, Field, IconButton, NumberField, Section, Select, TextField, Toggle } from "../../kit";
import { DraftProblem, OBJECT, useDraftCheck, useFieldProblem } from "../problems";
import { useDeviceWatch } from "./DonderDevicePanel";

type ControllerConfig = Extract<ProjectGuiEdit, { type: "addController" }>["config"];
type ControllerPorts = ProjectGuiDocument["controllers"][number]["ports"];
type Universes = Extract<ControllerConfig, { type: "e131" }>["universes"];

const PROTOCOLS = [
  { value: "e131", label: "E1.31 / sACN" },
  { value: "artNet", label: "Art-Net" },
  { value: "donder", label: "Donder" }
] as const;

const MAX_UNIVERSE_CHANNELS = 512;
/** 170 RGB pixels, the usual pixel controller universe. */
const DEFAULT_UNIVERSE_CHANNELS = 510;
const E131_UNIVERSES = { min: 1, max: 63999 };
const ARTNET_PORT_ADDRESSES = { min: 0, max: 32767 };
const DONDER_OUTPUT_CHANNELS = 600;

function defaultConfig(type: ControllerConfig["type"], device: string): ControllerConfig {
  switch (type) {
    case "e131": return { type, senderName: "Donder", bindAddress: "0.0.0.0", priority: 100, destination: null, universes: { start: E131_UNIVERSES.min, count: 1, channels: DEFAULT_UNIVERSE_CHANNELS }, fseqChannel: null };
    case "artNet": return { type, bindAddress: "0.0.0.0:6454", destination: "255.255.255.255:6454", broadcast: true, universes: { start: ARTNET_PORT_ADDRESSES.min, count: 1, channels: DEFAULT_UNIVERSE_CHANNELS }, fseqChannel: null };
    case "donder": return { type, device };
  }
}

/** The first number no other port of the controller is named. */
function freshPortName(ports: ControllerPorts): string {
  const used = new Set(ports.map((port) => port.name));
  for (let index = ports.length + 1; ; index += 1) {
    const name = String(index);
    if (!used.has(name)) return name;
  }
}

/** A Donder controller's ports each drive a physical output; other controllers' ports drive none. */
const donderOutput = (type: ControllerConfig["type"], output: number) => type === "donder" ? { output, slotCount: DONDER_OUTPUT_CHANNELS } : null;

function newPort(type: ControllerConfig["type"], ports: ControllerPorts): ControllerPorts[number] {
  return { id: Math.max(0, ...ports.map((port) => port.id)) + 1, name: freshPortName(ports), description: null, donder: donderOutput(type, ports.length + 1) };
}

/** `ports` for a controller of `type`: Donder ports gain outputs, others drop them; a controller starts with one port. */
function portsFor(type: ControllerConfig["type"], ports: ControllerPorts): ControllerPorts {
  if (ports.length === 0) return [newPort(type, ports)];
  return ports.map((port, index) => ({ ...port, donder: type === "donder" ? port.donder ?? donderOutput(type, index + 1) : null }));
}

/** A labelled field of the controller or one of its ports, marked when its value breaks a rule. */
function ConfigField({ label, tip, member = OBJECT, field, children }: { label: string; tip?: string; member?: GuiMember | null; field: GuiField; children: (invalid: boolean) => ReactNode }) {
  const problem = useFieldProblem(member, field);
  return <Field label={label} tip={tip} error={problem}>{children(problem !== null)}</Field>;
}

function UniverseFields({ universes, addresses, onChange }: { universes: Universes; addresses: { min: number; max: number }; onChange: (universes: Universes) => void }) {
  return <>
    <ConfigField label="Start universe" field={{ type: "universeStart" }}>{(invalid) => <NumberField name="Start universe" value={universes.start} step={1} min={addresses.min} max={addresses.max} defaultValue={addresses.min} integer invalid={invalid} onCommit={(start) => { onChange({ ...universes, start }); }} />}</ConfigField>
    <ConfigField label="Universes" field={{ type: "universeCount" }}>{(invalid) => <NumberField name="Universes" value={universes.count} step={1} min={1} max={addresses.max - universes.start + 1} defaultValue={1} integer invalid={invalid} onCommit={(count) => { onChange({ ...universes, count }); }} />}</ConfigField>
    <ConfigField label="Channels" tip="Channels per universe; 510 carries 170 RGB pixels" field={{ type: "universeChannels" }}>{(invalid) =>
      <NumberField name="Channels per universe" value={universes.channels} step={1} min={1} max={MAX_UNIVERSE_CHANNELS} defaultValue={DEFAULT_UNIVERSE_CHANNELS} integer invalid={invalid} onCommit={(channels) => { onChange({ ...universes, channels }); }} />}
    </ConfigField>
  </>;
}

/** Where FSEQ files place the controller's first channel, as the player's channel outputs number it; off leaves it out of FSEQ files. */
function FseqFields({ channel, onChange }: { channel: number | null; onChange: (channel: number | null) => void }) {
  return <>
    <Field label="In FSEQ" tip="Include this controller in FSEQ files"><Toggle name="In FSEQ" checked={channel !== null} onChange={(included) => { onChange(included ? 1 : null); }} /></Field>
    {channel !== null && <ConfigField label="FSEQ channel" tip="Its first channel in FSEQ files, as FPP's channel outputs number it" field={{ type: "fseqChannel" }}>{(invalid) =>
      <NumberField name="FSEQ channel" value={channel} step={1} min={1} defaultValue={1} integer invalid={invalid} onCommit={onChange} />}
    </ConfigField>}
  </>;
}

/** A controller's protocol, addresses and outputs. The open controller commits each field as it changes; a new
 * controller is a draft checked after each change and added together. `edit` makes the edit that applies them. */
export function ControllerForm({ controller, edit, onApplied }: {
  controller?: ProjectGuiDocument["controllers"][number];
  edit: (config: ControllerConfig, ports: ControllerPorts) => GuiEdit;
  onApplied?: () => void;
}) {
  const devices = useAppStore((state) => state.snapshot?.devices ?? []);
  const request = useAppStore((state) => state.guiRequest);
  const [draft, setDraft] = useState<{ config: ControllerConfig; ports: ControllerPorts }>(() => ({ config: defaultConfig("e131", ""), ports: [newPort("e131", [])] }));
  const { config, ports } = controller ?? draft;
  useDeviceWatch(config.type === "donder");
  const added = useMemo(() => controller === undefined ? edit(draft.config, draft.ports) : null, [controller, edit, draft]);
  const { checking, problem } = useDraftCheck(request, added);
  const change = (next: { config?: ControllerConfig; ports?: ControllerPorts }) => {
    const value = { config: next.config ?? config, ports: next.ports ?? ports };
    if (controller === undefined) setDraft(value);
    else void applyEdit(edit(value.config, value.ports));
  };
  const setConfig = (next: ControllerConfig) => { change({ config: next }); };
  const setPorts = (next: ControllerPorts) => { change({ ports: next }); };
  const changePort = (index: number, change: Partial<ControllerPorts[number]>) => {
    setPorts(ports.map((candidate, i) => i === index ? { ...candidate, ...change } : candidate));
  };
  /** Swap the port at `index` with its neighbour `offset` away; each keeps its own Donder output. */
  const movePort = (index: number, offset: number) => {
    const other = index + offset;
    const moved = [...ports];
    const [a, b] = [moved[index], moved[other]];
    if (a === undefined || b === undefined) return;
    moved[index] = b;
    moved[other] = a;
    setPorts(moved);
  };
  const deviceOptions = config.type !== "donder" ? [] : [
    ...(config.device === "" ? [{ value: "", label: devices.length === 0 ? "None found" : "Choose", disabled: true }] : []),
    ...(config.device !== "" && !devices.some((device) => device.id === config.device) ? [{ value: config.device, label: `${config.device} (offline)` }] : []),
    ...devices.map((device) => ({ value: device.id, label: `${device.name} (${device.id})` }))
  ];
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    if (added !== null) void applyEdit(added).then((result) => { if (result !== null) onApplied?.(); });
  }}><DraftProblem problem={problem}>
    <fieldset>
      <Field label="Protocol">
        <Select name="Protocol" value={config.type} options={PROTOCOLS} onChange={(type) => {
          change({ config: defaultConfig(type, devices[0]?.id ?? ""), ports: portsFor(type, ports) });
        }} />
      </Field>
      {config.type === "e131" && <>
        <ConfigField label="Destination" tip="Empty for multicast" field={{ type: "destination" }}>{(invalid) => <TextField name="Destination" placeholder="Multicast" value={config.destination ?? ""} invalid={invalid} onCommit={(destination) => { setConfig({ ...config, destination: destination.trim() === "" ? null : destination }); }} />}</ConfigField>
        <UniverseFields universes={config.universes} addresses={E131_UNIVERSES} onChange={(universes) => { setConfig({ ...config, universes }); }} />
        <FseqFields channel={config.fseqChannel} onChange={(fseqChannel) => { setConfig({ ...config, fseqChannel }); }} />
        <Section title="Advanced" defaultOpen={false}>
          <ConfigField label="Interface" tip="The local address to send from; 0.0.0.0 picks one" field={{ type: "interface" }}>{(invalid) => <TextField name="Local interface" value={config.bindAddress} invalid={invalid} onCommit={(bindAddress) => { setConfig({ ...config, bindAddress }); }} />}</ConfigField>
          <ConfigField label="Sender" tip="The source name receivers show" field={{ type: "senderName" }}>{(invalid) => <TextField name="Sender name" value={config.senderName} invalid={invalid} onCommit={(senderName) => { setConfig({ ...config, senderName }); }} />}</ConfigField>
          <ConfigField label="Priority" tip="Receivers follow the highest-priority source" field={{ type: "priority" }}>{(invalid) => <NumberField name="Priority" value={config.priority} step={1} min={1} max={200} defaultValue={100} integer invalid={invalid} onCommit={(priority) => { setConfig({ ...config, priority }); }} />}</ConfigField>
        </Section>
      </>}
      {config.type === "artNet" && <>
        <ConfigField label="Destination" field={{ type: "destination" }}>{(invalid) => <TextField name="Destination socket" value={config.destination} invalid={invalid} onCommit={(destination) => { setConfig({ ...config, destination }); }} />}</ConfigField>
        <Field label="Broadcast"><Toggle name="Broadcast" checked={config.broadcast} onChange={(broadcast) => { setConfig({ ...config, broadcast }); }} /></Field>
        <UniverseFields universes={config.universes} addresses={ARTNET_PORT_ADDRESSES} onChange={(universes) => { setConfig({ ...config, universes }); }} />
        <FseqFields channel={config.fseqChannel} onChange={(fseqChannel) => { setConfig({ ...config, fseqChannel }); }} />
        <Section title="Advanced" defaultOpen={false}>
          <ConfigField label="Interface" tip="The local address and port to send from" field={{ type: "interface" }}>{(invalid) => <TextField name="Local interface" value={config.bindAddress} invalid={invalid} onCommit={(bindAddress) => { setConfig({ ...config, bindAddress }); }} />}</ConfigField>
        </Section>
      </>}
      {config.type === "donder" && <>
        <ConfigField label="Device" field={{ type: "device" }}>{(invalid) =>
          <Select name="Device" value={config.device} options={deviceOptions} invalid={invalid} onChange={(device) => { setConfig({ ...config, device }); }} />}
        </ConfigField>
      </>}
      <Section title="Ports">
        {ports.map((port, index) => <div className="controller-port-row" key={port.id}>
          <ConfigField label="Name" tip="As the controller names this output" member={{ type: "port", id: port.id }} field={{ type: "name" }}>{(invalid) => <TextField name="Port name" value={port.name} invalid={invalid} onCommit={(name) => { changePort(index, { name }); }} />}</ConfigField>
          <Field label="Description"><TextField name="Port description" value={port.description ?? ""} onCommit={(description) => { changePort(index, { description: description.trim() === "" ? null : description }); }} /></Field>
          {port.donder !== null && <>
            <span>Output {port.donder.output}</span>
            <ConfigField label="Channels" member={{ type: "port", id: port.id }} field={{ type: "channels" }}>{(invalid) =>
              <NumberField name="Channels" value={port.donder?.slotCount ?? DONDER_OUTPUT_CHANNELS} step={1} min={1} integer invalid={invalid} onCommit={(slotCount) => { if (port.donder !== null) changePort(index, { donder: { ...port.donder, slotCount } }); }} />}
            </ConfigField>
          </>}
          <IconButton icon={ArrowUp} label="Move up" disabled={index === 0} onClick={() => { movePort(index, -1); }} />
          <IconButton icon={ArrowDown} label="Move down" disabled={index === ports.length - 1} onClick={() => { movePort(index, 1); }} />
          <IconButton icon={X} label="Remove port" danger onClick={() => {
            // Donder outputs are numbered 1..n to match the controller's physical outputs.
            setPorts(ports.filter((_, i) => i !== index).map((candidate, i) => ({ ...candidate, donder: candidate.donder === null ? null : { ...candidate.donder, output: i + 1 } })));
          }} />
        </div>)}
        <AddRow label="Add port" onAdd={() => { setPorts([...ports, newPort(config.type, ports)]); }} />
      </Section>
      {added !== null && <Button type="submit" variant="primary" disabled={checking || problem !== null} {...(problem === null ? {} : { tip: problem.message })}>Add controller</Button>}
    </fieldset>
  </DraftProblem></form>;
}

import { X } from "lucide-react";
import { useState } from "react";
import { useAppStore } from "../../../store";
import type { SetupGuiDocument, SetupGuiEdit } from "../../../types";
import { AddRow, Button, Field, IconButton, NumberField, Select, TextField, Toggle } from "../../kit";
import { useDeviceWatch } from "./DonderDevicePanel";

type ControllerConfig = Extract<SetupGuiEdit, { type: "addController" }>["config"];
type ControllerPorts = SetupGuiDocument["controllers"][number]["ports"];

const PROTOCOLS = [
  { value: "e131", label: "E1.31 / sACN" },
  { value: "artNet", label: "Art-Net" },
  { value: "donder", label: "Donder" }
] as const;

const MAX_UNIVERSE_SLOTS = 512;

function defaultConfig(type: ControllerConfig["type"], device: string): ControllerConfig {
  switch (type) {
    case "e131": return { type, senderName: "Donder", bindAddress: "0.0.0.0", priority: 100, destination: null };
    case "artNet": return { type, bindAddress: "0.0.0.0:6454", destination: "255.255.255.255:6454", broadcast: true };
    case "donder": return { type, device };
  }
}

/** The first `port_<n>` no other port of the controller uses. */
function freshPortName(ports: ControllerPorts): string {
  const used = new Set(ports.map((port) => port.name));
  for (let index = ports.length + 1; ; index += 1) {
    const name = `port_${index}`;
    if (!used.has(name)) return name;
  }
}

export function ControllerForm({ controller, onSave }: { controller?: SetupGuiDocument["controllers"][number]; onSave: (config: ControllerConfig, ports: ControllerPorts) => Promise<void> }) {
  const devices = useAppStore((state) => state.snapshot?.devices ?? []);
  const [config, setConfig] = useState<ControllerConfig>(controller?.config ?? defaultConfig("e131", ""));
  const [ports, setPorts] = useState<ControllerPorts>(controller?.ports ?? [{ id: 1, name: "port_1", address: 1, slotCount: MAX_UNIVERSE_SLOTS }]);
  useDeviceWatch(config.type === "donder");
  const firstAddress = config.type === "artNet" ? 0 : 1;
  const changePort = (index: number, change: Partial<ControllerPorts[number]>) => {
    setPorts(ports.map((candidate, i) => i === index ? { ...candidate, ...change } : candidate));
  };
  const deviceOptions = config.type !== "donder" ? [] : [
    ...(config.device === "" ? [{ value: "", label: devices.length === 0 ? "None found" : "Choose", disabled: true }] : []),
    ...(config.device !== "" && !devices.some((device) => device.id === config.device) ? [{ value: config.device, label: `${config.device} (offline)` }] : []),
    ...devices.map((device) => ({ value: device.id, label: `${device.name} (${device.id})` }))
  ];
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    void onSave(config, ports);
  }}>
    <fieldset>
      <Field label="Protocol">
        <Select name="Protocol" value={config.type} options={PROTOCOLS} onChange={(type) => {
          setConfig(defaultConfig(type, devices[0]?.id ?? ""));
          // Donder outputs are numbered 1..n to match the controller's physical outputs.
          if (type === "donder") setPorts(ports.map((port, index) => ({ ...port, address: index + 1 })));
        }} />
      </Field>
      {config.type === "e131" && <>
        <Field label="Interface"><TextField name="Local interface" value={config.bindAddress} invalid={config.bindAddress.trim() === ""} onCommit={(bindAddress) => { setConfig({ ...config, bindAddress }); }} /></Field>
        <Field label="Sender"><TextField name="Sender name" value={config.senderName} invalid={config.senderName.trim() === ""} onCommit={(senderName) => { setConfig({ ...config, senderName }); }} /></Field>
        <Field label="Priority"><NumberField name="Priority" value={config.priority} step={1} min={1} max={200} defaultValue={100} integer onCommit={(priority) => { setConfig({ ...config, priority }); }} /></Field>
        <Field label="Destination" tip="Empty for multicast"><TextField name="Destination" placeholder="Multicast" value={config.destination ?? ""} onCommit={(destination) => { setConfig({ ...config, destination: destination.trim() === "" ? null : destination }); }} /></Field>
      </>}
      {config.type === "artNet" && <>
        <Field label="Interface"><TextField name="Local interface" value={config.bindAddress} invalid={config.bindAddress.trim() === ""} onCommit={(bindAddress) => { setConfig({ ...config, bindAddress }); }} /></Field>
        <Field label="Destination"><TextField name="Destination socket" value={config.destination} invalid={config.destination.trim() === ""} onCommit={(destination) => { setConfig({ ...config, destination }); }} /></Field>
        <Field label="Broadcast"><Toggle name="Broadcast" checked={config.broadcast} onChange={(broadcast) => { setConfig({ ...config, broadcast }); }} /></Field>
      </>}
      {config.type === "donder" && <Field label="Device">
        <Select name="Device" value={config.device} options={deviceOptions} onChange={(device) => { setConfig({ ...config, device }); }} />
      </Field>}
      {ports.map((port, index) => <div className="controller-port-row" key={port.id}>
        <Field label="Name"><TextField name="Output name" value={port.name} onCommit={(name) => { changePort(index, { name }); }} /></Field>
        {config.type === "donder" ? <span>Output {port.address}</span>
          : <Field label={config.type === "e131" ? "Universe" : "Port address"}>
            <NumberField name={config.type === "e131" ? "Universe" : "Port address"} value={port.address} step={1} min={firstAddress} integer onCommit={(address) => { changePort(index, { address }); }} />
          </Field>}
        <Field label="Channels">
          <NumberField name="Channels" value={port.slotCount} step={1} min={1} {...(config.type === "donder" ? {} : { max: MAX_UNIVERSE_SLOTS })} integer onCommit={(slotCount) => { changePort(index, { slotCount }); }} />
        </Field>
        <IconButton icon={X} label="Remove output" danger onClick={() => {
          const remaining = ports.filter((_, i) => i !== index);
          setPorts(config.type === "donder" ? remaining.map((candidate, i) => ({ ...candidate, address: i + 1 })) : remaining);
        }} />
      </div>)}
      <AddRow label="Add output" onAdd={() => {
        const address = config.type === "donder" ? ports.length + 1 : Math.max(firstAddress - 1, ...ports.map((port) => port.address)) + 1;
        const id = Math.max(0, ...ports.map((port) => port.id)) + 1;
        setPorts([...ports, { id, name: freshPortName(ports), address, slotCount: config.type === "donder" ? 600 : MAX_UNIVERSE_SLOTS }]);
      }} />
      <Button type="submit" variant="primary">{controller === undefined ? "Add controller" : "Apply"}</Button>
    </fieldset>
  </form>;
}

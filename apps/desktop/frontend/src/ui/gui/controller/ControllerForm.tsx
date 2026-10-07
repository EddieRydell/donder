import { useState } from "react";
import { useAppStore } from "../../../store";
import type { SetupDocument, SetupGuiEdit } from "../../../types";
import { DeviceUsbTools } from "./DonderDevicePanel";

type ControllerConfig = Extract<SetupGuiEdit, { type: "addController" }>["config"];
type ControllerPorts = SetupDocument["controllers"][number]["ports"];

function defaultConfig(type: ControllerConfig["type"], device: string): ControllerConfig {
  switch (type) {
    case "e131": return { type, sourceName: "Donder", bindAddress: "0.0.0.0", priority: 100, destination: null };
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

export function ControllerForm({ controller, onSave }: { controller?: SetupDocument["controllers"][number]; onSave: (config: ControllerConfig, ports: ControllerPorts) => Promise<void> }) {
  const devices = useAppStore((state) => state.snapshot?.devices ?? []);
  const [config, setConfig] = useState<ControllerConfig>(controller?.config ?? defaultConfig("e131", ""));
  const [ports, setPorts] = useState<ControllerPorts>(controller?.ports ?? [{ id: 1, name: "port_1", address: 1, slotCount: 512 }]);
  const firstAddress = config.type === "artNet" ? 0 : 1;
  return <>
    <form className="setup-authoring-form" onSubmit={(event) => {
      event.preventDefault();
      void onSave(config, ports);
    }}>
      <h4>{controller === undefined ? "Add a controller" : "Controller settings"}</h4>
      <fieldset disabled={controller?.readOnly === true}>
        <label>Protocol<select value={config.type} onChange={(event) => {
          const type = event.target.value as ControllerConfig["type"];
          setConfig(defaultConfig(type, devices[0]?.id ?? ""));
          // Donder outputs are numbered 1..n to match the controller's physical outputs.
          if (type === "donder") setPorts(ports.map((port, index) => ({ ...port, address: index + 1 })));
        }}><option value="e131">E1.31 / sACN</option><option value="artNet">Art-Net</option><option value="donder">Donder controller</option></select></label>
        {config.type === "e131" && <>
          <label>Local interface<input required value={config.bindAddress} onChange={(event) => { setConfig({ ...config, bindAddress: event.target.value }); }} /></label>
          <label>Source name<input required value={config.sourceName} onChange={(event) => { setConfig({ ...config, sourceName: event.target.value }); }} /></label>
          <label>Priority<input type="number" min={1} max={200} value={config.priority} onChange={(event) => { setConfig({ ...config, priority: Number(event.target.value) }); }} /></label>
          <label>Destination (empty for multicast)<input value={config.destination ?? ""} onChange={(event) => { setConfig({ ...config, destination: event.target.value || null }); }} /></label>
        </>}
        {config.type === "artNet" && <>
          <label>Local interface<input required value={config.bindAddress} onChange={(event) => { setConfig({ ...config, bindAddress: event.target.value }); }} /></label>
          <label>Destination socket<input required value={config.destination} onChange={(event) => { setConfig({ ...config, destination: event.target.value }); }} /></label>
          <label><input type="checkbox" checked={config.broadcast} onChange={(event) => { setConfig({ ...config, broadcast: event.target.checked }); }} />Broadcast</label>
        </>}
        {config.type === "donder" && <label>Device<select required value={config.device} onChange={(event) => { setConfig({ ...config, device: event.target.value }); }}>
          <option value="" disabled>{devices.length === 0 ? "No Donder controllers found" : "Choose a controller"}</option>
          {config.device !== "" && !devices.some((device) => device.id === config.device) && <option value={config.device}>{config.device} (not on the network)</option>}
          {devices.map((device) => <option key={device.id} value={device.id}>{device.name} ({device.id})</option>)}
        </select></label>}
        {ports.map((port, index) => <div className="controller-port-row" key={port.id}>
          <label>Name<input required pattern="[a-z_][a-z0-9_]*" value={port.name} onChange={(event) => { setPorts(ports.map((candidate, i) => i === index ? { ...candidate, name: event.target.value } : candidate)); }} /></label>
          {config.type === "donder" ? <span>Controller output {port.address}</span>
            : <label>{config.type === "e131" ? "Universe" : "Port address"}<input type="number" min={firstAddress} value={port.address} onChange={(event) => { setPorts(ports.map((candidate, i) => i === index ? { ...candidate, address: Number(event.target.value) } : candidate)); }} /></label>}
          <label>Channels<input type="number" min={1} max={config.type === "donder" ? undefined : 512} value={port.slotCount} onChange={(event) => { setPorts(ports.map((candidate, i) => i === index ? { ...candidate, slotCount: Number(event.target.value) } : candidate)); }} /></label>
          <button type="button" onClick={() => {
            const remaining = ports.filter((_, i) => i !== index);
            setPorts(config.type === "donder" ? remaining.map((candidate, i) => ({ ...candidate, address: i + 1 })) : remaining);
          }}>Remove output</button>
        </div>)}
        <button type="button" onClick={() => {
          const address = config.type === "donder" ? ports.length + 1 : Math.max(firstAddress - 1, ...ports.map((port) => port.address)) + 1;
          const id = Math.max(0, ...ports.map((port) => port.id)) + 1;
          setPorts([...ports, { id, name: freshPortName(ports), address, slotCount: config.type === "donder" ? 600 : 512 }]);
        }}>Add output</button>
        <button type="submit">{controller === undefined ? "Add controller" : "Apply controller settings"}</button>
      </fieldset>
    </form>
    {controller === undefined && config.type === "donder" && <DeviceUsbTools />}
  </>;
}

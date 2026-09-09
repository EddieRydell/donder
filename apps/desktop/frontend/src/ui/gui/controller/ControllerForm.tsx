import { useState } from "react";
import type { SetupDocument, SetupGuiEdit } from "../../../types";

type ControllerConfig = Extract<SetupGuiEdit, { type: "addController" }>["config"];
export function ControllerForm({ controller, onSave }: { controller?: SetupDocument["controllers"][number]; onSave: (config: ControllerConfig, ports: SetupDocument["controllers"][number]["ports"]) => Promise<void> }) {
  const [config, setConfig] = useState<ControllerConfig>(controller?.config ?? { type: "e131", sourceName: "Dawn", bindAddress: "0.0.0.0", priority: 100, destination: null });
  const [ports, setPorts] = useState(controller?.ports ?? [{ id: 1, address: 1, slotCount: 512 }]);
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    void onSave(config, ports);
  }}>
    <h4>{controller === undefined ? "Add a controller" : "Controller settings"}</h4>
    <fieldset disabled={controller?.readOnly === true}>
      <label>Protocol<select value={config.type} onChange={(event) => {
        setConfig(event.target.value === "e131" ? { type: "e131", sourceName: "Dawn", bindAddress: "0.0.0.0", priority: 100, destination: null }
          : { type: "artNet", bindAddress: "0.0.0.0:6454", destination: "255.255.255.255:6454", broadcast: true });
      }}><option value="e131">E1.31 / sACN</option><option value="artNet">Art-Net</option></select></label>
      <label>Local interface<input required value={config.bindAddress} onChange={(event) => { setConfig({ ...config, bindAddress: event.target.value }); }} /></label>
      {config.type === "e131" ? <>
        <label>Source name<input required value={config.sourceName} onChange={(event) => { setConfig({ ...config, sourceName: event.target.value }); }} /></label>
        <label>Priority<input type="number" min={1} max={200} value={config.priority} onChange={(event) => { setConfig({ ...config, priority: Number(event.target.value) }); }} /></label>
        <label>Destination (empty for multicast)<input value={config.destination ?? ""} onChange={(event) => { setConfig({ ...config, destination: event.target.value || null }); }} /></label>
      </> : <>
        <label>Destination socket<input required value={config.destination} onChange={(event) => { setConfig({ ...config, destination: event.target.value }); }} /></label>
        <label><input type="checkbox" checked={config.broadcast} onChange={(event) => { setConfig({ ...config, broadcast: event.target.checked }); }} />Broadcast</label>
      </>}
      {ports.map((port, index) => <div className="controller-port-row" key={port.id}>
        <span>Output {port.id}</span>
        <label>{config.type === "e131" ? "Universe" : "Port address"}<input type="number" min={config.type === "e131" ? 1 : 0} value={port.address} onChange={(event) => { setPorts(ports.map((candidate, i) => i === index ? { ...candidate, address: Number(event.target.value) } : candidate)); }} /></label>
        <label>Channels<input type="number" min={1} max={512} value={port.slotCount} onChange={(event) => { setPorts(ports.map((candidate, i) => i === index ? { ...candidate, slotCount: Number(event.target.value) } : candidate)); }} /></label>
        <button type="button" onClick={() => { setPorts(ports.filter((_, i) => i !== index)); }}>Remove output</button>
      </div>)}
      <button type="button" onClick={() => { setPorts([...ports, { id: Math.max(0, ...ports.map((port) => port.id)) + 1, address: Math.max(config.type === "e131" ? 0 : -1, ...ports.map((port) => port.address)) + 1, slotCount: 512 }]); }}>Add output</button>
      <button type="submit">{controller === undefined ? "Add controller" : "Apply controller settings"}</button>
    </fieldset>
  </form>;
}

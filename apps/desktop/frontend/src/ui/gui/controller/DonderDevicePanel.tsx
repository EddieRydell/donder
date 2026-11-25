import { CircleAlert, WifiOff } from "lucide-react";
import { useEffect, useState } from "react";
import { ACCESS_POINT_PASSWORD, commands, errorMessage } from "../../../api";
import { runSnapshotCommand, useAppStore } from "../../../store";
import type { DonderDeviceNetworkRequest, DonderDeviceStatus } from "../../../types";
import { Badge, Button, Field, Section, TextField } from "../../kit";

const MAX_DEVICE_NAME = 32;

/** Live status and settings for the Donder controller a controller of the project targets. */
export function DonderDevicePanel({ deviceId }: { deviceId: string }) {
  useDeviceWatch(true);
  const device = useAppStore((state) => state.snapshot?.devices.find((candidate) => candidate.id === deviceId));
  return <section className="object-section">
    <h3>Controller device</h3>
    {device === undefined
      ? <p>{deviceId} <Badge icon={WifiOff} tip={`Not on the network. Power it on, then join its Wi-Fi network (Donder-${deviceId.slice(8)}, password ${ACCESS_POINT_PASSWORD}) or the network it joined.`}>Offline</Badge></p>
      : <DeviceSettings key={device.id} device={device} />}
  </section>;
}

function DeviceSettings({ device }: { device: DonderDeviceStatus }) {
  const [ssid, setSsid] = useState("");
  const [password, setPassword] = useState("");
  const [pending, setPending] = useState(false);
  // The failure of the last rename or network change, shown on the control that ran it.
  const [error, setError] = useState<{ on: "name" | "network"; message: string } | null>(null);
  const run = (on: "name" | "network", command: () => Promise<unknown>) => {
    setPending(true); setError(null);
    void command().catch((commandError: unknown) => { setError({ on, message: errorMessage(commandError) }); }).finally(() => { setPending(false); });
  };
  const connection = device.connection;
  return <div className="device-form">
    <p role="status">{device.name} · {device.address} · {device.network === "accessPoint" ? "own Wi-Fi network" : "joined Wi-Fi network"}
      {connection.state === "connected" ? ` · live output connected${connection.clockUncertaintyMicros === null ? "" : `, clock within ${(connection.clockUncertaintyMicros / 1000).toFixed(2)} ms`}` : ""}
      {connection.state === "connecting" ? " · live output connecting" : ""}
      {!device.firmwareCurrent && <> <Badge icon={CircleAlert} tip="Its firmware doesn't match this version of Donder. Flash the current firmware onto it.">Firmware</Badge></>}
      {connection.state === "failed" && <> <Badge icon={CircleAlert} tip={connection.error}>Live output</Badge></>}
    </p>
    <Field label="Name" error={error?.on === "name" ? error.message : null}>
      <TextField name="Controller name" value={device.name} disabled={pending} invalid={error?.on === "name"} onCommit={(text) => {
        const name = text.trim();
        if (name !== "" && name.length <= MAX_DEVICE_NAME && name !== device.name) run("name", () => runSnapshotCommand(() => commands.renameDevice(device.id, name)));
      }} />
    </Field>
    <Section title="Wi-Fi">
      <Field label="Network"><TextField name="Wi-Fi network name" value={ssid} disabled={pending} onCommit={setSsid} /></Field>
      <Field label="Password"><TextField name="Wi-Fi password" secret value={password} disabled={pending} onCommit={setPassword} /></Field>
      <div className="dialog-actions">
        {device.network === "station" && <Button tip={error?.on === "network" ? error.message : "Host its own Wi-Fi network again"} failed={error?.on === "network"} disabled={pending} onClick={() => { run("network", () => runSnapshotCommand(() => commands.setDeviceNetwork(device.id, null))); }}>Own network</Button>}
        <Button tip={error?.on === "network" ? error.message : "Join a 2.4 GHz WPA2 network. It restarts to apply the change and returns to its own network when it cannot join."}
          failed={error?.on === "network"}
          disabled={pending}
          onClick={() => { run("network", () => runSnapshotCommand(() => commands.setDeviceNetwork(device.id, { ssid, password } satisfies DonderDeviceNetworkRequest)).then(() => { setPassword(""); })); }}>Join</Button>
      </div>
    </Section>
  </div>;
}

/** Keeps controller discovery running while a view that lists Donder controllers is open. */
export function useDeviceWatch(active: boolean) {
  useEffect(() => {
    if (!active) return;
    void commands.watchDevices(true);
    return () => { void commands.watchDevices(false); };
  }, [active]);
}

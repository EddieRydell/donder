import { useEffect, useState } from "react";
import { commands } from "../../../api";
import { runSnapshotCommand, useAppStore } from "../../../store";
import type { DonderDeviceNetworkRequest, DonderDeviceStatus } from "../../../types";

/** Live status and settings for the Donder controller a setup controller targets. */
export function DonderDevicePanel({ deviceId }: { deviceId: string }) {
  useDeviceWatch(true);
  const device = useAppStore((state) => state.snapshot?.devices.find((candidate) => candidate.id === deviceId));
  return <section className="setup-section">
    <h3>Controller device</h3>
    {device === undefined
      ? <p>Controller {deviceId} is not on the network. Power it on, then connect this computer to its Wi-Fi network (Donder-{deviceId.slice(8)}) or to the network it joined.</p>
      : <DeviceSettings key={device.id} device={device} />}
  </section>;
}

function DeviceSettings({ device }: { device: DonderDeviceStatus }) {
  const [name, setName] = useState(device.name);
  const [ssid, setSsid] = useState("");
  const [password, setPassword] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = (command: () => Promise<unknown>) => {
    setPending(true); setError(null);
    void command().catch((commandError: unknown) => { setError(String(commandError)); }).finally(() => { setPending(false); });
  };
  const connection = device.connection;
  return <div className="device-form">
    <p role="status">{device.name} · {device.address} · {device.network === "accessPoint" ? "own Wi-Fi network" : "joined Wi-Fi network"}
      {connection.state === "connected" ? ` · live output connected${connection.clockUncertaintyMicros === null ? "" : `, clock within ${(connection.clockUncertaintyMicros / 1000).toFixed(2)} ms`}` : ""}
      {connection.state === "connecting" ? " · live output connecting" : ""}</p>
    {connection.state === "failed" && <p role="alert">{connection.error}</p>}
    {!device.firmwareCurrent && <p role="alert">This controller runs firmware for a different sequence format. Flash the current Donder firmware onto it.</p>}
    {error !== null && <p role="alert">{error}</p>}
    {device.claim === "unclaimed" && <>
      <p>Claim this controller so this computer can upload shows and control playback.</p>
      <button type="button" disabled={pending} onClick={() => { run(() => runSnapshotCommand(() => commands.claimDevice(device.id))); }}>Claim controller</button>
    </>}
    {device.claim === "claimedElsewhere" && <p>Another computer claimed this controller. Reinstall its firmware with saved data erased to claim it here.</p>}
    {device.claim === "claimed" && <fieldset disabled={pending}>
      <form onSubmit={(event) => { event.preventDefault(); run(() => runSnapshotCommand(() => commands.renameDevice(device.id, name))); }}>
        <label>Name<input required maxLength={32} value={name} onChange={(event) => { setName(event.target.value); }} /></label>
        <button type="submit" disabled={name === device.name}>Rename</button>
      </form>
      <form onSubmit={(event) => {
        event.preventDefault();
        run(() => runSnapshotCommand(() => commands.setDeviceNetwork(device.id, { ssid, password } satisfies DonderDeviceNetworkRequest)).then(() => { setPassword(""); }));
      }}>
        <p>The controller hosts its own Wi-Fi network until it joins another 2.4 GHz WPA2 network. It restarts to apply the change and falls back to its own network when it cannot join.</p>
        <label>Wi-Fi network name<input required value={ssid} onChange={(event) => { setSsid(event.target.value); }} autoComplete="off" /></label>
        <label>Wi-Fi password<input required type="password" minLength={8} value={password} onChange={(event) => { setPassword(event.target.value); }} autoComplete="off" /></label>
        <button type="submit">Join network</button>
        {device.network === "station" && <button type="button" onClick={() => { run(() => runSnapshotCommand(() => commands.setDeviceNetwork(device.id, null))); }}>Use its own Wi-Fi network</button>}
      </form>
      <div>
        <p>The controller keeps the last show the editor played. It can loop that show on its own, with no computer attached. Restarts leave it stopped.</p>
        <button type="button" onClick={() => { run(() => runSnapshotCommand(() => commands.setDeviceStandalone(device.id, true))); }}>Loop saved show</button>
        <button type="button" onClick={() => { run(() => runSnapshotCommand(() => commands.setDeviceStandalone(device.id, false))); }}>Stop saved show</button>
      </div>
    </fieldset>}
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


import * as Dialog from "@radix-ui/react-dialog";
import { Channel } from "@tauri-apps/api/core";
import { Download } from "lucide-react";
import { useState } from "react";
import { commands } from "../../../api";
import { useAppStore } from "../../../store";
import { THEME_METRICS } from "../../../theme";
import type { DeviceFirmwareInfo, DeviceInstallProgress, DeviceCapabilities, DevicePlaybackMode, DeviceTransportStatus, DeviceSerialPort, GuiDocumentRequest, SequenceExportPort } from "../../../types";

export function SequenceExportDialog() {
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const editing = useAppStore((state) => state.guiEditPending);
  const [origin, setOrigin] = useState<GuiDocumentRequest | null>(null);
  const [ports, setPorts] = useState<SequenceExportPort[]>([]);
  const [selected, setSelected] = useState<number[]>([]);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [serialPorts, setSerialPorts] = useState<DeviceSerialPort[]>([]);
  const [firmware, setFirmware] = useState<DeviceFirmwareInfo | null>(null);
  const [installConfirmed, setInstallConfirmed] = useState(false);
  const [installProgress, setInstallProgress] = useState<DeviceInstallProgress | null>(null);
  const [eraseConfirmed, setEraseConfirmed] = useState(false);
  const [serialPort, setSerialPort] = useState("");
  const [ssid, setSsid] = useState("");
  const [password, setPassword] = useState("");
  const [connectionStatus, setConnectionStatus] = useState<string | null>(null);
  const [address, setAddress] = useState("");
  const [token, setToken] = useState("");
  const [capabilities, setCapabilities] = useState<DeviceCapabilities | null>(null);
  const [transport, setTransport] = useState<DeviceTransportStatus | null>(null);
  const [uploaded, setUploaded] = useState<string | null>(null);
  const stale = origin !== null && origin !== request;
  const begin = async () => {
    if (request === null || editing || request.projectRevision !== revision) return;
    setToken(""); setPassword(""); setConnectionStatus(null); setCapabilities(null); setTransport(null); setUploaded(null);
    setFirmware(null); setInstallProgress(null);
    setOrigin(request); setPorts([]); setSelected([]); setError(null); setSaved(null); setPending(true);
    try {
      const result = await commands.sequenceExportPorts(request);
      if (result.status === "error") throw new Error(result.error);
      setPorts(result.data);
      const image = await commands.deviceFirmwareInfo();
      if (image.status === "error") throw new Error(image.error);
      setFirmware(image.data);
      const devices = await commands.deviceSerialPorts();
      if (devices.status === "error") throw new Error(devices.error);
      setSerialPorts(devices.data); setSerialPort(""); setEraseConfirmed(false); setInstallConfirmed(false);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(false); }
  };
  const save = async () => {
    if (origin === null || stale) return;
    setPending(true); setError(null); setSaved(null);
    try {
      const result = await commands.exportSequenceFile(origin, selected);
      if (result.status === "error") throw new Error(result.error);
      setSaved(result.data);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(false); }
  };
  const readTransport = async (deviceAddress: string, deviceToken: string) => {
    const result = await commands.deviceTransport(deviceAddress, deviceToken, null);
    if (result.status === "error") throw new Error(result.error);
    setTransport(result.data);
  };
  const changeTransport = async (mode: DevicePlaybackMode | null) => {
    setPending(true); setError(null); setTransport(null);
    try {
      const result = await commands.deviceTransport(address, token, mode);
      if (result.status === "error") throw new Error(result.error);
      setTransport(result.data);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(false); }
  };
  const checkDevice = async () => {
    setPending(true); setError(null); setCapabilities(null); setTransport(null); setUploaded(null);
    try {
      const result = await commands.deviceCapabilities(address, token);
      if (result.status === "error") throw new Error(result.error);
      setCapabilities(result.data);
      if (result.data.output.type === "ws281x") await readTransport(address, token);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(false); }
  };
  const upload = async () => {
    if (origin === null || stale) return;
    setPending(true); setError(null); setUploaded(null); setTransport(null);
    try {
      const result = await commands.uploadSequenceDevice(origin, selected, address, token);
      if (result.status === "error") throw new Error(result.error);
      setUploaded(result.data);
      await readTransport(address, token);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(false); }
  };
  const refreshDevices = async () => {
    setSerialPort(""); setEraseConfirmed(false); setInstallConfirmed(false);
    setPending(true); setError(null);
    try {
      const result = await commands.deviceSerialPorts();
      if (result.status === "error") throw new Error(result.error);
      setSerialPorts(result.data); setSerialPort(""); setEraseConfirmed(false); setInstallConfirmed(false);
    } catch (error: unknown) { setError(String(error)); }
    finally { setPending(false); }
  };
  const provision = async () => {
    setEraseConfirmed(false); setInstallConfirmed(false);
    setPending(true); setError(null); setCapabilities(null); setTransport(null); setUploaded(null); setToken("");
    setConnectionStatus("Resetting device and connecting to Wi-Fi. This can take up to 85 seconds.");
    try {
      const result = await commands.provisionDevice(serialPort, ssid, password);
      if (result.status === "error") throw new Error(result.error);
      setAddress(result.data.address); setToken(result.data.token);
      setConnectionStatus("Wi-Fi connected. Checking device capabilities...");
      const checked = await commands.deviceCapabilities(result.data.address, result.data.token);
      if (checked.status === "error") throw new Error(checked.error);
      setCapabilities(checked.data);
      if (checked.data.output.type === "ws281x") await readTransport(result.data.address, result.data.token);
      setConnectionStatus("Device connected. Select outputs, then upload and play.");
    } catch (error: unknown) { setConnectionStatus(null); setError(String(error)); }
    finally { setPassword(""); setPending(false); }
  };
  const installFirmware = async () => {
    if (!installConfirmed || serialPort === "" || firmware === null) return;
    setPending(true); setError(null); setCapabilities(null); setTransport(null);
    setToken(""); setPassword(""); setAddress(""); setUploaded(null); setConnectionStatus(null);
    setEraseConfirmed(false); setInstallConfirmed(false); setInstallProgress({ stage: "connecting" });
    const progress = new Channel<DeviceInstallProgress>();
    progress.onmessage = setInstallProgress;
    try {
      const result = await commands.installDeviceFirmware(serialPort, progress);
      if (result.status === "error") throw new Error(result.error);
      setConnectionStatus("Firmware installed and verified. Enter Wi-Fi credentials and connect below.");
    } catch (error: unknown) { setError(String(error)); }
    finally { setInstallProgress(null); setPending(false); }
  };
  const eraseSavedData = async () => {
    if (!eraseConfirmed || serialPort === "") return;
    setPending(true); setError(null); setCapabilities(null); setTransport(null);
    setToken(""); setPassword(""); setAddress(""); setUploaded(null); setEraseConfirmed(false); setInstallConfirmed(false);
    setConnectionStatus("Erasing saved controller data. This can take up to 65 seconds.");
    try {
      const result = await commands.eraseDeviceSavedData(serialPort);
      if (result.status === "error") throw new Error(result.error);
      setConnectionStatus("Saved controller data erased. Enter Wi-Fi credentials and connect again.");
    } catch (error: unknown) { setConnectionStatus(null); setError(String(error)); }
    finally { setPending(false); }
  };
  const close = () => { setEraseConfirmed(false); setInstallConfirmed(false); setOrigin(null); setToken(""); setPassword(""); setCapabilities(null); setTransport(null); };
  return <>
    <button type="button" title="Export compiled sequence" disabled={pending || editing || request === null || request.projectRevision !== revision} onClick={() => { void begin(); }}><Download size={THEME_METRICS.iconSizeCompact} /></button>
    <Dialog.Root open={origin !== null} onOpenChange={(open) => { if (!open && !pending) close(); }}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content sequence-export-dialog">
          <Dialog.Title>Export compiled sequence</Dialog.Title>
          <Dialog.Description>Choose the outputs to include. Their selection order becomes the controller payload's output order.</Dialog.Description>
          {stale && <p role="alert">The project changed. Close and reopen export to use the current sequence and outputs.</p>}
          {error !== null && <p role="alert">{error}</p>}
          {saved !== null && <p role="status">Saved {saved}</p>}
          {uploaded !== null && <p role="status">{uploaded}</p>}
          <fieldset className="sequence-export-ports" disabled={pending || stale}>
            <legend>Output ports</legend>
            {ports.map((port) => <label key={port.index}>
              <input type="checkbox" checked={selected.includes(port.index)} onChange={(event) => { setSaved(null); setSelected(event.target.checked ? [...selected, port.index] : selected.filter((index) => index !== port.index)); }} />
              {port.label} ({port.channels} channels){selected.includes(port.index) ? ` · payload output ${selected.indexOf(port.index) + 1}` : ""}
            </label>)}
            {!pending && ports.length === 0 && <p>Add controller outputs in Display Setup before exporting.</p>}
          </fieldset>
          <fieldset className="sequence-device-form" disabled={pending}>
            <legend>Connect device over USB</legend>
            <label>USB serial device<select value={serialPort} onChange={(event) => { setSerialPort(event.target.value); setEraseConfirmed(false); setInstallConfirmed(false); }}>
              <option value="" disabled>Choose a device</option>
              {serialPorts.map((port) => <option key={port.path} value={port.path}>{port.label}</option>)}
            </select></label>
            <button type="button" onClick={() => { void refreshDevices(); }}>Refresh USB devices</button>
            {!pending && serialPorts.length === 0 && <p>No serial devices found. Connect the controller with a data-capable USB cable.</p>}
            {firmware !== null && <p>Dawn {firmware.version} firmware is included. Supports dual-core ESP32 controllers with 4 MB flash and a 40 MHz crystal. Installation can take several minutes. Keep Dawn open and the USB cable connected.</p>}
            <label className="device-erase-confirmation"><input type="checkbox" checked={installConfirmed} onChange={(event) => { setInstallConfirmed(event.target.checked); }} />Replace firmware on the selected controller with Dawn. This overwrites its current software.</label>
            <button type="button" disabled={serialPort === "" || !installConfirmed || firmware === null} onClick={() => { void installFirmware(); }}>Install Dawn firmware</button>
            {installProgress !== null && <p role="status">{installProgress.stage === "connecting" ? "Connecting to controller bootloader..."
              : installProgress.stage === "writing" ? `Installing firmware: ${Math.round(100 * installProgress.completed / Math.max(1, installProgress.total))}%`
              : installProgress.stage === "verifying" ? "Verifying written firmware..." : "Restarting controller..."}</p>}
            <label>Wi-Fi network name<input value={ssid} onChange={(event) => { setSsid(event.target.value); }} autoComplete="off" /></label>
            <label>Wi-Fi password<input type="password" value={password} onChange={(event) => { setPassword(event.target.value); }} autoComplete="off" /></label>
            <p>Use a 2.4 GHz WPA2 personal network. Provisioning resets the selected device. Wi-Fi credentials and the uploaded sequence are saved on the controller; a saved sequence restarts after reset.</p>
            <button type="button" disabled={serialPort === "" || ssid === "" || password === ""} onClick={() => { void provision(); }}>Reset and connect to Wi-Fi</button>
            <label className="device-erase-confirmation"><input type="checkbox" checked={eraseConfirmed} onChange={(event) => { setEraseConfirmed(event.target.checked); }} />Erase the selected controller's saved Wi-Fi credentials, token, and sequence. This cannot be undone.</label>
            <button type="button" disabled={serialPort === "" || !eraseConfirmed} onClick={() => { void eraseSavedData(); }}>Erase saved controller data</button>
            {connectionStatus !== null && <p role="status">{connectionStatus}</p>}
          </fieldset>
          <fieldset className="sequence-device-form" disabled={pending || stale}>
            <legend>Upload to a provisioned device</legend>
            <label>Device IP address and port<input value={address} placeholder="192.168.1.50:80" onChange={(event) => { setAddress(event.target.value); setCapabilities(null); setTransport(null); setUploaded(null); }} /></label>
            <label>Device token<input type="password" autoComplete="off" value={token} onChange={(event) => { setToken(event.target.value); setCapabilities(null); setTransport(null); setUploaded(null); }} /></label>
            <button type="button" disabled={address === "" || token === ""} onClick={() => { void checkDevice(); }}>Check device</button>
            {capabilities !== null && <p>{capabilities.output.type === "ws281x"
              ? `${capabilities.output.lanes} outputs, up to ${capabilities.output.channelsPerLane} channels each in multiples of ${capabilities.output.channelMultiple}, at ${capabilities.output.frameRate} Hz.`
              : "This firmware cannot drive lights."} Maximum sequence payload: {capabilities.maxPayloadBytes} bytes. Storage: persistent flash.</p>}
            <p>Connect over USB above, or enter the address and token of an already provisioned device. Upload replaces the running sequence and starts playback. Output selection order maps to physical device lanes. Audio is not uploaded.</p>
            <button type="button" disabled={selected.length === 0 || address === "" || token === "" || capabilities?.output.type !== "ws281x"} onClick={() => { void upload(); }}>Upload and play</button>
          </fieldset>
          {capabilities?.output.type === "ws281x" && <fieldset className="sequence-device-form" disabled={pending}>
            <legend>Device playback</legend>
            <p role="status">{transport === null ? "Playback status unknown. Refresh to check the device."
              : transport.playback === null ? "No sequence loaded on device."
              : `Device ${transport.playback.mode} at ${(transport.playback.positionMicros / 1_000_000).toFixed(2)} / ${(transport.playback.durationMicros / 1_000_000).toFixed(2)} seconds when last checked.`}</p>
            <div className="dialog-actions">
              <button type="button" onClick={() => { void changeTransport(null); }}>Refresh playback status</button>
              <button type="button" disabled={transport === null || transport.playback === null || transport.playback.mode === "playing"} onClick={() => { void changeTransport("playing"); }}>Play device</button>
              <button type="button" disabled={transport?.playback?.mode !== "playing"} onClick={() => { void changeTransport("paused"); }}>Pause device</button>
              <button type="button" disabled={transport === null || transport.playback === null || transport.playback.mode === "stopped"} onClick={() => { void changeTransport("stopped"); }}>Stop device</button>
            </div>
            <p>Pause holds the current frame. Stop clears the lights and rewinds. Playback loops on the device independently of the editor. Changes follow frames already queued for output.</p>
          </fieldset>}
          <div className="dialog-actions">
            <button type="button" disabled={pending} onClick={close}>Close</button>
            <button type="button" disabled={pending || stale || selected.length === 0} onClick={() => { void save(); }}>{pending ? "Preparing…" : "Save .dawnseq file"}</button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  </>;
}

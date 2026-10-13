import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupDocument } from "../../../types";

export function AvailableInputDevices({ document }: { document: SetupDocument }) {
  if (document.availableInputDevices.length === 0) return null;
  return <div className="setup-controller-library">
    <h4>Available input devices</h4>
    {document.availableInputDevices.map((device) => <div className="setup-summary" key={JSON.stringify(device.sourceRef)}>
      <span>{device.name}{device.readOnly ? " (unavailable source)" : ""}</span>
      <button type="button" onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "attachInputDevice", device: device.sourceRef }))}>Use in this setup</button>
    </div>)}
  </div>;
}

export function SetupInputDeviceActions({ device }: { device: SetupDocument["inputDevices"][number] }) {
  return <div className="setup-controller-membership">
    <p>{device.sourceRef.ownedPath.length > 0 ? "Removing this input device deletes its owned contents." : "Removing this link keeps the reusable input device source."} A rig of the project cannot play from a device the setup does not list.</p>
    <button type="button" onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "detachInputDevice", device: device.sourceRef }))}>Remove from setup</button>
  </div>;
}

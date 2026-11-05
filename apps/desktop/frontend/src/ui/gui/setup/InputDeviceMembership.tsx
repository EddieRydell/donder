import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupGuiDocument } from "../../../types";
import { Button, Section, type MenuEntry } from "../../kit";

export function AvailableInputDevices({ document }: { document: SetupGuiDocument }) {
  if (document.availableInputDevices.length === 0) return null;
  return <Section title="Available">
    {document.availableInputDevices.map((device) => <div className="setup-summary" key={JSON.stringify(device.sourceRef)}>
      <span>{device.name}</span>
      <Button tip="Use in this setup" onClick={() => void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setup", edit: { type: "attachInputDevice", device: device.sourceRef } }))}>Use</Button>
    </div>)}
  </Section>;
}

/** Removing an input device from the setup; consoles play only from devices the setup lists. */
export function inputDeviceEntries(device: SetupGuiDocument["inputDevices"][number]): MenuEntry[] {
  return [{ type: "action", label: "Remove from setup", danger: true, onSelect: () => void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setup", edit: { type: "detachInputDevice", device: device.sourceRef } })) }];
}

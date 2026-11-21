import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupGuiDocument } from "../../../types";
import { Button, Section } from "../../kit";

export function AvailableInputDevices({ document }: { document: SetupGuiDocument }) {
  if (document.availableInputDevices.length === 0) return null;
  return <Section title="Available">
    {document.availableInputDevices.map((device) => <div className="setup-summary" key={JSON.stringify(device.sourceRef)}>
      <span>{device.name}</span>
      <Button tip="Use in this setup" onClick={() => void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setup", edit: { type: "attachInputDevice", device: device.sourceRef } }))}>Use</Button>
    </div>)}
  </Section>;
}


import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { InputDeviceGuiDocument } from "../../../types";
import { InputDeviceForm } from "./InputDeviceForm";

export function InputDeviceEditor({ document }: { document: InputDeviceGuiDocument }) {
  return <main className="setup-editor">
    <header className="object-overview-header"><div><span className="object-overview-eyebrow">{document.path}</span><h2>{document.device.name}</h2></div></header>
    <section className="setup-section"><InputDeviceForm
      device={document.device}
      keyCodes={document.keyCodes}
      limits={document.inputLimits}
      onSave={async (driver, controls) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "inputDevice", driver, controls })); }}
    /></section>
  </main>;
}

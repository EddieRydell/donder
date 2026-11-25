import type { GuiEdit, GuiInputControl, GuiInputDriver, InputDeviceGuiDocument } from "../../../types";
import { InputDeviceForm } from "./InputDeviceForm";
import { site } from "../edits";

const deviceEdit = (driver: GuiInputDriver, controls: GuiInputControl[]): GuiEdit => ({ type: "put", sites: [site(null)], value: { type: "inputDevice", driver, controls } });

export function InputDeviceEditor({ document }: { document: InputDeviceGuiDocument }) {
  return <main className="object-editor">
    <header className="object-overview-header"><h2>{document.device.name}</h2></header>
    <section className="object-section input-device-section"><InputDeviceForm
      device={document.device}
      keyCodes={document.keyCodes}
      limits={document.inputLimits}
      edit={deviceEdit}
    /></section>
  </main>;
}

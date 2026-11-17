import type { GuiEditCommand, GuiInputControl, GuiInputDriver, InputDeviceGuiDocument } from "../../../types";
import { InputDeviceForm } from "./InputDeviceForm";

const deviceEdit = (driver: GuiInputDriver, controls: GuiInputControl[]): GuiEditCommand => ({ type: "inputDevice", driver, controls });

export function InputDeviceEditor({ document }: { document: InputDeviceGuiDocument }) {
  return <main className="setup-editor">
    <header className="object-overview-header"><h2>{document.device.name}</h2></header>
    <section className="setup-section input-device-section"><InputDeviceForm
      device={document.device}
      keyCodes={document.keyCodes}
      limits={document.inputLimits}
      edit={deviceEdit}
    /></section>
  </main>;
}

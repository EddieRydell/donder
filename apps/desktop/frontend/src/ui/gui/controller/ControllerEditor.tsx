import type { GuiDocument, GuiEditCommand, SetupControllerConfig, SetupControllerPort } from "../../../types";
import { ControllerForm } from "./ControllerForm";
import { DonderDevicePanel } from "./DonderDevicePanel";
import { OutputTestForm } from "./OutputTestForm";

const controllerEdit = (config: SetupControllerConfig, ports: SetupControllerPort[]): GuiEditCommand => ({ type: "controller", config, ports });

export function ControllerEditor({ document }: { document: Extract<GuiDocument, { type: "controller" }>["document"] }) {
  const config = document.controller.config;
  return <main className="setup-editor">
    <header className="object-overview-header"><h2>{document.controller.label}</h2></header>
    <section className="setup-section"><ControllerForm
      key={JSON.stringify([config, document.controller.ports])}
      controller={document.controller}
      edit={controllerEdit}
    /></section>
    {config.type === "donder" ? <DonderDevicePanel deviceId={config.device} /> : <OutputTestForm universes={config.universes} />}
  </main>;
}

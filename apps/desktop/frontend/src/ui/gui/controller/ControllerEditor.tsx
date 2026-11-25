import type { GuiDocument, GuiEdit, SetupControllerConfig, SetupControllerPort } from "../../../types";
import { ControllerForm } from "./ControllerForm";
import { DonderDevicePanel } from "./DonderDevicePanel";
import { OutputTestForm } from "./OutputTestForm";
import { site } from "../edits";

const controllerEdit = (config: SetupControllerConfig, ports: SetupControllerPort[]): GuiEdit => ({ type: "put", sites: [site(null)], value: { type: "controller", config, ports } });

export function ControllerEditor({ document }: { document: Extract<GuiDocument, { type: "controller" }>["document"] }) {
  const config = document.controller.config;
  return <main className="object-editor">
    <header className="object-overview-header"><h2>{document.controller.label}</h2></header>
    <section className="object-section"><ControllerForm
      controller={document.controller}
      edit={controllerEdit}
    /></section>
    {config.type === "donder" ? <DonderDevicePanel deviceId={config.device} /> : <OutputTestForm universes={config.universes} />}
  </main>;
}

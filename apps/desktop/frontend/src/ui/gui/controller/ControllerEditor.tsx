import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { GuiDocument } from "../../../types";
import { ControllerForm } from "./ControllerForm";
import { DonderDevicePanel } from "./DonderDevicePanel";
import { OutputTestForm } from "./OutputTestForm";

export function ControllerEditor({ document }: { document: Extract<GuiDocument, { type: "controller" }>["document"] }) {
  const config = document.controller.config;
  return <main className="setup-editor">
    <header className="object-overview-header"><div><span className="object-overview-eyebrow">{document.path}</span><h2>{document.controller.label}</h2></div></header>
    <section className="setup-section"><ControllerForm
      key={JSON.stringify([config, document.controller.ports])}
      controller={document.controller}
      onSave={async (config, ports) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "controller", config, ports })); }}
    /></section>
    {config.type === "donder" ? <DonderDevicePanel deviceId={config.device} /> : <OutputTestForm document={document.controller} />}
  </main>;
}

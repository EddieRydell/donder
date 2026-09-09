import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { GuiDocument } from "../../../types";
import { ControllerForm } from "./ControllerForm";

export function ControllerEditor({ document }: { document: Extract<GuiDocument, { type: "controller" }>["document"] }) {
  return <main className="setup-editor">
    <header className="object-overview-header"><div><span className="object-overview-eyebrow">{document.path}</span><h2>{document.objectKey}</h2></div></header>
    <section className="setup-section"><ControllerForm
      key={JSON.stringify([document.controller.config, document.controller.ports])}
      controller={document.controller}
      onSave={async (config, ports) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "controller", config, ports })); }}
    /></section>
  </main>;
}

import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupGuiDocument } from "../../../types";
import { Button, Section } from "../../kit";

export function AvailableControllers({ document }: { document: SetupGuiDocument }) {
  if (document.availableControllers.length === 0) return null;
  return <Section title="Available">
    {document.availableControllers.map((controller) => <div className="setup-summary" key={JSON.stringify(controller.sourceRef)}>
      <span>{controller.label}</span>
      <Button tip="Use in this setup" onClick={() => void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setup", edit: { type: "attachController", controller: controller.sourceRef } }))}>Use</Button>
    </div>)}
  </Section>;
}


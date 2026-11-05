import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupGuiDocument } from "../../../types";
import { Button, Section, type MenuEntry } from "../../kit";

export function AvailableControllers({ document }: { document: SetupGuiDocument }) {
  if (document.availableControllers.length === 0) return null;
  return <Section title="Available">
    {document.availableControllers.map((controller) => <div className="setup-summary" key={JSON.stringify(controller.sourceRef)}>
      <span>{controller.label}</span>
      <Button tip="Use in this setup" onClick={() => void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setup", edit: { type: "attachController", controller: controller.sourceRef } }))}>Use</Button>
    </div>)}
  </Section>;
}

/** Removing a controller from the setup, alone or with the outputs routed to it. */
export function controllerEntries(controller: SetupGuiDocument["controllers"][number]): MenuEntry[] {
  const detach = (removeOutputs: boolean) => void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setup", edit: { type: "detachController", controller: controller.sourceRef, removeOutputs } }));
  return [
    { type: "action", label: "Remove from setup", danger: true, onSelect: () => { detach(false); } },
    { type: "action", label: "Remove with outputs", danger: true, onSelect: () => { detach(true); } }
  ];
}

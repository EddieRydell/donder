import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupDocument } from "../../../types";

export function AvailableControllers({ document }: { document: SetupDocument }) {
  if (document.availableControllers.length === 0) return null;
  return <div className="setup-controller-library">
    <h4>Available controllers</h4>
    {document.availableControllers.map((controller) => <div className="setup-summary" key={JSON.stringify(controller.sourceRef)}>
      <span>{controller.label}{controller.readOnly ? " (read-only package source)" : ""}</span>
      <button type="button" onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "attachController", controller: controller.sourceRef }))}>Use in this setup</button>
    </div>)}
  </div>;
}

export function SetupControllerActions({ controller, patchReadOnly }: { controller: SetupDocument["controllers"][number]; patchReadOnly: boolean }) {
  return <div className="setup-controller-membership">
    <p>{controller.sourceRef.ownedPath.length > 0 ? "Removing this controller deletes its owned contents." : "Removing this link keeps the reusable controller source."}</p>
    <button type="button" onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "detachController", controller: controller.sourceRef, removeOutputs: false }))}>Remove from setup</button>
    <button type="button" disabled={patchReadOnly} onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "detachController", controller: controller.sourceRef, removeOutputs: true }))}>Remove controller and outputs</button>
  </div>;
}

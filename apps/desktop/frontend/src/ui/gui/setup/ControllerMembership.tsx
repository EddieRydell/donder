import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupDocument } from "../../../types";

export function AvailableControllers({ document }: { document: SetupDocument }) {
  if (document.availableControllers.length === 0) return null;
  return <div className="setup-controller-library">
    <h4>Existing project controllers</h4>
    {document.availableControllers.map((controller) => <div className="setup-summary" key={JSON.stringify(controller.sourceRef)}>
      <span>{controller.label}{controller.readOnly ? " (dependency)" : ""}</span>
      <button type="button" onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "attachController", controller: controller.sourceRef }))}>Use in this setup</button>
    </div>)}
  </div>;
}

export function SetupControllerActions({ controller, patchReadOnly }: { controller: SetupDocument["controllers"][number]; patchReadOnly: boolean }) {
  return <div className="setup-controller-membership">
    <p>Create an independent controller with the same settings and output assignments for this setup.</p>
    <button type="button" onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "copyController", controller: controller.sourceRef }))}>Create editable controller copy</button>
    <p>Removing a controller from this setup keeps its definition available for reuse.</p>
    <button type="button" onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "detachController", controller: controller.sourceRef, removeOutputs: false }))}>Remove from setup</button>
    <button type="button" disabled={patchReadOnly} onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "detachController", controller: controller.sourceRef, removeOutputs: true }))}>Remove controller and outputs</button>
  </div>;
}

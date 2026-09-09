import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupDocument } from "../../../types";

type Controller = SetupDocument["controllers"][number];

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

export function ControllerMembership({ controller, patchReadOnly }: { controller: Controller; patchReadOnly: boolean }) {
  const assigned = controller.assignments.length > 0;
  return <div className="setup-controller-membership">
    <p>Create an independent controller with the same settings and output assignments for this setup.</p>
    <button type="button" onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "copyController", controller: controller.sourceRef }))}>Create editable controller copy</button>
    <p>Removing a controller from this setup keeps its definition available for reuse.</p>
    <button type="button" disabled={assigned} title={assigned ? "Remove or reassign its outputs first." : undefined}
      onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "detachController", controller: controller.sourceRef, removeOutputs: false }))}>Remove from setup</button>
    {assigned && <button type="button" disabled={patchReadOnly}
      onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "detachController", controller: controller.sourceRef, removeOutputs: true }))}>Remove controller and outputs</button>}
  </div>;
}

export function ControllerUsage({ controller, portId }: { controller: Controller; portId: number }) {
  const port = controller.ports.find((candidate) => candidate.id === portId);
  if (port === undefined) return null;
  const assignments = controller.assignments.filter((assignment) => assignment.port === portId).sort((left, right) => left.startChannel - right.startChannel);
  const used = assignments.reduce((sum, assignment) => sum + assignment.channelCount, 0);
  const freeRanges: string[] = [];
  let firstFree = 1;
  for (const assignment of assignments) {
    if (firstFree < assignment.startChannel) freeRanges.push(`${firstFree}–${assignment.startChannel - 1}`);
    firstFree = assignment.startChannel + assignment.channelCount;
  }
  if (firstFree <= port.slotCount) freeRanges.push(`${firstFree}–${port.slotCount}`);
  return <div className="setup-controller-usage">
    <span>Port {port.id}: {used} of {port.slotCount} channels assigned</span>
    <span>Free channels: {freeRanges.length === 0 ? "none" : freeRanges.join(", ")}</span>
    {assignments.length > 0 && <span>Assigned channels: {assignments.map((assignment) => `${assignment.startChannel}–${assignment.startChannel + assignment.channelCount - 1}`).join(", ")}</span>}
  </div>;
}

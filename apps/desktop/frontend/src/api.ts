import { commands as generatedCommands } from "./generated/bindings";
import type {
  FixtureGuiEdit,
  GuiDocumentRequest,
  LayoutGuiEdit,
  SetupGuiEdit,
  SequenceGuiEdit
} from "./types";

export const commands = {
  ...generatedCommands,
  resolveGuiSource: async (moduleId: string, path: string, objectKey: string) =>
    unwrapResult(await generatedCommands.resolveGuiSource(moduleId, path, objectKey)),
  setLiveOutputActive: async (active: boolean) =>
    unwrapResult(await generatedCommands.setLiveOutputActive(active)),
  startOutputTest: async (request: GuiDocumentRequest, test: import("./types").ControllerOutputTest) =>
    unwrapResult(await generatedCommands.startOutputTest(request, test)),
  searchProject: async (request: import("./types").ProjectSearchRequest) =>
    unwrapResult(await generatedCommands.searchProject(request)),
  planWorkspacePathChange: async (request: import("./types").WorkspacePathChangeRequest) =>
    unwrapResult(await generatedCommands.planWorkspacePathChange(request)),
  applyWorkspacePathChange: async (request: import("./types").WorkspacePathChangeRequest) =>
    unwrapResult(await generatedCommands.applyWorkspacePathChange(request)),
  updateDocument: async (update: import("./types").DocumentUpdate) =>
    unwrapResult(await generatedCommands.updateDocument(update)),
  saveAll: async () => unwrapResult(await generatedCommands.saveAll()),
  requestTransition: async (request: import("./types").TransitionRequest) => unwrapResult(await generatedCommands.requestTransition(request)),
  reconcileExternalFiles: async () => unwrapResult(await generatedCommands.reconcileExternalFiles()),
  resolveExternalConflict: async (epoch: number, path: string, revision: number, decision: import("./types").ExternalConflictDecision) =>
    unwrapResult(await generatedCommands.resolveExternalConflict(epoch, path, revision, decision)),
  applySequenceGuiEdit: (request: GuiDocumentRequest, edit: SequenceGuiEdit) =>
    generatedCommands.applyGuiEdit(request, { type: "sequence", edit }),
  applySetupGuiEdit: (request: GuiDocumentRequest, edit: SetupGuiEdit) =>
    generatedCommands.applyGuiEdit(request, { type: "setup", edit }),
  applyLayoutGuiEdit: (request: GuiDocumentRequest, edit: LayoutGuiEdit) =>
    generatedCommands.applyGuiEdit(request, { type: "layout", edit }),
  applyFixtureGuiEdit: (request: GuiDocumentRequest, edit: FixtureGuiEdit) =>
    generatedCommands.applyGuiEdit(request, { type: "fixture", edit })
};

function unwrapResult<T>(result: { status: "ok"; data: T } | { status: "error"; error: string }): T {
  if (result.status === "error") throw new Error(result.error);
  return result.data;
}

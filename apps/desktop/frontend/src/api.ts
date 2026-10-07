import { commands as generatedCommands } from "./generated/bindings";
import type {
  DonderDeviceNetworkRequest,
  FixtureGuiEdit,
  GuiDocumentRequest,
  LayoutGuiEdit,
  SetupGuiEdit,
  SequenceGuiEdit
} from "./types";

export const commands = {
  ...generatedCommands,
  audioPlay: async () => unwrapResult(await generatedCommands.audioPlay()),
  audioPause: async () => unwrapResult(await generatedCommands.audioPause()),
  audioStop: async () => unwrapResult(await generatedCommands.audioStop()),
  audioRewindToZero: async () => unwrapResult(await generatedCommands.audioRewindToZero()),
  audioSeek: async (positionSeconds: number) => unwrapResult(await generatedCommands.audioSeek(positionSeconds)),
  audioSetPlaybackSpeed: async (speed: import("./types").PlaybackSpeed) =>
    unwrapResult(await generatedCommands.audioSetPlaybackSpeed(speed)),
  audioSetRange: async (range: import("./types").PlaybackRange | null) =>
    unwrapResult(await generatedCommands.audioSetRange(range)),
  audioSetLooping: async (looping: boolean) => unwrapResult(await generatedCommands.audioSetLooping(looping)),
  createSequence: async (request: import("./types").NewSequenceRequest) =>
    unwrapResult(await generatedCommands.createSequence(request)),
  setPreviewAppearance: async (appearance: import("./types").PreviewAppearance) =>
    unwrapResult(await generatedCommands.setPreviewAppearance(appearance)),
  resolveGuiSource: async (moduleId: string, path: string, objectKey: string) =>
    unwrapResult(await generatedCommands.resolveGuiSource(moduleId, path, objectKey)),
  setLiveOutputActive: async (active: boolean) =>
    unwrapResult(await generatedCommands.setLiveOutputActive(active)),
  startOutputTest: async (request: GuiDocumentRequest, test: import("./types").ControllerOutputTest) =>
    unwrapResult(await generatedCommands.startOutputTest(request, test)),
  claimDevice: async (id: string) => unwrapResult(await generatedCommands.claimDevice(id)),
  renameDevice: async (id: string, name: string) => unwrapResult(await generatedCommands.renameDevice(id, name)),
  setDeviceStandalone: async (id: string, playing: boolean) =>
    unwrapResult(await generatedCommands.setDeviceStandalone(id, playing)),
  setDeviceNetwork: async (id: string, network: DonderDeviceNetworkRequest | null) =>
    unwrapResult(await generatedCommands.setDeviceNetwork(id, network)),
  searchProject: async (request: import("./types").ProjectSearchRequest) =>
    unwrapResult(await generatedCommands.searchProject(request)),
  planWorkspacePathChange: async (request: import("./types").WorkspacePathChangeRequest) =>
    unwrapResult(await generatedCommands.planWorkspacePathChange(request)),
  applyWorkspacePathChange: async (request: import("./types").WorkspacePathChangeRequest) =>
    unwrapResult(await generatedCommands.applyWorkspacePathChange(request)),
  updateDocument: async (update: import("./types").DocumentUpdate) =>
    unwrapResult(await generatedCommands.updateDocument(update)),
  languageServerSend: async (message: string) =>
    unwrapResult(await generatedCommands.languageServerSend(message)),
  applyTextEdits: async (projectEpoch: number, edits: import("./types").DocumentTextEdits[]) =>
    unwrapResult(await generatedCommands.applyTextEdits(projectEpoch, edits)),
  includeDocument: async (projectEpoch: number, inclusion: import("./types").DocumentInclusion) =>
    unwrapResult(await generatedCommands.includeDocument(projectEpoch, inclusion)),
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

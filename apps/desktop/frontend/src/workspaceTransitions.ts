import { create } from "zustand";
import { commands, errorMessage } from "./api";
import { flushDocumentSync, useAppStore } from "./store";
import type { TransitionDecision, WorkspaceTransition } from "./types";
import { flushViewStateSaves } from "./viewStatePersistence";

type PendingDecision = { paths: string[]; decide: (decision: TransitionDecision) => void };
export const useTransitionStore = create<{ pending: PendingDecision | null; inProgress: boolean }>(() => ({ pending: null, inProgress: false }));
let activeTransition: Promise<boolean> | null = null;

export function runWorkspaceTransition(transition: WorkspaceTransition): Promise<boolean> {
  if (activeTransition !== null) return activeTransition;
  useTransitionStore.setState({ inProgress: true });
  activeTransition = performTransition(transition).catch((error: unknown) => {
    useAppStore.getState().reportFailure("command", errorMessage(error));
    return false;
  }).finally(() => {
    activeTransition = null;
    useTransitionStore.setState({ inProgress: false });
  });
  return activeTransition;
}

async function performTransition(transition: WorkspaceTransition): Promise<boolean> {
  await flushDocumentSync();
  await flushViewStateSaves();
  const current = useAppStore.getState().snapshot;
  if (current === null) return false;
  const request = { transition, projectEpoch: current.projectEpoch, projectRevision: current.projectRevision, decision: null as TransitionDecision | null };
  let result = await commands.requestTransition(request);
  useAppStore.getState().setSnapshot(result.snapshot);
  if (result.type === "needsDecision") {
    const decision = await new Promise<TransitionDecision>((resolve) => {
      useTransitionStore.setState({ pending: { paths: result.type === "needsDecision" ? result.dirtyPaths : [], decide: resolve } });
    });
    useTransitionStore.setState({ pending: null });
    await flushDocumentSync();
    result = await commands.requestTransition({ ...request, decision });
    useAppStore.getState().setSnapshot(result.snapshot);
  }
  if (result.type !== "applied") return false;
  if (result.closeApplication) {
    await flushDocumentSync();
    await flushViewStateSaves();
    await commands.completeClose(result.snapshot.projectEpoch, result.snapshot.projectRevision);
  }
  if (result.snapshot.projectRoot !== current.projectRoot) {
    useAppStore.getState().setRestoreState(await commands.getRestoredViewState());
  }
  return true;
}

export async function openProjectDialog() {
  const path = await commands.openProjectDialog();
  if (path !== null) await runWorkspaceTransition({ type: "openProject", path });
}

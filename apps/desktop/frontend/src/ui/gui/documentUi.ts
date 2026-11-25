import { useLayoutEffect, useState } from "react";
import { create } from "zustand";
import type { GuiMember, SequenceAutomationTarget, SequenceSelection } from "../../types";

/** What the inspector shows: a member, a graph edge, a mark, or an action of a console control by its event and
 * place among that event's actions. */
export type GuiFocus =
  | { type: "member"; member: GuiMember }
  | { type: "graphEdge"; edgeId: string }
  | { type: "mark"; collectionId: number; index: number }
  | { type: "consoleAction"; on: number; index: number }
  | null;

/** The parameter an envelope is being chosen for, by clicking one on the timeline. */
export type BindingChooser = { target: SequenceAutomationTarget } | null;

/** What the open sequence's or console's editor shows beside its document, until another document opens. */
export type DocumentUi = {
  focus: GuiFocus;
  selection: SequenceSelection | null;
  bindingChooser: BindingChooser;
  /** The mark collection new marks go into, by name. */
  activeMarks: string | null;
  /** The mark collections the timeline draws, by name. */
  visibleMarks: ReadonlySet<string>;
  graphOpen: boolean;
};

const CLOSED: DocumentUi = { focus: null, selection: null, bindingChooser: null, activeMarks: null, visibleMarks: new Set(), graphOpen: false };

/** The open editor's state, keyed by the editor that opened it; File > Export bumps `exportRequests`. */
export const useDocumentUi = create<DocumentUi & { key: string | null; exportRequests: number }>(() => ({ ...CLOSED, key: null, exportRequests: 0 }));

/** Change the open editor's state. */
export function setDocumentUi(change: Partial<DocumentUi>) {
  if (useDocumentUi.getState().key !== null) useDocumentUi.setState(change);
}

/** Holds the editor `key`'s state, starting from `initial`, while it is mounted; whether it is the open one. */
export function useOpenDocumentUi(key: string, initial: () => Partial<DocumentUi>): boolean {
  // The state starts once per editor; later renders keep it.
  const [start] = useState(initial);
  useLayoutEffect(() => {
    useDocumentUi.setState({ ...CLOSED, ...start, key });
    return () => { if (useDocumentUi.getState().key === key) useDocumentUi.setState({ ...CLOSED, key: null }); };
  }, [key, start]);
  return useDocumentUi((state) => state.key) === key;
}

export function openLayerGraph() {
  setDocumentUi({ graphOpen: true });
}

export function requestExport() {
  useDocumentUi.setState((state) => ({ exportRequests: state.exportRequests + 1 }));
}

/** The clip a focus names, if it names one. */
export function focusedClipId(focus: GuiFocus): number | null {
  return focus?.type === "member" && focus.member.type === "clip" ? focus.member.id : null;
}

/** A focus on clip `id`. */
export function clipFocus(id: number): GuiFocus {
  return { type: "member", member: { type: "clip", id } };
}

/** Show the mark collections `names`; with `activate`, the first takes new marks. */
export function showMarks(names: string[], activate: boolean) {
  const { visibleMarks } = useDocumentUi.getState();
  setDocumentUi({ visibleMarks: new Set([...visibleMarks, ...names]), ...(activate ? { activeMarks: names[0] ?? null } : {}) });
}

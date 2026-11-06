import type { SidebarView } from "../types";
import type { TextNavigation } from "../workspace/navigation";

/** Every window event the frontend sends itself, with its detail. */
type UiEvents = {
  newProject: undefined;
  copyProject: undefined;
  importProfile: undefined;
  newSequence: undefined;
  settings: undefined;
  openCommandPalette: undefined;
  openQuickOpen: undefined;
  focusSidebar: SidebarView;
  navigateToText: TextNavigation;
  openLayerGraph: undefined;
  /** Asks the open sequence editor to drop a mark at the playhead. */
  tapMark: undefined;
  /** A GUI undo or redo replaced the document. */
  guiHistoryChanged: undefined;
};

type UiEvent = keyof UiEvents;

const eventName = (event: UiEvent) => `donder:${event}`;

export function dispatchUiEvent<E extends UiEvent>(...[event, detail]: UiEvents[E] extends undefined ? [E] : [E, UiEvents[E]]) {
  window.dispatchEvent(new CustomEvent(eventName(event), { detail }));
}

/** Calls `handler` on each `event` until the returned function removes it. */
export function listenUiEvent<E extends UiEvent>(event: E, handler: (detail: UiEvents[E]) => void): () => void {
  const listener = (raw: Event) => { handler((raw as CustomEvent<UiEvents[E]>).detail); };
  window.addEventListener(eventName(event), listener);
  return () => { window.removeEventListener(eventName(event), listener); };
}

export function focusSidebar(view: SidebarView) {
  dispatchUiEvent("focusSidebar", view);
}

export function requestOpenLayerGraph() {
  dispatchUiEvent("openLayerGraph");
}

export function requestTapMark() {
  dispatchUiEvent("tapMark");
}

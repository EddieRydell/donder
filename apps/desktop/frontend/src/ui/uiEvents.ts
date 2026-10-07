export const OPEN_LAYER_GRAPH_EVENT = "donder:open-layer-graph";

export function requestOpenLayerGraph() {
  window.dispatchEvent(new CustomEvent(OPEN_LAYER_GRAPH_EVENT));
}

export const SHOW_MARK_COLLECTION_EVENT = "donder:show-mark-collection";

/** Asks the open sequence editor to show a mark collection, such as one created by tapping. */
export function requestShowMarkCollection(key: string) {
  window.dispatchEvent(new CustomEvent<string>(SHOW_MARK_COLLECTION_EVENT, { detail: key }));
}

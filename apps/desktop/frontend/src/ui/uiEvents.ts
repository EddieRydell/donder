export const OPEN_LAYER_GRAPH_EVENT = "donder:open-layer-graph";

export function requestOpenLayerGraph() {
  window.dispatchEvent(new CustomEvent(OPEN_LAYER_GRAPH_EVENT));
}

export const TAP_MARK_EVENT = "donder:tap-mark";

/** Asks the open sequence editor to drop a mark at the playhead. */
export function requestTapMark() {
  window.dispatchEvent(new CustomEvent(TAP_MARK_EVENT));
}

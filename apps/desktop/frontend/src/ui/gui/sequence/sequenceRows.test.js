import assert from "node:assert/strict";
import { after, test } from "node:test";
import { fileURLToPath, URL } from "node:url";
import { createServer } from "vite";
import { installThemeDom } from "../../../testSupport/themeDom.js";

installThemeDom();
const server = await createServer({
  configFile: false,
  root: fileURLToPath(new URL("../../../..", import.meta.url)),
  server: { middlewareMode: true, ws: false },
  optimizeDeps: { noDiscovery: true }
});
after(() => server.close());
const layout = await server.ssrLoadModule("/src/ui/gui/sequence/sequenceRows.ts");
const selection = await server.ssrLoadModule("/src/ui/gui/sequence/sequenceSelection.ts");
const { THEME_METRICS } = await server.ssrLoadModule("/src/theme.ts");
const mainHeight = THEME_METRICS.sequenceInitialLaneHeight;
const automationHeight = layout.automationLaneRowHeight(mainHeight);
const lanes = [10, 20, 30].map((fixture) => ({ target: { fixture }, label: String(fixture) }));
/** An envelope over `fixture`'s envelope row. */
const clip = (id, fixture, startSeconds = 2) => ({
  id, target: { fixture }, placement: { type: "timed", startSeconds, durationSeconds: 3 },
  content: { type: "envelope", curve: [], bindings: [] }
});
const effect = (id, fixture) => ({
  id, target: { fixture }, placement: { type: "timed", startSeconds: 1, durationSeconds: 4 },
  content: { type: "effect", layerId: 0 }
});
const targets = (clips) => clips.map((clip) => clip.target);
const heights = () => layout.restoreRowHeights(lanes, undefined, mainHeight);

test("each target retains exactly two logical rows and hidden automation heights survive history and restart", () => {
  const sizes = heights();
  sizes[10].clips = THEME_METRICS.sequenceMaxLaneHeight;
  sizes[10].automation = THEME_METRICS.automationRowMinHeight;
  const persisted = layout.persistRowHeights(sizes);
  const reordered = [lanes[1], lanes[0], lanes[2]];
  const restored = layout.restoreRowHeights(reordered, JSON.parse(JSON.stringify(persisted)), mainHeight);
  assert.deepEqual(restored[10], sizes[10]);
  for (const clips of [[clip(1, 10)], [], [clip(1, 10)]]) {
    const rows = layout.sequenceRowLayout(reordered, targets(clips), restored, mainHeight, automationHeight, false);
    assert.equal(rows.length, reordered.length * 2);
    const automation = rows.find((row) => row.target.fixture === 10 && row.kind === "automation");
    assert.equal(automation.laneIndex, 1);
    assert.equal(automation.height, clips.length === 0 ? 0 : sizes[10].automation);
    assert.deepEqual(layout.persistRowHeights(restored), persisted);
  }
  const revealed = layout.sequenceRowLayout(lanes, [], restored, mainHeight, automationHeight, true);
  assert.ok(revealed.every((row) => row.height > 0));
});

test("multiple envelopes share one target row and draft movement does not redefine hit-test geometry", () => {
  const clips = [clip(1, 10), clip(2, 10)];
  const document = { durationSeconds: 30, targets: lanes, clips, markCollections: [] };
  const rows = layout.sequenceRowLayout(lanes, targets(clips), heights(), mainHeight, automationHeight, true);
  const original = globalThis.structuredClone(rows);
  const viewport = { scrollXSeconds: 0, scrollY: 0, pxPerSecond: THEME_METRICS.sequenceInitialPixelsPerSecond };
  const bounds = { width: THEME_METRICS.sequenceMaxLaneHeight, height: THEME_METRICS.sequenceMaxLaneHeight };
  const drawn = selection.buildSequenceClipLayout(document, [], null, viewport, 0, 0, bounds, rows);
  assert.equal(drawn.length, 2);
  assert.notEqual(drawn[0].rect.y, drawn[1].rect.y);
  assert.ok(drawn.every((layout) => rows.find((row) => row.laneIndex === layout.laneIndex && row.kind === "automation").top <= layout.rect.y));
  const draft = { id: 1, startSeconds: 2, durationSeconds: 3, laneIndex: 1, contentStartSeconds: 2, contentDurationSeconds: 3 };
  const moved = selection.buildSequenceClipLayout(document, [draft], null, viewport, 0, 0, bounds, rows);
  assert.equal(moved.find((layout) => layout.clip.id === 1).clip.target.fixture, 20);
  assert.deepEqual(rows, original);
  assert.equal(rows.filter((row) => row.target.fixture === 10).length, 2);
});

test("mixed selections preserve both row kinds and move or resize with one constrained delta", () => {
  const placed = effect(1, 10);
  const automation = clip(9, 20);
  const document = { durationSeconds: 30, targets: lanes, clips: [placed, automation], markCollections: [] };
  let selected = selection.nextClipSelection(null, placed.id, false, false);
  selected = selection.nextClipSelection(selected, automation.id, true, false);
  assert.deepEqual(selected, { type: "clips", ids: [1, 9] });
  assert.deepEqual(selection.nextClipSelection(selected, 1, true, false), selected);
  const draftOf = (gesture, id) => gesture.clips.find((draft) => draft.id === id);
  const move = selection.clipSelectionGesture(document, selected, "none", 3, 0, 9);
  assert.equal(move.edit.laneDelta, 1);
  assert.equal(move.edit.anchorLane, 0);
  assert.equal(draftOf(move, 1).laneIndex, 1);
  assert.equal(draftOf(move, 9).laneIndex, 2);
  assert.equal(draftOf(move, 1).startSeconds, 4);
  assert.equal(draftOf(move, 9).startSeconds, 5);
  const resize = selection.clipSelectionGesture(document, selected, "right", 2, 0, 1, "crop");
  assert.equal(draftOf(resize, 1).durationSeconds, 6);
  assert.equal(draftOf(resize, 9).durationSeconds, 5);
  assert.equal(draftOf(resize, 9).contentDurationSeconds, 3);
  assert.equal(draftOf(resize, 9).laneIndex, 1);
  const surviving = selection.reconcileSequenceSelection({ ...document, clips: [automation] }, selected);
  assert.deepEqual(surviving, { type: "clips", ids: [9] });
  assert.equal(selection.reconcileSequenceSelection({ ...document, clips: [] }, selected), null);
});

test("a target shared by two groups draws its clips on both lanes and moves from the nearest copy", () => {
  // Group 1 holds 10 and 20; group 2 holds 10 again.
  const shared = [
    { target: { fixture: 1 }, label: "1", kind: "group", depth: 0, occurrences: 1 },
    { target: { fixture: 10 }, label: "10", kind: "fixture", depth: 1, occurrences: 2 },
    { target: { fixture: 20 }, label: "20", kind: "fixture", depth: 1, occurrences: 1 },
    { target: { fixture: 2 }, label: "2", kind: "group", depth: 0, occurrences: 1 },
    { target: { fixture: 10 }, label: "10", kind: "fixture", depth: 1, occurrences: 2 }
  ];
  const placed = effect(1, 10);
  const document = { durationSeconds: 30, targets: shared, clips: [placed], markCollections: [] };
  const sizes = layout.restoreRowHeights(shared, undefined, mainHeight);
  const rows = layout.sequenceRowLayout(shared, [], sizes, mainHeight, automationHeight, false);
  const viewport = { scrollXSeconds: 0, scrollY: 0, pxPerSecond: THEME_METRICS.sequenceInitialPixelsPerSecond };
  const drawn = selection.buildSequenceClipLayout(document, [], null, viewport, 0, 0, { width: THEME_METRICS.sequenceMaxLaneHeight, height: THEME_METRICS.sequenceMaxLaneHeight }, rows);
  assert.deepEqual(drawn.map((clip) => clip.laneIndex).sort(), [1, 4]);
  const both = selection.selectionFromMarqueeClips(drawn, { startX: 0, startY: 0, x: 10000, y: 10000 });
  assert.deepEqual(both.ids, [1]);
  const selected = { type: "clips", ids: [1] };
  // Moving up from the second copy reaches group 2, not group 1.
  const up = selection.clipSelectionGesture(document, selected, "none", 0, 4, -1);
  assert.equal(up.clips[0].laneIndex, 3);
  assert.equal(up.edit.anchorLane, 4);
  const draft = selection.buildSequenceClipLayout(document, up.clips, null, viewport, 0, 0, { width: THEME_METRICS.sequenceMaxLaneHeight, height: THEME_METRICS.sequenceMaxLaneHeight }, rows);
  assert.deepEqual(draft.map((clip) => clip.laneIndex), [3]);
  // Collapsing a group hides only the lanes under that copy.
  assert.deepEqual([...layout.collapsedLanes(shared, new Set([2]))], [4]);
});

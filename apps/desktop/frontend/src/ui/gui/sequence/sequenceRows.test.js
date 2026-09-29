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
const layout = await server.ssrLoadModule("/src/ui/gui/sequence/sequenceAutomationLayout.ts");
const selection = await server.ssrLoadModule("/src/ui/gui/sequence/sequenceSelection.ts");
const { THEME_METRICS } = await server.ssrLoadModule("/src/theme.ts");
const mainHeight = THEME_METRICS.sequenceInitialLaneHeight;
const automationHeight = layout.automationLaneRowHeight(mainHeight);
const lanes = [10, 20, 30].map((fixture) => ({ target: { fixture }, label: String(fixture) }));
const clip = (id, fixture, startSeconds = 2) => ({
  id, rowTarget: { fixture }, startSeconds, durationSeconds: 3,
  curve: [], bindings: [], detachedBindings: []
});
const heights = () => layout.restoreRowHeights(lanes, undefined, mainHeight);

test("each target retains exactly two logical rows and hidden automation heights survive history and restart", () => {
  const sizes = heights();
  sizes[10].effects = THEME_METRICS.sequenceMaxLaneHeight;
  sizes[10].automation = THEME_METRICS.sequenceMinLaneHeight;
  const persisted = layout.persistRowHeights(sizes);
  const reordered = [lanes[1], lanes[0], lanes[2]];
  const restored = layout.restoreRowHeights(reordered, JSON.parse(JSON.stringify(persisted)), mainHeight);
  assert.deepEqual(restored[10], sizes[10]);
  for (const clips of [[clip(1, 10)], [], [clip(1, 10)]]) {
    const rows = layout.sequenceRowLayout(reordered, clips, restored, mainHeight, automationHeight, false);
    assert.equal(rows.length, reordered.length * 2);
    const automation = rows.find((row) => row.target.fixture === 10 && row.kind === "automation");
    assert.equal(automation.laneIndex, 1);
    assert.equal(automation.height, clips.length === 0 ? 0 : sizes[10].automation);
    assert.deepEqual(layout.persistRowHeights(restored), persisted);
  }
  const revealed = layout.sequenceRowLayout(lanes, [], restored, mainHeight, automationHeight, true);
  assert.ok(revealed.every((row) => row.height > 0));
});

test("multiple automation clips share one target row and draft movement does not redefine hit-test geometry", () => {
  const clips = [clip(1, 10), clip(2, 10)];
  const rows = layout.sequenceRowLayout(lanes, clips, heights(), mainHeight, automationHeight, true);
  const original = globalThis.structuredClone(rows);
  const drawn = layout.buildAutomationClipLayout(clips, rows, { scrollXSeconds: 0, scrollY: 0, pxPerSecond: THEME_METRICS.sequenceInitialPixelsPerSecond }, 0, 0, { width: THEME_METRICS.sequenceMaxLaneHeight, height: THEME_METRICS.sequenceMaxLaneHeight });
  assert.equal(drawn.length, 2);
  assert.notEqual(drawn[0].rect.y, drawn[1].rect.y);
  const drafts = layout.automationClipsWithDrafts(clips, [{ ...clips[0], rowTarget: { fixture: 20 } }], null);
  assert.equal(drafts[0].rowTarget.fixture, 20);
  assert.deepEqual(rows, original);
  assert.equal(rows.filter((row) => row.target.fixture === 10).length, 2);
});

test("mixed selections preserve both row kinds and move or resize with one constrained delta", () => {
  const effect = { id: 1, target: { fixture: 10 }, startSeconds: 1, durationSeconds: 4 };
  const automation = clip(9, 20);
  const document = { durationSeconds: 30, lanes, effects: [effect], automationClips: [automation], markCollections: [] };
  let selected = selection.nextEffectSelection(null, effect.id, false, false);
  selected = selection.nextAutomationSelection(selected, automation.id, true, false);
  assert.deepEqual(selected, { type: "clips", effectIds: [1], automationIds: [9] });
  assert.deepEqual(selection.nextEffectSelection(selected, 1, true, false), selected);
  const move = selection.clipSelectionGesture(document, selected, "none", 3, 9);
  assert.equal(move.edit.laneDelta, 1);
  assert.equal(move.effects[0].laneIndex, 1);
  assert.equal(move.automation[0].rowTarget.fixture, 30);
  assert.equal(move.effects[0].startSeconds, 4);
  assert.equal(move.automation[0].startSeconds, 5);
  const resize = selection.clipSelectionGesture(document, selected, "right", 2, 1);
  assert.equal(resize.effects[0].durationSeconds, 6);
  assert.equal(resize.automation[0].durationSeconds, 5);
  assert.equal(resize.automation[0].rowTarget.fixture, 20);
  const surviving = selection.reconcileSequenceSelection({ ...document, effects: [] }, selected);
  assert.deepEqual(surviving, { type: "clips", effectIds: [], automationIds: [9] });
  assert.equal(selection.reconcileSequenceSelection({ ...document, effects: [], automationClips: [] }, selected), null);
  assert.deepEqual(selection.selectionFromSingle({ type: "automationClip", id: 9 }), { type: "clips", effectIds: [], automationIds: [9] });
});

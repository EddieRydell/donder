import assert from "node:assert/strict";
import test from "node:test";
import { constrainPoint, constrainedMove, snapPoint, spatialUnits, visibleGridStep } from "./spatialSnapping.ts";
import { arrange, boxSelection, fixtureItems, layoutItems, moveLayout, repeatOffsets, selectedItems, selectionClick } from "./spatialSelection.ts";
const p = (x, y, z = 0) => ({ xMeters: x, yMeters: y, zMeters: z });
const settings = { enabled: true, spacingMeters: 0.1, unit: "meters" };
const none = { shiftKey: false, ctrlKey: false, metaKey: false };
const shift = { ...none, shiftKey: true };
const close = (a, b) => { assert.ok(Math.abs(a - b) < 1e-9, `${a} != ${b}`); };

test("grid snapping handles negative coordinates, imperial increments, and bypass", () => {
  assert.deepEqual(constrainPoint(p(-0.26, 0.34, 2), settings, none), p(-0.3, 0.3, 2));
  const inch = spatialUnits.inches.meters;
  const snapped = constrainPoint(p(inch * 2.2, inch * 4.7), { ...settings, spacingMeters: inch }, none);
  close(snapped.xMeters, inch * 2); close(snapped.yMeters, inch * 5);
  for (const bypass of [{ ...none, ctrlKey: true }, { ...none, metaKey: true }]) assert.deepEqual(constrainPoint(p(0.123, -0.257), settings, bypass), p(0.123, -0.257));
  assert.throws(() => constrainPoint(p(1, 1), { ...settings, spacingMeters: 0 }, none));
});
test("Shift preserves off-grid anchors and constrains every octant; square grids remain square", () => {
  const anchor = p(0.03, -0.07);
  for (let octant = 0; octant < 8; octant++) {
    const angle = octant * Math.PI / 4;
    const point = constrainPoint(p(anchor.xMeters + Math.cos(angle + 0.08), anchor.yMeters + Math.sin(angle + 0.08)), settings, shift, anchor);
    const dx = point.xMeters - anchor.xMeters; const dy = point.yMeters - anchor.yMeters;
    close(dx * Math.sin(angle) - dy * Math.cos(angle), 0);
    close(dx / settings.spacingMeters, Math.round(dx / settings.spacingMeters));
    close(dy / settings.spacingMeters, Math.round(dy / settings.spacingMeters));
  }
  const square = constrainPoint(p(-1, 0.3), settings, shift, p(0, 0), true);
  close(-square.xMeters, square.yMeters);
  assert.deepEqual(constrainedMove(anchor, p(0, 0), settings, none), p(0, 0));
});
test("endpoint and guide priority preserve constraints and bypass", () => {
  const targets = [{ owner: 1, position: p(0.253, 0.253, 7) }];
  assert.deepEqual(snapPoint(p(0.26, 0.25, 3), settings, shift, targets, [], 0.04, p(0, 0)), p(0.253, 0.253, 3));
  const offAxis = [{ owner: 1, position: p(0.25, 0.22) }];
  const snapped = snapPoint(p(0.26, 0.25), settings, shift, offAxis, [], 0.1, p(0, 0));
  close(snapped.xMeters, snapped.yMeters);
  const guides = [{ axis: "x", positionMeters: 0.253 }];
  const guided = snapPoint(p(0.26, 0.25), settings, shift, [], guides, 0.05, p(0, 0));
  close(guided.xMeters, 0.253); close(guided.yMeters, 0.253);
  assert.deepEqual(snapPoint(p(0.26, 0.25), settings, { ...none, ctrlKey: true }, targets, guides, 0.1), p(0.26, 0.25));
  assert.ok(visibleGridStep(0.001, 10, 36) * 10 >= 36);
});
const transform = (x, y = 0) => ({ position: p(x, y), rotation: { xDegrees: 0, yDegrees: 0, zDegrees: 0 }, scale: { x: 1, y: 1, z: 1 } });
const fixture = (id, x) => ({ id, name: `Fixture ${id}`, kind: { type: "fixture", transform: transform(x), definition: { type: "inline", elements: [] } } });
const group = (id, children) => ({ id, name: `Group ${id}`, kind: { type: "group", children } });
test("nested group selection moves each descendant once and preserves source geometry", () => {
  const fixtures = [group(10, [fixture(1, 1), group(11, [fixture(2, 3)])]), fixture(3, 8)];
  const items = layoutItems(fixtures, { pixels: [] });
  const selected = selectedItems(items, [1, 2, 10, 11, 3]);
  assert.deepEqual(selected.map((item) => item.id), [10, 3]);
  const moved = moveLayout(fixtures, selected.map((item) => ({ id: item.id, delta: p(2, -1) })));
  assert.deepEqual(moved[0].kind.children[0].kind.transform.position, p(3, -1));
  assert.deepEqual(moved[0].kind.children[1].kind.children[0].kind.transform.position, p(5, -1));
  assert.deepEqual(moved[1].kind.transform.position, p(10, -1));
  assert.equal(fixtures[0].kind.children[0].kind.transform.position.xMeters, 1);
  assert.equal(moved[0].kind.children[0].kind.definition, fixtures[0].kind.children[0].kind.definition);
});
test("alignment, equal gaps, and marquee use visible pixel bounds", () => {
  const elements = [1, 2, 3].map((id) => ({ id, transform: transform(0) }));
  const pixels = [{ owner: 1, position: p(1, 1), diameterMeters: 2 }, { owner: 2, position: p(5, 2), diameterMeters: 4 }, { owner: 3, position: p(12, 1), diameterMeters: 2 }];
  const items = fixtureItems(elements, { pixels });
  assert.deepEqual(arrange(items, "left").map((move) => move.delta.xMeters), [0, -3, -11]);
  assert.deepEqual(arrange(items, "spaceX").map((move) => move.delta.xMeters), [0, 1.5, 0]);
  assert.deepEqual(boxSelection(items, { left: 0, right: 7, bottom: 0, top: 4 }), [1, 2]);
  assert.deepEqual(selectionClick([1, 2], 2, true), [1]);
  assert.deepEqual(selectionClick([1], 3, true), [1, 3]);
  assert.deepEqual(repeatOffsets(2, 2, 3, -4), [p(3, 0), p(0, -4), p(3, -4)]);
  assert.throws(() => repeatOffsets(100, 100, 1, 1));
});

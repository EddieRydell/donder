import assert from "node:assert/strict";
import test from "node:test";
import { setImmediate } from "node:timers";
import { isNewerSnapshot } from "./snapshotState.ts";
import { DocumentSync } from "./documentSync.ts";

const acknowledgement = (update, stateRevision) => ({
  stateRevision, projectEpoch: update.projectEpoch,
  tabs: [{ path: update.path, text: update.text, documentRevision: update.expectedDocumentRevision + 1 }]
});

test("full snapshots from an older project or command cannot roll back state", () => {
  assert.equal(isNewerSnapshot(null, { stateRevision: 2 }), true);
  assert.equal(isNewerSnapshot({ stateRevision: 8 }, { stateRevision: 7 }), false);
  assert.equal(isNewerSnapshot({ stateRevision: 8 }, { stateRevision: 8 }), false);
  assert.equal(isNewerSnapshot({ stateRevision: 8 }, { stateRevision: 9 }), true);
});

test("typing coalesces per document and navigation waits for the latest queued text", async () => {
  const calls = [];
  const sync = new DocumentSync((update) => new Promise((resolve) => calls.push({ update, resolve })), () => {}, assert.fail);
  sync.queue(1, "sequence.donder", 0, "first");
  sync.queue(1, "sequence.donder", 0, "second");
  sync.queue(1, "sequence.donder", 0, "latest");
  let navigated = false;
  const navigation = sync.flush().then(() => { navigated = true; });
  assert.equal(calls.length, 1);
  calls[0].resolve(acknowledgement(calls[0].update, 2));
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(navigated, false);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].update.text, "latest");
  assert.equal(calls[1].update.expectedDocumentRevision, 1);
  calls[1].resolve(acknowledgement(calls[1].update, 3));
  await navigation;
  assert.equal(navigated, true);
  assert.equal(sync.pendingText(1, "sequence.donder"), null);
});

test("out-of-order acknowledgements across documents never roll back the full snapshot", async () => {
  const calls = [];
  let snapshot = null;
  const sync = new DocumentSync((update) => new Promise((resolve) => calls.push({ update, resolve })), (incoming) => {
    if (isNewerSnapshot(snapshot, incoming)) snapshot = incoming;
  }, assert.fail);
  sync.queue(1, "a.donder", 0, "A");
  sync.queue(1, "b.donder", 0, "B");
  calls[1].resolve(acknowledgement(calls[1].update, 7));
  calls[0].resolve(acknowledgement(calls[0].update, 6));
  await sync.flush();
  assert.equal(snapshot.stateRevision, 7);
});

test("failed text submission keeps the latest text and cancels the transition barrier", async () => {
  const error = new Error("document changed");
  const sync = new DocumentSync(() => Promise.reject(error), () => assert.fail("unexpected acknowledgement"), () => {});
  sync.queue(1, "a.donder", 0, "must survive");
  await assert.rejects(sync.flush(), /document changed/);
  assert.equal(sync.pendingText(1, "a.donder"), "must survive");
});

test("a rejected edit can only be rebased or discarded by an explicit conflict decision", async () => {
  const calls = [];
  const sync = new DocumentSync(async (update) => {
    calls.push(update);
    if (calls.length === 1) throw new Error("document changed");
    return acknowledgement(update, 9);
  }, () => {}, () => {});
  sync.queue(1, "a.donder", 0, "mine");
  await assert.rejects(sync.flush());
  sync.resolveFailure(1, "a.donder", 4);
  await sync.flush();
  assert.equal(calls[1].expectedDocumentRevision, 4);
  assert.equal(calls[1].text, "mine");
  const rejected = new DocumentSync(() => Promise.reject(new Error("deleted")), () => {}, () => {});
  rejected.queue(1, "b.donder", 1, "pending");
  await assert.rejects(rejected.flush());
  rejected.resolveFailure(1, "b.donder", null);
  await rejected.flush();
  assert.equal(rejected.pendingText(1, "b.donder"), null);
});

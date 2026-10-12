import assert from "node:assert/strict";
import test from "node:test";
import { setImmediate } from "node:timers";
import { isNewerSnapshot } from "./snapshotState.ts";
import { DocumentSync } from "./documentSync.ts";

const acknowledgement = (update, stateRevision) => ({
  stateRevision, projectEpoch: update.projectEpoch,
  tabs: [{ path: update.path, documentRevision: update.expectedDocumentRevision + 1 }]
});

const insert = (text) => [{ range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } }, text }];

const loadedSync = (submit, onSnapshot, onError, ...documents) => {
  const sync = new DocumentSync(submit, onSnapshot, onError, () => {});
  for (const [path, revision] of documents) sync.loaded(1, path, revision);
  return sync;
};

test("typing coalesces per document and navigation waits for the latest queued edits", async () => {
  const calls = [];
  const sync = loadedSync((update) => new Promise((resolve) => calls.push({ update, resolve })), () => {}, assert.fail, ["sequence.donder", 0]);
  sync.queue(1, "sequence.donder", insert("first"), () => "first");
  sync.queue(1, "sequence.donder", insert("second"), () => "secondfirst");
  sync.queue(1, "sequence.donder", insert("latest"), () => "latestsecondfirst");
  let navigated = false;
  const navigation = sync.flush().then(() => { navigated = true; });
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0].update.changes, [insert("first")]);
  calls[0].resolve(acknowledgement(calls[0].update, 2));
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(navigated, false);
  assert.equal(calls.length, 2);
  assert.deepEqual(calls[1].update.changes, [insert("second"), insert("latest")]);
  assert.equal(calls[1].update.expectedDocumentRevision, 1);
  calls[1].resolve(acknowledgement(calls[1].update, 3));
  await navigation;
  assert.equal(navigated, true);
  assert.equal(sync.pending(1, "sequence.donder"), false);
  assert.equal(sync.revision(1, "sequence.donder"), 2);
});

test("out-of-order acknowledgements across documents never roll back the full snapshot", async () => {
  const calls = [];
  let snapshot = null;
  const sync = loadedSync((update) => new Promise((resolve) => calls.push({ update, resolve })), (incoming) => {
    if (isNewerSnapshot(snapshot, incoming)) snapshot = incoming;
  }, assert.fail, ["a.donder", 0], ["b.donder", 0]);
  sync.queue(1, "a.donder", insert("A"), () => "A");
  sync.queue(1, "b.donder", insert("B"), () => "B");
  calls[1].resolve(acknowledgement(calls[1].update, 7));
  calls[0].resolve(acknowledgement(calls[0].update, 6));
  await sync.flush();
  assert.equal(snapshot.stateRevision, 7);
});

test("failed edit submission keeps the edits and cancels the transition barrier", async () => {
  const error = new Error("document changed");
  const sync = loadedSync(() => Promise.reject(error), () => assert.fail("unexpected acknowledgement"), () => {}, ["a.donder", 0]);
  sync.queue(1, "a.donder", insert("must survive"), () => "must survive");
  await assert.rejects(sync.flush(), /document changed/);
  assert.equal(sync.pending(1, "a.donder"), true);
});

test("a rejected edit can only be rebased or discarded by an explicit conflict decision", async () => {
  const calls = [];
  const sync = loadedSync(async (update) => {
    calls.push(update);
    if (calls.length === 1) throw new Error("document changed");
    return acknowledgement(update, 9);
  }, () => {}, () => {}, ["a.donder", 0]);
  sync.queue(1, "a.donder", insert("mine"), () => "mine");
  await assert.rejects(sync.flush());
  sync.resolveFailure(1, "a.donder", 4);
  await sync.flush();
  assert.equal(calls[1].expectedDocumentRevision, 4);
  assert.equal(calls[1].changes.length, 1);
  assert.equal(calls[1].changes[0][0].text, "mine");
  const rejected = loadedSync(() => Promise.reject(new Error("deleted")), () => {}, () => {}, ["b.donder", 1]);
  rejected.queue(1, "b.donder", insert("pending"), () => "pending");
  await assert.rejects(rejected.flush());
  rejected.resolveFailure(1, "b.donder", null);
  await rejected.flush();
  assert.equal(rejected.pending(1, "b.donder"), false);
  assert.notEqual(rejected.revision(1, "b.donder"), 1);
});
